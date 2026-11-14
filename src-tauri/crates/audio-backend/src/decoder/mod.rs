mod convert;
pub(crate) mod ffmpeg;
mod input;
mod pcm;
mod resample;
mod symphonia;
pub(crate) mod tempo;

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc as tokio_mpsc, oneshot};
use tracing::warn;

use crate::analysis::{AnalysisCommand, AnalysisPcm, AnalysisSender};
use crate::error::{AudioError, AudioResult};
use crate::output::{OutputWriter, PushCancel};

use convert::FrameConverter;
pub(crate) use input::OpenedDecoder;
#[cfg(test)]
use pcm::append_available_interleaved_frames;
use pcm::PcmSource;
#[cfg(test)]
use symphonia::SeekableSymphoniaSource;

pub(crate) trait PlaybackSink: Clone + Send + 'static {
    fn push_block(&self, block: Vec<f32>, cancel: PushCancel<'_>) -> bool;
    fn push_timed_block(
        &self,
        block: Vec<f32>,
        _position: crate::output::PcmPosition,
        cancel: PushCancel<'_>,
    ) -> bool {
        self.push_block(block, cancel)
    }
    fn flush_for_seek(&self);
    fn set_paused(&self, paused: bool);
    fn queued_samples(&self) -> usize;
    /// Device delay past `queued_samples`; `0` until measured.
    fn output_latency_us(&self) -> u32;
    fn generation(&self) -> u64;

    fn take_recycled_buffer(&self, capacity: usize) -> Vec<f32> {
        Vec::with_capacity(capacity)
    }
}

impl PlaybackSink for OutputWriter {
    fn push_block(&self, block: Vec<f32>, cancel: PushCancel<'_>) -> bool {
        OutputWriter::push_block(self, block, cancel)
    }

    fn push_timed_block(
        &self,
        block: Vec<f32>,
        position: crate::output::PcmPosition,
        cancel: PushCancel<'_>,
    ) -> bool {
        self.push_block_with_anchor(
            block,
            Some(crate::output::OutputAnchor {
                source: position,
                transition: 0,
                phase: 0,
            }),
            cancel,
        )
    }

    fn flush_for_seek(&self) {
        OutputWriter::flush(self);
    }

    fn set_paused(&self, paused: bool) {
        OutputWriter::set_paused(self, paused);
    }

    fn queued_samples(&self) -> usize {
        OutputWriter::queued_samples(self)
    }

    fn output_latency_us(&self) -> u32 {
        OutputWriter::output_latency_us(self)
    }

    fn generation(&self) -> u64 {
        OutputWriter::generation(self)
    }

    fn take_recycled_buffer(&self, capacity: usize) -> Vec<f32> {
        OutputWriter::take_recycled_buffer(self, capacity)
    }
}

// ── DecoderHandle ────────────────────────────────────────────────

enum DecoderControl {
    Seek {
        pos: Duration,
        epoch: u64,
        ack: oneshot::Sender<Result<(), String>>,
    },
    ArmTempo {
        start: f64,
        processor: Box<tempo::AutoMixStretch>,
        ack: oneshot::Sender<Result<(), String>>,
    },
    CancelTempo,
    SetPaused(bool),
    Stop,
}

/// Handle for controlling the active source. This mirrors AMLL's decoder
/// handle: the player sends seek intent directly to the decoder/source layer
/// instead of waiting for `rodio::Sink::try_seek()` to round-trip through
/// rodio's periodic access callback.
pub struct DecoderHandle {
    control_tx: mpsc::Sender<DecoderControl>,
    stop_flag: Arc<AtomicBool>,
    seek_epoch: Arc<AtomicU64>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl DecoderHandle {
    /// Interrupt a decode thread parked on a full sink queue. Producers park
    /// for about the time the consumer needs to free a slot, which is far
    /// longer than a control message should take to land, so every control
    /// path pokes the thread instead of waiting for that park to expire.
    fn wake(&self) {
        if let Some(thread) = self.thread.as_ref() {
            thread.thread().unpark();
        }
    }

    pub(crate) fn seek(&self, pos: Duration) -> AudioResult<DecoderSeekAck> {
        let (ack_tx, ack_rx) = oneshot::channel();
        let epoch = self
            .seek_epoch
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        self.control_tx
            .send(DecoderControl::Seek {
                pos,
                epoch,
                ack: ack_tx,
            })
            .map_err(|_| AudioError::ThreadError("decoder control channel closed".into()))?;
        self.wake();

        Ok(DecoderSeekAck { ack_rx })
    }

    pub(crate) fn arm_tempo(
        &self,
        start: f64,
        processor: tempo::AutoMixStretch,
    ) -> AudioResult<DecoderSeekAck> {
        let (ack, ack_rx) = oneshot::channel();
        self.control_tx
            .send(DecoderControl::ArmTempo {
                start,
                processor: Box::new(processor),
                ack,
            })
            .map_err(|_| AudioError::ThreadError("decoder control channel closed".into()))?;
        self.wake();
        Ok(DecoderSeekAck { ack_rx })
    }

    pub(crate) fn cancel_tempo(&self) {
        let _ = self.control_tx.send(DecoderControl::CancelTempo);
        self.wake();
    }

    pub fn set_paused(&self, paused: bool) -> AudioResult<()> {
        self.control_tx
            .send(DecoderControl::SetPaused(paused))
            .map_err(|_| AudioError::ThreadError("decoder control channel closed".into()))?;
        self.wake();
        Ok(())
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Release);
        self.seek_epoch.fetch_add(1, Ordering::AcqRel);
        let _ = self.control_tx.send(DecoderControl::Stop);
        self.wake();
    }
}

impl Drop for DecoderHandle {
    fn drop(&mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            join_decoder_thread_async(thread);
        }
    }
}

pub(crate) struct DecoderSeekAck {
    ack_rx: oneshot::Receiver<Result<(), String>>,
}

impl DecoderSeekAck {
    pub(crate) async fn wait(self) -> AudioResult<()> {
        match tokio::time::timeout(Duration::from_secs(2), self.ack_rx).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(err))) => Err(AudioError::Decode(err)),
            Ok(Err(_)) => Err(AudioError::ThreadError(
                "decoder seek acknowledgement channel closed".into(),
            )),
            Err(_) => Err(AudioError::ThreadError("decoder seek timed out".into())),
        }
    }
}

fn join_decoder_thread_async(handle: std::thread::JoinHandle<()>) {
    let _ = std::thread::Builder::new()
        .name("audio-decode-join".into())
        .spawn(move || {
            let _ = handle.join();
        });
}

#[derive(Debug, Clone, Copy)]
pub enum DecoderEvent {
    Finished {
        playback_id: u64,
    },
    /// The decoder ran ahead of a download still landing and is holding for it
    /// (`starved`), or has enough again. Only streamed sources emit it.
    Starved {
        playback_id: u64,
        starved: bool,
    },
}

const DECODE_BLOCK_FRAMES: usize = 512;
const PRE_ROLL_SILENCE_FRAMES: usize = 1024;
const START_RAMP_FRAMES: usize = 2048;
const SEEK_RAMP_FRAMES: usize = 512;
const SEEK_CONTROL_CHECK_FRAMES: usize = 64;
// A paused decoder resumes, seeks and stops through the control channel, which
// wakes this wait immediately. The timeout is only a backstop for `stop_flag`
// being set without a message, so it costs nothing to keep it low-frequency.
const PAUSED_POLL_INTERVAL: Duration = Duration::from_millis(200);

#[allow(clippy::too_many_arguments)]
pub fn spawn_playback_decoder<S>(
    path: &Path,
    initial_position: Option<f64>,
    output: S,
    output_channels: u16,
    output_sample_rate: u32,
    analysis_tx: AnalysisSender,
    analysis_enabled: Arc<AtomicBool>,
    event_tx: tokio_mpsc::UnboundedSender<DecoderEvent>,
    playback_id: u64,
    start_paused: bool,
) -> AudioResult<DecoderHandle>
where
    S: PlaybackSink,
{
    let opened = OpenedDecoder::open_with(|| {
        let input = crate::source::open_playback(crate::source::SourceLocator::from_path(path))?;
        let extension = input.extension().map(str::to_string);
        Ok((input, extension))
    })?;
    match opened {
        OpenedDecoder::Symphonia(source) => spawn_decoder_for_source(
            source,
            path,
            initial_position,
            output,
            output_channels,
            output_sample_rate,
            analysis_tx,
            analysis_enabled,
            event_tx,
            playback_id,
            start_paused,
        ),
        OpenedDecoder::Ffmpeg(source) => spawn_decoder_for_source(
            source,
            path,
            initial_position,
            output,
            output_channels,
            output_sample_rate,
            analysis_tx,
            analysis_enabled,
            event_tx,
            playback_id,
            start_paused,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_decoder_for_source<S: PlaybackSink, D: PcmSource>(
    mut source: D,
    path: &Path,
    initial_position: Option<f64>,
    output: S,
    output_channels: u16,
    output_sample_rate: u32,
    analysis_tx: AnalysisSender,
    analysis_enabled: Arc<AtomicBool>,
    event_tx: tokio_mpsc::UnboundedSender<DecoderEvent>,
    playback_id: u64,
    start_paused: bool,
) -> AudioResult<DecoderHandle> {
    let input_channels = source.channels().get();
    let input_sample_rate = source.sample_rate().get();

    if let Some(position) = initial_position.filter(|position| *position > 0.0) {
        source
            .try_seek(Duration::from_secs_f64(position))
            .map_err(|e| AudioError::Decode(format!("initial seek failed: {e}")))?;
    }

    let gate = crate::source::growing::lookup(path)
        .map(|progress| StreamGate::new(progress, rodio::Source::total_duration(&source)));

    let (control_tx, control_rx) = mpsc::channel();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let seek_epoch = Arc::new(AtomicU64::new(0));
    let worker_stop = Arc::clone(&stop_flag);
    let worker_seek_epoch = Arc::clone(&seek_epoch);
    let output_channels = output_channels.max(1);
    let output_sample_rate = output_sample_rate.max(1);

    let thread = std::thread::Builder::new()
        .name("audio-decode".into())
        .spawn(move || {
            // Held for the thread's lifetime: this thread is the only producer
            // for the deck queue, so its scheduling latency is playback's. It
            // renders one DECODE_BLOCK_FRAMES block per wake, which is the
            // period the deadline-based backends want to know about.
            let _priority = crate::rt_priority::promote_current_thread(
                crate::rt_priority::AudioThreadKind::Decode,
                DECODE_BLOCK_FRAMES,
                output_sample_rate,
            );
            let mut worker = DecodeWorker::new(
                source,
                input_channels,
                input_sample_rate,
                output,
                output_channels,
                output_sample_rate,
                analysis_tx,
                analysis_enabled,
                control_rx,
                event_tx,
                playback_id,
                worker_stop,
                worker_seek_epoch,
                start_paused,
                gate,
                initial_position.unwrap_or(0.0),
            );
            worker.run();
        })
        .map_err(|e| AudioError::ThreadError(format!("spawn audio-decode thread: {e}")))?;

    Ok(DecoderHandle {
        control_tx,
        stop_flag,
        seek_epoch,
        thread: Some(thread),
    })
}

struct DecodeWorker<S: PlaybackSink, D: PcmSource> {
    frames: FrameConverter<D>,
    output: S,
    output_channels: u16,
    output_sample_rate: u32,
    analysis_tx: AnalysisSender,
    analysis_enabled: Arc<AtomicBool>,
    analysis_recycle_tx: mpsc::Sender<Vec<f32>>,
    analysis_recycle_rx: mpsc::Receiver<Vec<f32>>,
    control_rx: mpsc::Receiver<DecoderControl>,
    event_tx: tokio_mpsc::UnboundedSender<DecoderEvent>,
    playback_id: u64,
    stop_flag: Arc<AtomicBool>,
    seek_epoch: Arc<AtomicU64>,
    paused: bool,
    pre_roll_frames_remaining: usize,
    start_ramp_total_frames: usize,
    start_ramp_frames_remaining: usize,
    applied_seek_epoch: u64,
    pending_seek_flush_epoch: Option<u64>,
    gate: Option<StreamGate>,
    tempo: Option<(f64, Box<tempo::AutoMixStretch>)>,
    cancel_tempo: bool,
    source_position: f64,
    emitted_position: f64,
    raw_spare: Option<Vec<f32>>,
}

/// Watermarks for a source that is still downloading. The decoder stops
/// producing when the bytes ahead of it fall below `low` and resumes at `high`;
/// the gap is hysteresis, so a link running just under the bitrate stalls in
/// long rebuffers instead of flapping every block.
struct StreamGate {
    progress: Arc<crate::source::growing::Progress>,
    low: u64,
    high: u64,
    starved: bool,
}

const GATE_LOW_SECONDS: f64 = 1.0;
const GATE_HIGH_SECONDS: f64 = 3.0;
/// Bytes per second assumed when the container declares no duration.
const GATE_FALLBACK_BYTES_PER_SEC: f64 = 128.0 * 1024.0;
const GATE_POLL: Duration = Duration::from_millis(100);

impl StreamGate {
    fn new(progress: Arc<crate::source::growing::Progress>, duration: Option<Duration>) -> Self {
        let rate = duration
            .map(|d| d.as_secs_f64())
            .filter(|d| *d > 0.0)
            .map_or(GATE_FALLBACK_BYTES_PER_SEC, |d| progress.total() as f64 / d);
        let low = ((rate * GATE_LOW_SECONDS) as u64).max(16 * 1024);
        let high = ((rate * GATE_HIGH_SECONDS) as u64).max(low * 2);
        Self {
            progress,
            low,
            high,
            starved: false,
        }
    }

    /// Whether the decoder must hold now. A finished download never holds, which
    /// is also what releases the tail: less than `high` may be left to arrive.
    fn should_hold(&self) -> bool {
        if self.progress.is_finished() || self.progress.reaches_end() {
            return false;
        }
        self.progress.lead() < if self.starved { self.high } else { self.low }
    }
}

/// Frames of pre-roll silence to emit in one run of the decode loop.
///
/// Must never return zero while `remaining > 0`: the loop `continue`s on this
/// value without consuming a source frame, so a zero-length run would spin the
/// decode thread forever. It also must not run past the next control-check
/// boundary (seek latency) or the end of the block (reserved capacity).
#[inline]
fn pre_roll_run_frames(remaining: usize, frame_index: usize) -> usize {
    debug_assert!(remaining > 0);
    debug_assert!(frame_index < DECODE_BLOCK_FRAMES);
    let until_control_check = SEEK_CONTROL_CHECK_FRAMES - (frame_index % SEEK_CONTROL_CHECK_FRAMES);
    remaining
        .min(until_control_check)
        .min(DECODE_BLOCK_FRAMES - frame_index)
}

#[allow(clippy::too_many_arguments)]
impl<S: PlaybackSink, D: PcmSource> DecodeWorker<S, D> {
    fn new(
        source: D,
        input_channels: u16,
        input_sample_rate: u32,
        output: S,
        output_channels: u16,
        output_sample_rate: u32,
        analysis_tx: AnalysisSender,
        analysis_enabled: Arc<AtomicBool>,
        control_rx: mpsc::Receiver<DecoderControl>,
        event_tx: tokio_mpsc::UnboundedSender<DecoderEvent>,
        playback_id: u64,
        stop_flag: Arc<AtomicBool>,
        seek_epoch: Arc<AtomicU64>,
        paused: bool,
        gate: Option<StreamGate>,
        initial_position: f64,
    ) -> Self {
        let (analysis_recycle_tx, analysis_recycle_rx) = mpsc::channel();
        Self {
            frames: FrameConverter::new(
                source,
                input_channels,
                input_sample_rate,
                output_channels,
                output_sample_rate,
            ),
            output,
            output_channels,
            output_sample_rate,
            analysis_tx,
            analysis_enabled,
            analysis_recycle_tx,
            analysis_recycle_rx,
            control_rx,
            event_tx,
            playback_id,
            stop_flag,
            seek_epoch,
            paused,
            pre_roll_frames_remaining: PRE_ROLL_SILENCE_FRAMES,
            start_ramp_total_frames: START_RAMP_FRAMES,
            start_ramp_frames_remaining: START_RAMP_FRAMES,
            applied_seek_epoch: 0,
            pending_seek_flush_epoch: None,
            gate,
            tempo: None,
            cancel_tempo: false,
            source_position: initial_position,
            emitted_position: initial_position,
            raw_spare: None,
        }
    }

    fn run(&mut self) {
        let mut frame = vec![0.0f32; self.output_channels as usize];

        loop {
            if self.stop_flag.load(Ordering::Acquire) || !self.drain_controls() {
                break;
            }

            if self.paused {
                if !self.wait_while_paused() {
                    break;
                }
                continue;
            }

            if self.seek_epoch.load(Ordering::Acquire) != self.applied_seek_epoch {
                std::thread::yield_now();
                continue;
            }

            if !self.hold_for_stream() {
                break;
            }
            if self.paused || self.seek_epoch.load(Ordering::Acquire) != self.applied_seek_epoch {
                continue;
            }

            if self.cancel_tempo {
                self.flush_tempo();
                self.cancel_tempo = false;
            }
            let block_seek_epoch = self.applied_seek_epoch;
            let source_start = self.source_position;
            let mut block_frames = DECODE_BLOCK_FRAMES;
            if let Some((start, _)) = &self.tempo {
                if *start > source_start + 0.5 / self.output_sample_rate as f64 {
                    block_frames = block_frames
                        .min(
                            ((*start - source_start) * self.output_sample_rate as f64).round()
                                as usize,
                        )
                        .max(1);
                }
            }
            let generation = self.output.generation();
            let block_capacity = DECODE_BLOCK_FRAMES * self.output_channels as usize;
            let mut block = self
                .raw_spare
                .take()
                .unwrap_or_else(|| self.output.take_recycled_buffer(block_capacity));
            block.clear();
            if block.capacity() < block_capacity {
                block.reserve(block_capacity - block.capacity());
            }
            let mut analysis_block = if self.analysis_enabled.load(Ordering::Acquire) {
                Some(
                    self.analysis_recycle_rx
                        .try_recv()
                        .unwrap_or_else(|_| Vec::with_capacity(block_capacity)),
                )
            } else {
                None
            };
            if let Some(block) = analysis_block.as_mut() {
                block.clear();
            }
            let mut ended = false;
            let mut superseded = false;

            let mut frame_index = 0;
            let mut audio_frames = 0;
            while frame_index < block_frames {
                if frame_index % SEEK_CONTROL_CHECK_FRAMES == 0
                    && self.seek_epoch.load(Ordering::Acquire) != block_seek_epoch
                {
                    superseded = true;
                    break;
                }

                // Prime the sink with silence before any audio is emitted. This
                // must not pull from the source: doing so discarded the frame it
                // decoded, which truncated the first PRE_ROLL_SILENCE_FRAMES
                // (~21 ms at 48 kHz) of every track. Emitting the run in one
                // resize keeps the reserved capacity intact and costs no source
                // frames, so the pre-roll now only delays audio, never drops it.
                // Clamped to the next control-check boundary so seek latency is
                // unchanged.
                if self.pre_roll_frames_remaining > 0 {
                    let frames = pre_roll_run_frames(self.pre_roll_frames_remaining, frame_index)
                        .min(block_frames - frame_index);
                    block.resize(block.len() + frames * self.output_channels as usize, 0.0);
                    self.pre_roll_frames_remaining -= frames;
                    frame_index += frames;
                    continue;
                }

                // The common file/device layout needs no channel mixing. Once
                // startup/seek shaping is complete, append whole runs of output
                // frames — copied straight through when the rates match,
                // produced by the resampler when they do not — instead of
                // pulling them one at a time through next_frame(). Stop each
                // chunk at the next control-check boundary so seek latency is
                // unchanged.
                if self.start_ramp_frames_remaining == 0 && self.frames.can_bulk_append() {
                    let until_control_check =
                        SEEK_CONTROL_CHECK_FRAMES - (frame_index % SEEK_CONTROL_CHECK_FRAMES);
                    let requested = until_control_check.min(block_frames - frame_index);
                    let block_start = block.len();
                    let copied = self.frames.append_frames(&mut block, requested);
                    if copied == 0 {
                        ended = true;
                        break;
                    }
                    if let Some(analysis_block) = analysis_block.as_mut() {
                        analysis_block.extend_from_slice(&block[block_start..]);
                    }
                    frame_index += copied;
                    audio_frames += copied;
                    if copied < requested {
                        ended = true;
                        break;
                    }
                    continue;
                }

                if self.frames.next_frame(&mut frame).is_none() {
                    ended = true;
                    break;
                }

                if self.start_ramp_frames_remaining > 0 {
                    let ramp_total = self.start_ramp_total_frames.max(1);
                    let elapsed = ramp_total.saturating_sub(self.start_ramp_frames_remaining);
                    let t = (elapsed as f32 / ramp_total as f32).clamp(0.0, 1.0);
                    let gain = t * t * (3.0 - 2.0 * t);
                    for sample in &mut frame {
                        *sample *= gain;
                    }
                    self.start_ramp_frames_remaining -= 1;
                }
                block.extend_from_slice(&frame);
                if let Some(block) = analysis_block.as_mut() {
                    block.extend_from_slice(&frame);
                }
                frame_index += 1;
                audio_frames += 1;
            }

            if superseded || self.seek_epoch.load(Ordering::Acquire) != block_seek_epoch {
                continue;
            }

            self.source_position += audio_frames as f64 / self.output_sample_rate as f64;
            let mut position = crate::output::PcmPosition {
                playback_id: self.playback_id,
                seconds: source_start,
                rate: if audio_frames == 0 { 0.0 } else { 1.0 },
            };
            if let Some((start, processor)) = self.tempo.as_mut() {
                if source_start + 0.5 / self.output_sample_rate as f64 >= *start
                    && (audio_frames > 0 || ended)
                {
                    let mut stretched = self
                        .output
                        .take_recycled_buffer(processor.output_capacity(ended));
                    stretched.clear();
                    let mut consumed = processor.process(&block, &mut stretched);
                    if ended {
                        consumed += processor.flush(&mut stretched);
                    }
                    position.seconds = self.emitted_position;
                    position.rate = if ended && !stretched.is_empty() {
                        consumed as f64 / stretched.len() as f64
                    } else {
                        processor.rate
                    };
                    self.emitted_position += consumed as f64
                        / (self.output_sample_rate as f64 * self.output_channels as f64);
                    self.raw_spare = Some(block);
                    block = stretched;
                    if let Some(analysis) = analysis_block.as_mut() {
                        analysis.clear();
                        analysis.extend_from_slice(&block);
                    }
                } else {
                    self.emitted_position = self.source_position;
                }
            } else {
                self.emitted_position = self.source_position;
            }

            if self.pending_seek_flush_epoch == Some(block_seek_epoch)
                && (!block.is_empty() || ended)
            {
                self.output.flush_for_seek();
                self.pending_seek_flush_epoch = None;
            }

            let pushed = block.is_empty()
                || self.output.push_timed_block(
                    block,
                    position,
                    PushCancel::with_interrupt_epoch(
                        self.stop_flag.as_ref(),
                        self.seek_epoch.as_ref(),
                        block_seek_epoch,
                    ),
                );
            let generation_current = self.output.generation() == generation;

            if pushed && generation_current && self.analysis_enabled.load(Ordering::Acquire) {
                if let Some(analysis_block) = analysis_block.take() {
                    if !analysis_block.is_empty() {
                        let pcm = AnalysisPcm {
                            samples: analysis_block,
                            channels: self.output_channels,
                            sample_rate: self.output_sample_rate,
                            // Everything queued ahead of the block just pushed
                            // plays first, then the device adds its own delay
                            // (the one lyrics already compensate for); analysis
                            // holds the block back that long.
                            audible_at: Instant::now()
                                + Duration::from_secs_f64(
                                    self.output.queued_samples() as f64
                                        / (self.output_sample_rate.max(1) as f64
                                            * self.output_channels.max(1) as f64),
                                )
                                + Duration::from_micros(
                                    // Past 2 s it is a broken timestamp, as in the
                                    // frontend's `MAX_PLAUSIBLE_OUTPUT_LATENCY_SECONDS`.
                                    (self.output.output_latency_us() as u64).min(2_000_000),
                                ),
                            recycle: Some(self.analysis_recycle_tx.clone()),
                        };
                        if let Err(err) = self.analysis_tx.try_send_pcm(pcm) {
                            // Analysis is best-effort. A full queue must never
                            // slow decode; recover this allocation immediately.
                            let mut samples = match err {
                                mpsc::TrySendError::Full(pcm)
                                | mpsc::TrySendError::Disconnected(pcm) => pcm.samples,
                            };
                            samples.clear();
                            let _ = self.analysis_recycle_tx.send(samples);
                        }
                    }
                }
            }

            if ended {
                if self.stop_flag.load(Ordering::Acquire) {
                    break;
                }
                if pushed && generation_current {
                    self.finish_after_drain(generation);
                    break;
                }
            }
        }
    }

    /// Hold while the download is too far behind to decode without waiting,
    /// reporting the edges so the player can freeze its clock and show a
    /// rebuffer instead of playing silence. Controls keep being served, so a
    /// pause, seek or stop does not wait for the network. `false` means stop.
    fn hold_for_stream(&mut self) -> bool {
        let Some(gate) = self.gate.as_mut() else {
            return true;
        };
        if !gate.should_hold() {
            if gate.starved {
                gate.starved = false;
                let _ = self.event_tx.send(DecoderEvent::Starved {
                    playback_id: self.playback_id,
                    starved: false,
                });
            }
            return true;
        }
        if !gate.starved {
            gate.starved = true;
            let _ = self.event_tx.send(DecoderEvent::Starved {
                playback_id: self.playback_id,
                starved: true,
            });
        }
        let progress = Arc::clone(&gate.progress);
        loop {
            if self.stop_flag.load(Ordering::Acquire) || !self.drain_controls() {
                return false;
            }
            if self.paused || self.seek_epoch.load(Ordering::Acquire) != self.applied_seek_epoch {
                // Let the main loop serve it; the gate re-evaluates on return.
                return true;
            }
            let Some(gate) = self.gate.as_mut() else {
                return true;
            };
            if !gate.should_hold() {
                return true;
            }
            progress.wait_for_change(GATE_POLL);
        }
    }

    fn wait_while_paused(&mut self) -> bool {
        loop {
            if self.stop_flag.load(Ordering::Acquire) {
                return false;
            }

            match self.control_rx.recv_timeout(PAUSED_POLL_INTERVAL) {
                Ok(control) => {
                    if !self.apply_control(control) {
                        return false;
                    }
                    if !self.drain_controls() {
                        return false;
                    }
                    if !self.paused {
                        return true;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return false,
            }
        }
    }

    fn finish_after_drain(&mut self, generation: u64) {
        loop {
            if self.stop_flag.load(Ordering::Acquire) || self.output.generation() != generation {
                return;
            }
            if !self.drain_controls() || self.output.generation() != generation {
                return;
            }
            if self.paused {
                let _ = self.wait_while_paused();
                continue;
            }
            if self.output.queued_samples() == 0 {
                let _ = self.event_tx.send(DecoderEvent::Finished {
                    playback_id: self.playback_id,
                });
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn drain_controls(&mut self) -> bool {
        while let Ok(control) = self.control_rx.try_recv() {
            if !self.apply_control(control) {
                return false;
            }
        }
        true
    }

    fn flush_tempo(&mut self) {
        if let Some((_, mut processor)) = self.tempo.take() {
            let mut block = self
                .output
                .take_recycled_buffer(processor.output_capacity(true));
            block.clear();
            let consumed = processor.flush(&mut block);
            if !block.is_empty() {
                let position = crate::output::PcmPosition {
                    playback_id: self.playback_id,
                    seconds: self.emitted_position,
                    rate: consumed as f64 / block.len() as f64,
                };
                let _ = self.output.push_timed_block(
                    block,
                    position,
                    PushCancel::with_interrupt_epoch(
                        self.stop_flag.as_ref(),
                        self.seek_epoch.as_ref(),
                        self.applied_seek_epoch,
                    ),
                );
            }
            self.emitted_position = self.source_position;
        }
    }

    fn apply_control(&mut self, control: DecoderControl) -> bool {
        match control {
            DecoderControl::ArmTempo {
                start,
                processor,
                ack,
            } => {
                let result = if start <= self.source_position + 1.0 / self.output_sample_rate as f64
                {
                    Err("AutoMix tempo start is already decoded".into())
                } else {
                    self.tempo = Some((start, processor));
                    Ok(())
                };
                let _ = ack.send(result);
                true
            }
            DecoderControl::CancelTempo => {
                self.cancel_tempo = true;
                true
            }
            DecoderControl::Seek { pos, epoch, ack } => {
                self.tempo = None;
                self.cancel_tempo = false;
                self.source_position = pos.as_secs_f64();
                self.emitted_position = self.source_position;
                let result = match self.frames.seek(pos) {
                    Ok(()) => {
                        self.applied_seek_epoch = epoch;
                        if cfg!(target_os = "android") {
                            self.output.flush_for_seek();
                            self.pending_seek_flush_epoch = None;
                        } else {
                            self.pending_seek_flush_epoch = Some(epoch);
                        }
                        self.pre_roll_frames_remaining = 0;
                        self.start_ramp_total_frames = SEEK_RAMP_FRAMES;
                        self.start_ramp_frames_remaining = SEEK_RAMP_FRAMES;
                        let _ = self.analysis_tx.send(AnalysisCommand::Clear);
                        Ok(())
                    }
                    Err(e) => {
                        self.applied_seek_epoch = epoch;
                        self.pending_seek_flush_epoch = None;
                        let err = format!("decoder seek failed: {e}");
                        warn!("{err}");
                        Err(err)
                    }
                };
                let _ = ack.send(result);
                true
            }
            DecoderControl::SetPaused(paused) => {
                self.paused = paused;
                self.output.set_paused(paused);
                let _ = self.analysis_tx.send(AnalysisCommand::SetPaused { paused });
                true
            }
            DecoderControl::Stop => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_roll_silence_never_stalls_and_lands_on_control_boundaries() {
        // Zero would spin the decode loop forever: the pre-roll branch
        // `continue`s without consuming a source frame.
        let mut frame_index = 0;
        let mut remaining = PRE_ROLL_SILENCE_FRAMES;
        let mut runs = 0;
        while remaining > 0 {
            if frame_index == DECODE_BLOCK_FRAMES {
                frame_index = 0; // next block
            }
            let frames = pre_roll_run_frames(remaining, frame_index);
            assert!(frames > 0, "pre-roll run of 0 frames would spin forever");
            assert!(
                frame_index + frames <= DECODE_BLOCK_FRAMES,
                "ran past the block"
            );
            frame_index += frames;
            assert_eq!(
                frame_index % SEEK_CONTROL_CHECK_FRAMES,
                0,
                "a run must end on a control-check boundary"
            );
            remaining -= frames;
            runs += 1;
            assert!(runs <= PRE_ROLL_SILENCE_FRAMES, "failed to converge");
        }

        // The whole pre-roll is emitted, and it is a whole number of blocks
        // here, so no partial block precedes the first audio.
        assert_eq!(remaining, 0);
        assert_eq!(
            runs,
            PRE_ROLL_SILENCE_FRAMES / SEEK_CONTROL_CHECK_FRAMES,
            "pre-roll should advance one control-check span per run"
        );
    }

    #[test]
    fn pre_roll_emits_silence_without_consuming_source_frames() {
        // The regression this guards: the pre-roll branch used to call
        // `next_frame()` and then throw the decoded frame away, silently
        // truncating the first PRE_ROLL_SILENCE_FRAMES of every track. The run
        // length is derived from the pre-roll counter alone — no source is
        // touched — so a reintroduced `next_frame()` call would have to be
        // added back explicitly rather than hidden in the arithmetic.
        const CHANNELS: usize = 2;
        let mut block: Vec<f32> = Vec::with_capacity(DECODE_BLOCK_FRAMES * CHANNELS);
        let frames = pre_roll_run_frames(PRE_ROLL_SILENCE_FRAMES, 0);

        block.resize(block.len() + frames * CHANNELS, 0.0);

        assert_eq!(block.len(), frames * CHANNELS);
        assert!(block.iter().all(|&s| s == 0.0), "pre-roll must be silence");
        // Filling in place must not outgrow the block the sink recycled for us.
        assert!(block.len() <= DECODE_BLOCK_FRAMES * CHANNELS);
    }

    #[test]
    fn bulk_passthrough_copies_only_complete_frames() {
        let samples = [0.1, -0.1, 0.2, -0.2, 9.0];
        let mut offset = 0;
        let mut output = Vec::new();

        let frames = append_available_interleaved_frames(&samples, &mut offset, &mut output, 2, 8);

        assert_eq!(frames, 2);
        assert_eq!(offset, 4);
        assert_eq!(output, vec![0.1, -0.1, 0.2, -0.2]);
    }

    #[test]
    fn bulk_passthrough_respects_chunk_limit_and_offset() {
        let samples = [0.1, -0.1, 0.2, -0.2, 0.3, -0.3];
        let mut offset = 2;
        let mut output = vec![7.0];

        let frames = append_available_interleaved_frames(&samples, &mut offset, &mut output, 2, 1);

        assert_eq!(frames, 1);
        assert_eq!(offset, 4);
        assert_eq!(output, vec![7.0, 0.2, -0.2]);
    }
}

#[cfg(test)]
mod gate_tests {
    use super::*;
    use crate::source::growing::Progress;

    #[test]
    fn the_gate_holds_below_low_and_releases_only_at_high() {
        // 100 KB/s declared: low = 100 KB, high = 300 KB.
        let progress = Progress::new(10_000_000);
        let mut gate = StreamGate::new(progress.clone(), Some(Duration::from_secs(100)));
        assert_eq!((gate.low, gate.high), (100_000, 300_000));

        let id = progress.begin_writer(0);
        assert!(gate.should_hold(), "nothing landed yet");
        progress.advance(id, 150_000);
        assert!(!gate.should_hold(), "above low and not yet starved");

        gate.starved = true;
        assert!(gate.should_hold(), "starved: 150 KB is below high");
        progress.advance(id, 200_000);
        assert!(!gate.should_hold(), "350 KB clears high");
    }

    #[test]
    fn a_finished_download_never_holds() {
        let progress = Progress::new(1000);
        let gate = StreamGate::new(progress.clone(), None);
        assert!(gate.should_hold());
        let id = progress.begin_writer(0);
        progress.advance(id, 1000);
        progress.end_writer(id, Ok(()));
        assert!(!gate.should_hold(), "the tail is released by completion");
    }
}
