//! Audio analysis pipeline (FFT) running on a dedicated OS thread.
//!
//! Previously `FFTFeedSource` (audio callback thread) and the FFT broadcast
//! task (audio player tokio runtime) shared an
//! `Arc<ParkingLotRwLock<AudioProcessor>>`. That gave us `try_write`-based
//! non-blocking pushes from the audio thread, but the FFT task still tied up
//! the audio player's `current_thread` runtime for ~1 ms per tick during
//! `process_frame`, and the shared lock was a potential priority-inversion
//! risk if a low-priority thread held the write lock while the audio thread
//! tried to push.
//!
//! Here the `AudioProcessor` lives entirely on a dedicated `audio-analysis`
//! OS thread. Reliable controls and best-effort PCM use separate channels.
//! The PCM channel is deliberately tiny and bounded: analysis may drop audio
//! under load, but it can never backlog memory or stall playback/decoding.
//!
//! The FFT runs once per `FFT_HOP` on the latest window, as AMLL's pacemaker
//! does, and each frame also publishes the lowest bins as `LowFreqBins`; the
//! frontend runs the low-pass gradient/smoothing on them per display frame.
//! The decoder taps PCM well ahead of the device, so each block carries how
//! long it will wait in the playback queues and is held back by that long; the
//! visuals then follow what is audible instead of what was decoded.
//!
//! Events leave through the same `tokio::sync::mpsc::UnboundedSender` that
//! every other player event uses, so the WS forwarder picks them up unchanged.

use std::collections::VecDeque;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use audio_analysis::{FftConfig, FftProcessor};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::warn;

use crate::types::{AudioThreadEvent, AudioThreadEventMessage};

const FFT_SIZE: usize = 2048;
const TICK: Duration = Duration::from_millis(16);
const FFT_HOP: Duration = Duration::from_millis(50);
/// Covers the deepest playback queue (48 deck + 48 output blocks): after a start
/// or seek the decoder fills it in a burst, and those first blocks are the ones
/// played first. A shorter channel dropped most of them, so the opening second
/// had no visuals.
const PCM_QUEUE_CAPACITY: usize = 128;
/// Bound on blocks waiting out their lead; a runaway lead releases the oldest.
const MAX_DELAYED_BLOCKS: usize = 512;
/// Distance between the `result_buf` entries AMLL's `vec_interp` reads for
/// bins 0 and 1 (`RESULT_BUF_SIZE >> 7`).
const BIN_STRIDE: usize = 16;
const LOW_BIN_COUNT: usize = 2;
/// A playing stream produces a frame every `FFT_HOP`; a gap this long means the
/// decoder or the playback queues stalled, which shows up as a dead background.
const FRAME_GAP_WARN: Duration = Duration::from_millis(300);

/// Reliable controls from the player and decoder threads.
pub enum AnalysisCommand {
    Clear,
    SetEnabled {
        enabled: bool,
    },
    SetFftEnabled {
        enabled: bool,
    },
    SetFreqRange {
        from: f32,
        to: f32,
    },
    /// Playback paused/resumed: blocks stop ageing while the device is stopped.
    SetPaused {
        paused: bool,
    },
}

pub struct AnalysisPcm {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub sample_rate: u32,
    /// Time until the end of this block is audible (queued audio ahead of it).
    pub lead: Duration,
    pub recycle: Option<mpsc::Sender<Vec<f32>>>,
}

/// PCM held until the device reaches it. Release times are monotonic and are
/// pushed back by however long playback was paused.
#[derive(Default)]
struct DelayQueue {
    blocks: VecDeque<(Instant, AnalysisPcm)>,
    paused_at: Option<Instant>,
}

impl DelayQueue {
    fn push(&mut self, now: Instant, pcm: AnalysisPcm) {
        let at = self
            .blocks
            .back()
            .map_or(now + pcm.lead, |(last, _)| (*last).max(now + pcm.lead));
        self.blocks.push_back((at, pcm));
    }

    fn pop_due(&mut self, now: Instant) -> Option<AnalysisPcm> {
        if self.paused_at.is_some() {
            return None;
        }
        let due = self
            .blocks
            .front()
            .is_some_and(|(at, _)| *at <= now || self.blocks.len() > MAX_DELAYED_BLOCKS);
        due.then(|| self.blocks.pop_front().map(|(_, pcm)| pcm))
            .flatten()
    }

    fn len(&self) -> usize {
        self.blocks.len()
    }

    fn set_paused(&mut self, paused: bool, now: Instant) {
        match (paused, self.paused_at) {
            (true, None) => self.paused_at = Some(now),
            (false, Some(since)) => {
                let shift = now.saturating_duration_since(since);
                for (at, _) in &mut self.blocks {
                    *at += shift;
                }
                self.paused_at = None;
            }
            _ => {}
        }
    }

    fn drain(&mut self) -> impl Iterator<Item = AnalysisPcm> + '_ {
        self.blocks.drain(..).map(|(_, pcm)| pcm)
    }

    /// Blocks the device has already played (or, while paused, was about to).
    fn take_past(&mut self, now: Instant) -> Vec<AnalysisPcm> {
        let until = self.paused_at.unwrap_or(now);
        let mut past = Vec::new();
        while self.blocks.front().is_some_and(|(at, _)| *at <= until) {
            past.extend(self.blocks.pop_front().map(|(_, pcm)| pcm));
        }
        past
    }
}

#[derive(Clone)]
pub struct AnalysisSender {
    control_tx: mpsc::Sender<AnalysisCommand>,
    pcm_tx: mpsc::SyncSender<AnalysisPcm>,
}

impl AnalysisSender {
    pub fn send(&self, command: AnalysisCommand) -> Result<(), mpsc::SendError<AnalysisCommand>> {
        self.control_tx.send(command)
    }

    /// Submit PCM without ever waiting for the analysis thread. On overload
    /// the caller gets ownership back and can immediately recycle the buffer.
    pub fn try_send_pcm(&self, pcm: AnalysisPcm) -> Result<(), mpsc::TrySendError<AnalysisPcm>> {
        self.pcm_tx.try_send(pcm)
    }
}

/// Spawn the dedicated analysis OS thread and return a `Sender` for
/// commands plus its `JoinHandle` (so the owner can join on shutdown if
/// it wants to).
///
/// The loop terminates when every `Sender` has been dropped (the channel
/// returns `Disconnected`) or the event sink is closed.
pub fn spawn_analysis_thread(
    evt_sender: tokio_mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
) -> std::io::Result<(AnalysisSender, std::thread::JoinHandle<()>)> {
    let (control_tx, control_rx) = mpsc::channel::<AnalysisCommand>();
    let (pcm_tx, pcm_rx) = mpsc::sync_channel::<AnalysisPcm>(PCM_QUEUE_CAPACITY);
    let handle = std::thread::Builder::new()
        .name("audio-analysis".into())
        .spawn(move || analysis_loop(control_rx, pcm_rx, evt_sender))?;
    Ok((AnalysisSender { control_tx, pcm_tx }, handle))
}

type EventSink = tokio_mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>;

fn analysis_loop(
    control_rx: mpsc::Receiver<AnalysisCommand>,
    pcm_rx: mpsc::Receiver<AnalysisPcm>,
    evt: EventSink,
) {
    let mut state = Analysis::new(evt);
    loop {
        // Controls are reliable and always drained before best-effort PCM.
        while let Ok(cmd) = control_rx.try_recv() {
            if !state.command(cmd, Instant::now()) {
                return;
            }
        }
        if !state.tick(&pcm_rx, Instant::now()) {
            return;
        }

        // Sleep on the reliable control channel rather than polling every
        // millisecond. A control wakes the thread immediately; PCM remains
        // best-effort and is picked up on the next analysis tick.
        let command = if state.enabled {
            control_rx.recv_timeout(TICK)
        } else {
            // Re-enabling and shutdown both arrive on this channel. No timer
            // can make disabled analysis useful, so it needs no periodic wake.
            control_rx
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        };
        match command {
            Ok(cmd) => {
                if !state.command(cmd, Instant::now()) {
                    return;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

struct Analysis {
    fft: FftProcessor,
    delayed: DelayQueue,
    pending_mono_samples: usize,
    sample_rate: u32,
    enabled: bool,
    fft_enabled: bool,
    /// Next FFT slot. A fixed grid: ticks are 16 ms apart, so measuring the hop
    /// from the last run would stretch 50 ms to 64 ms.
    next_fft: Instant,
    last_frame: Option<Instant>,
    evt: EventSink,
}

impl Analysis {
    fn new(evt: EventSink) -> Self {
        Self {
            fft: FftProcessor::new(FftConfig {
                output_size: FFT_SIZE,
                freq_min: 80.0,
                freq_max: 2000.0,
            }),
            delayed: DelayQueue::default(),
            pending_mono_samples: 0,
            sample_rate: 44_100,
            enabled: true,
            fft_enabled: true,
            next_fft: Instant::now(),
            last_frame: None,
            evt,
        }
    }

    /// Returns `false` once the event sink is closed.
    fn command(&mut self, cmd: AnalysisCommand, now: Instant) -> bool {
        // Every control either stops the stream or restarts it, so the next
        // frame is not late for any reason worth reporting.
        self.last_frame = None;
        match cmd {
            // A seek or track change drops the audio in flight but keeps the
            // FFT smoothing: resetting it halves the first frame's magnitudes
            // (the smoothing averages with zero) and the background dipped on
            // every jump. Nothing stale is read, because `pending_mono_samples`
            // holds the next frame back until a whole window of new audio is in.
            AnalysisCommand::Clear => {
                self.pending_mono_samples = 0;
                self.discard_delayed();
            }
            AnalysisCommand::SetEnabled { enabled } => {
                if self.enabled == enabled {
                    return true;
                }
                self.enabled = enabled;
                self.fft.clear_queue();
                self.pending_mono_samples = 0;
                // The frontend turns analysis off while paused. What is already
                // queued is still going to be played after resume, so keep it;
                // dropping it left the first `lead` of audio without visuals.
                if enabled {
                    for pcm in self.delayed.take_past(now) {
                        recycle(pcm);
                    }
                }
            }
            AnalysisCommand::SetFftEnabled { enabled } => self.fft_enabled = enabled,
            AnalysisCommand::SetFreqRange { from, to } => self.fft.set_freq_range(from, to),
            AnalysisCommand::SetPaused { paused } => self.delayed.set_paused(paused, now),
        }
        true
    }

    /// One pass: intake, release what the device has reached, FFT on its hop.
    /// Returns `false` once the sink is closed.
    fn tick(&mut self, pcm_rx: &mpsc::Receiver<AnalysisPcm>, now: Instant) -> bool {
        for _ in 0..PCM_QUEUE_CAPACITY {
            let Ok(pcm) = pcm_rx.try_recv() else {
                break;
            };
            if self.enabled {
                self.delayed.push(now, pcm);
            } else {
                recycle(pcm);
            }
        }
        if !self.enabled {
            return true;
        }

        while let Some(mut pcm) = self.delayed.pop_due(now) {
            let gain = downmix_gain(pcm.channels);
            if gain != 1.0 {
                pcm.samples.iter_mut().for_each(|s| *s *= gain);
            }
            self.pending_mono_samples +=
                self.fft
                    .push_interleaved_pcm(&pcm.samples, pcm.channels, pcm.sample_rate);
            self.sample_rate = pcm.sample_rate.max(1);
            recycle(pcm);
        }

        if now >= self.next_fft && self.pending_mono_samples >= FFT_SIZE {
            return self.fft_frame(now);
        }
        true
    }

    fn fft_frame(&mut self, now: Instant) -> bool {
        self.next_fft = (self.next_fft + FFT_HOP).max(now + FFT_HOP / 2);
        if let Some(prev) = self.last_frame.replace(now) {
            let gap = now.duration_since(prev);
            if gap > FRAME_GAP_WARN {
                warn!(
                    "低频分析 {} ms 没有新 FFT 帧（排队 {} 块，待处理 {} 采样）",
                    gap.as_millis(),
                    self.delayed.len(),
                    self.pending_mono_samples
                );
            }
        }
        self.fft.read_spectrum_raw();
        let raw = self.fft.raw_spectrum();
        let drained = (FFT_HOP.as_secs_f32() * self.sample_rate as f32) as usize;
        self.pending_mono_samples = self.pending_mono_samples.saturating_sub(drained.max(1));

        let bins = (0..LOW_BIN_COUNT).map(|i| raw[i * BIN_STRIDE]).collect();
        let sent = self
            .evt
            .send(AudioThreadEventMessage::new(
                String::new(),
                Some(AudioThreadEvent::LowFreqBins { bins }),
            ))
            .is_ok();
        sent && (!self.fft_enabled
            || self
                .evt
                .send(AudioThreadEventMessage::new(
                    String::new(),
                    Some(AudioThreadEvent::FFTData { data: raw.to_vec() }),
                ))
                .is_ok())
    }

    fn discard_delayed(&mut self) {
        for pcm in self.delayed.drain() {
            recycle(pcm);
        }
    }
}

/// `push_interleaved_pcm` averages channels; AMLL's ffmpeg downmix is
/// `0.7071 * (L + R)`, so a centred bass note comes out 1.41x louder. The FFT
/// levels feed a log curve and a fixed 0.35 threshold, and at the averaged level
/// the output moves about a quarter as much. Stereo only: other layouts use
/// ffmpeg's own matrix and are left averaged.
fn downmix_gain(channels: u16) -> f32 {
    if channels == 2 {
        std::f32::consts::SQRT_2
    } else {
        1.0
    }
}

/// Hand the buffer back to the decoder's pool.
fn recycle(pcm: AnalysisPcm) {
    let AnalysisPcm {
        mut samples,
        recycle,
        ..
    } = pcm;
    if let Some(tx) = recycle {
        samples.clear();
        let _ = tx.send(samples);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(value: f32) -> AnalysisPcm {
        AnalysisPcm {
            samples: vec![value; 16],
            channels: 2,
            sample_rate: 48_000,
            lead: Duration::ZERO,
            recycle: None,
        }
    }

    fn wait_event(
        events: &mut tokio_mpsc::UnboundedReceiver<AudioThreadEventMessage<AudioThreadEvent>>,
        matches: impl Fn(&AudioThreadEvent) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(message) = events.try_recv() {
                if message.data.as_ref().is_some_and(&matches) {
                    return;
                }
            }
            assert!(Instant::now() < deadline, "analysis event did not arrive");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn disabled_analysis_returns_every_queued_buffer_and_exits_on_disconnect() {
        let (control_tx, control_rx) = mpsc::channel();
        let (pcm_tx, pcm_rx) = mpsc::sync_channel(PCM_QUEUE_CAPACITY);
        let (recycle_tx, recycle_rx) = mpsc::channel();
        let (evt_tx, mut evt_rx) = tokio_mpsc::unbounded_channel();
        let (done_tx, done_rx) = mpsc::channel();
        control_tx
            .send(AnalysisCommand::SetEnabled { enabled: false })
            .unwrap();
        for _ in 0..PCM_QUEUE_CAPACITY {
            pcm_tx
                .send(AnalysisPcm {
                    recycle: Some(recycle_tx.clone()),
                    ..pcm(0.5)
                })
                .unwrap();
        }
        let worker = std::thread::spawn(move || {
            analysis_loop(control_rx, pcm_rx, evt_tx);
            done_tx.send(()).unwrap();
        });
        for _ in 0..PCM_QUEUE_CAPACITY {
            let buffer = recycle_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(buffer.is_empty());
            assert!(buffer.capacity() >= 16);
        }
        assert!(evt_rx.try_recv().is_err());
        drop(control_tx);
        drop(pcm_tx);
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn controls_wake_disabled_analysis_and_reenabling_restores_fft() {
        let (events, mut received) = tokio_mpsc::unbounded_channel();
        let (sender, worker) = spawn_analysis_thread(events).unwrap();
        sender
            .send(AnalysisCommand::SetEnabled { enabled: false })
            .unwrap();
        sender
            .send(AnalysisCommand::SetFreqRange {
                from: 100.0,
                to: 10_000.0,
            })
            .unwrap();
        sender
            .send(AnalysisCommand::SetFftEnabled { enabled: true })
            .unwrap();
        sender.send(AnalysisCommand::Clear).unwrap();
        sender
            .send(AnalysisCommand::SetEnabled { enabled: true })
            .unwrap();
        assert!(sender
            .try_send_pcm(AnalysisPcm {
                samples: vec![0.5; FFT_SIZE * 2],
                channels: 1,
                sample_rate: 48_000,
                lead: Duration::ZERO,
                recycle: None,
            })
            .is_ok());
        wait_event(
            &mut received,
            |event| matches!(event, AudioThreadEvent::LowFreqBins { bins } if bins.len() == LOW_BIN_COUNT),
        );
        wait_event(
            &mut received,
            |event| matches!(event, AudioThreadEvent::FFTData { data } if !data.is_empty()),
        );
        drop(sender);
        worker.join().unwrap();
    }

    #[test]
    fn stereo_downmix_matches_ffmpeg_not_the_average() {
        // ffmpeg: mono = 0.7071 * (L + R); the processor averages, so scale by sqrt(2).
        let (l, r) = (0.5f32, 0.1f32);
        assert!((downmix_gain(2) * (l + r) / 2.0 - 0.7071 * (l + r)).abs() < 1e-4);
        assert_eq!(downmix_gain(1), 1.0);
    }

    #[test]
    fn opening_burst_fits_the_channel_and_queued_audio_survives_a_toggle() {
        let (events, _received) = tokio_mpsc::unbounded_channel();
        let (sender, worker) = spawn_analysis_thread(events).unwrap();
        // Deepest real queue: the decoder fills it in a burst after a start/seek.
        for _ in 0..96 {
            assert!(sender.try_send_pcm(pcm(0.1)).is_ok());
        }
        drop(sender);
        worker.join().unwrap();

        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut queue = DelayQueue::default();
        for lead in [100, 300, 500] {
            queue.push(
                t0,
                AnalysisPcm {
                    lead: Duration::from_millis(lead),
                    ..pcm(0.0)
                },
            );
        }
        // Analysis off and on again at 350 ms: only the played block goes.
        assert_eq!(queue.take_past(at(350)).len(), 2);
        assert!(queue.pop_due(at(500)).is_some());
    }

    #[test]
    fn blocks_wait_out_their_lead_and_pausing_stops_the_clock() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let led = |ms: u64| AnalysisPcm {
            lead: Duration::from_millis(ms),
            ..pcm(0.0)
        };
        let mut queue = DelayQueue::default();
        queue.push(t0, led(200));
        // A shallower queue later must not overtake the block ahead of it.
        queue.push(at(10), led(50));
        assert!(queue.pop_due(at(199)).is_none());
        assert!(queue.pop_due(at(200)).is_some());
        assert!(queue.pop_due(at(200)).is_some());

        queue.push(t0, led(100));
        queue.set_paused(true, at(40));
        assert!(queue.pop_due(at(500)).is_none());
        queue.set_paused(false, at(540));
        // 500 ms paused: the block due at 100 ms is now due at 600 ms.
        assert!(queue.pop_due(at(599)).is_none());
        assert!(queue.pop_due(at(600)).is_some());
    }

    #[test]
    fn pcm_overload_is_bounded_and_returns_ownership() {
        let (control_tx, control_rx) = mpsc::channel();
        let (pcm_tx, _pcm_rx) = mpsc::sync_channel(2);
        let sender = AnalysisSender { control_tx, pcm_tx };

        assert!(sender.try_send_pcm(pcm(1.0)).is_ok());
        assert!(sender.try_send_pcm(pcm(2.0)).is_ok());
        let dropped = match sender.try_send_pcm(pcm(3.0)) {
            Err(mpsc::TrySendError::Full(pcm)) => pcm,
            _ => panic!("full PCM queue must reject immediately"),
        };
        assert_eq!(dropped.samples[0], 3.0);

        // A saturated PCM queue is independent of reliable controls.
        sender.send(AnalysisCommand::Clear).unwrap();
        assert!(matches!(control_rx.try_recv(), Ok(AnalysisCommand::Clear)));
    }
}
