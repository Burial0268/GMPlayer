use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use parking_lot::{Mutex, RwLock};

use crate::decoder::PlaybackSink;
use crate::effects::DspChain;
use crate::output::{OutputWriter, PushCancel};
use crate::types::{CrossfadeCurve, DspConfig};

#[cfg(any(target_os = "android", target_os = "linux"))]
// Match the deeper Android/Linux output queue so decode jitter does not
// immediately become zero-padded mixer blocks (Linux: Pulse/PipeWire server
// scheduling adds the same kind of jitter as the Android scheduler).
const DECK_QUEUE_BLOCKS: usize = 48;
#[cfg(not(any(target_os = "android", target_os = "linux")))]
const DECK_QUEUE_BLOCKS: usize = 8;
const DECK_RECYCLE_QUEUE_BLOCKS: usize = DECK_QUEUE_BLOCKS + 4;
const MIX_BLOCK_FRAMES: usize = 512;
const CROSSFADE_RAMP_FRAMES: usize = 64;
const PRODUCER_YIELD_RETRIES: u32 = 8;
const PRODUCER_MIN_PARK_US: u64 = 100;
// Steady-state playback keeps the rings full, so producers spend their lives
// in this park-poll loop: the cap IS the wakeup cadence. 4ms ≈ 2-3 polls per
// freed ~10ms block instead of ~10, cutting idle/full-ring wakeups ~4× under
// CPU stress. Interrupts (seek/stop/generation bump) are re-checked after
// every park, so the worst added latency for those paths is one cap.
const PRODUCER_MAX_PARK_US: u64 = 4_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeckId {
    Primary,
    Secondary,
}

enum MixerControl {
    FlushDeck {
        deck: DeckId,
        generation: u64,
        flush_epoch: u64,
        clear_output: bool,
        ack: mpsc::Sender<()>,
    },
    ClearDeck {
        deck: DeckId,
        generation: u64,
        flush_epoch: u64,
        clear_output: bool,
        ack: mpsc::Sender<()>,
    },
    ClearAll {
        primary_generation: u64,
        primary_flush_epoch: u64,
        secondary_generation: u64,
        secondary_flush_epoch: u64,
        ack: mpsc::Sender<()>,
    },
    #[allow(dead_code)]
    SetDeckGain {
        deck: DeckId,
        gain: f32,
    },
    StartCrossfade {
        outgoing: DeckId,
        incoming: DeckId,
        duration_samples: usize,
        params: CrossfadeParams,
    },
    ReplaceOutput {
        output: OutputWriter,
        ack: mpsc::Sender<()>,
    },
    SetDsp(DspConfig),
    Stop,
}

#[derive(Clone)]
pub struct DeckWriter {
    deck: DeckId,
    data_tx: mpsc::SyncSender<DeckBlock>,
    control_tx: mpsc::Sender<MixerControl>,
    control_epoch: Arc<AtomicU64>,
    output: Arc<RwLock<OutputWriter>>,
    paused: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    flush_epoch: Arc<AtomicU64>,
    queued_samples: Arc<AtomicUsize>,
    recycle_rx: Arc<Mutex<mpsc::Receiver<Vec<f32>>>>,
}

struct DeckBlock {
    samples: Vec<f32>,
    generation: u64,
    flush_epoch: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct CrossfadeParams {
    pub curve: CrossfadeCurve,
    pub incoming_gain: f32,
    pub outgoing_gain: f32,
    pub overlap_headroom_db: f32,
}

impl Default for CrossfadeParams {
    fn default() -> Self {
        Self {
            curve: CrossfadeCurve::EqualPower,
            incoming_gain: 1.0,
            outgoing_gain: 1.0,
            overlap_headroom_db: -0.8,
        }
    }
}

impl PlaybackSink for DeckWriter {
    fn push_block(&self, mut block: Vec<f32>, cancel: PushCancel<'_>) -> bool {
        if block.is_empty() {
            return true;
        }

        let generation = self.generation.load(Ordering::Acquire);
        let flush_epoch = self.flush_epoch.load(Ordering::Acquire);
        let mut retry_count = 0;
        loop {
            if cancel.is_cancelled()
                || self.generation.load(Ordering::Acquire) != generation
                || self.flush_epoch.load(Ordering::Acquire) != flush_epoch
            {
                return false;
            }

            let block_len = block.len();
            let deck_block = DeckBlock {
                samples: block,
                generation,
                flush_epoch,
            };
            self.queued_samples.fetch_add(block_len, Ordering::Release);
            match self.data_tx.try_send(deck_block) {
                Ok(()) => {
                    if cancel.is_cancelled()
                        || self.generation.load(Ordering::Acquire) != generation
                        || self.flush_epoch.load(Ordering::Acquire) != flush_epoch
                    {
                        saturating_sub(&self.queued_samples, block_len);
                        return false;
                    }
                    return true;
                }
                Err(mpsc::TrySendError::Full(returned)) => {
                    saturating_sub(&self.queued_samples, block_len);
                    block = returned.samples;
                    producer_retry_backoff(&mut retry_count);
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    saturating_sub(&self.queued_samples, block_len);
                    return false;
                }
            }
        }
    }

    fn flush_for_seek(&self) {
        self.flush_deck_for_seek();
    }

    fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
    }

    fn queued_samples(&self) -> usize {
        self.queued_samples.load(Ordering::Acquire) + self.output.read().queued_samples()
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    fn take_recycled_buffer(&self, capacity: usize) -> Vec<f32> {
        if let Some(rx) = self.recycle_rx.try_lock() {
            if let Ok(mut buf) = rx.try_recv() {
                buf.clear();
                if buf.capacity() < capacity {
                    buf.reserve(capacity - buf.capacity());
                }
                return buf;
            }
        }
        Vec::with_capacity(capacity)
    }
}

impl DeckWriter {
    fn flush_deck_for_seek(&self) {
        let generation = self.generation.load(Ordering::Acquire);
        let flush_epoch = self.flush_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.queued_samples.store(0, Ordering::Release);
        let (ack_tx, ack_rx) = mpsc::channel();
        self.control_epoch.fetch_add(1, Ordering::AcqRel);
        let _ = self.control_tx.send(MixerControl::FlushDeck {
            deck: self.deck,
            generation,
            flush_epoch,
            clear_output: true,
            ack: ack_tx,
        });
        let _ = ack_rx.recv_timeout(Duration::from_millis(200));
    }
}

pub struct DeckMixer {
    primary: DeckWriter,
    secondary: DeckWriter,
    control_tx: mpsc::Sender<MixerControl>,
    output: Arc<RwLock<OutputWriter>>,
    stop_flag: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl DeckMixer {
    pub fn new(
        output: OutputWriter,
        output_channels: u16,
        output_sample_rate: u32,
        dsp_config: &DspConfig,
    ) -> Self {
        let (primary_tx, primary_rx) = mpsc::sync_channel::<DeckBlock>(DECK_QUEUE_BLOCKS);
        let (secondary_tx, secondary_rx) = mpsc::sync_channel::<DeckBlock>(DECK_QUEUE_BLOCKS);
        let (primary_recycle_tx, primary_recycle_rx) =
            mpsc::sync_channel::<Vec<f32>>(DECK_RECYCLE_QUEUE_BLOCKS);
        let (secondary_recycle_tx, secondary_recycle_rx) =
            mpsc::sync_channel::<Vec<f32>>(DECK_RECYCLE_QUEUE_BLOCKS);
        let (control_tx, control_rx) = mpsc::channel::<MixerControl>();

        let stop_flag = Arc::new(AtomicBool::new(false));
        let primary_paused = Arc::new(AtomicBool::new(false));
        let secondary_paused = Arc::new(AtomicBool::new(true));
        let primary_generation = Arc::new(AtomicU64::new(0));
        let secondary_generation = Arc::new(AtomicU64::new(0));
        let primary_flush_epoch = Arc::new(AtomicU64::new(0));
        let secondary_flush_epoch = Arc::new(AtomicU64::new(0));
        let primary_queued = Arc::new(AtomicUsize::new(0));
        let secondary_queued = Arc::new(AtomicUsize::new(0));
        let control_epoch = Arc::new(AtomicU64::new(0));
        let output_state = Arc::new(RwLock::new(output.clone()));
        let primary = DeckWriter {
            deck: DeckId::Primary,
            data_tx: primary_tx,
            control_tx: control_tx.clone(),
            control_epoch: Arc::clone(&control_epoch),
            output: Arc::clone(&output_state),
            paused: Arc::clone(&primary_paused),
            generation: Arc::clone(&primary_generation),
            flush_epoch: Arc::clone(&primary_flush_epoch),
            queued_samples: Arc::clone(&primary_queued),
            recycle_rx: Arc::new(Mutex::new(primary_recycle_rx)),
        };

        let secondary = DeckWriter {
            deck: DeckId::Secondary,
            data_tx: secondary_tx,
            control_tx: control_tx.clone(),
            control_epoch: Arc::clone(&control_epoch),
            output: Arc::clone(&output_state),
            paused: Arc::clone(&secondary_paused),
            generation: Arc::clone(&secondary_generation),
            flush_epoch: Arc::clone(&secondary_flush_epoch),
            queued_samples: Arc::clone(&secondary_queued),
            recycle_rx: Arc::new(Mutex::new(secondary_recycle_rx)),
        };

        let mixer_stop = Arc::clone(&stop_flag);
        let dsp_config = dsp_config.clone();
        let thread = thread::Builder::new()
            .name("audio-deck-mixer".into())
            .spawn(move || {
                let mut dsp = DspChain::new(output_sample_rate, output_channels);
                dsp.set_dsp(&dsp_config);
                let mut worker = MixerWorker {
                    output,
                    output_channels: output_channels.max(1) as usize,
                    primary: DeckRuntime::new(
                        primary_rx,
                        primary_queued,
                        primary_paused,
                        primary_recycle_tx,
                        1.0,
                    ),
                    secondary: DeckRuntime::new(
                        secondary_rx,
                        secondary_queued,
                        secondary_paused,
                        secondary_recycle_tx,
                        0.0,
                    ),
                    crossfade: None,
                    dsp,
                    control_rx,
                    control_epoch,
                    stop_flag: mixer_stop,
                };
                worker.run();
            })
            .ok();

        Self {
            primary,
            secondary,
            control_tx,
            output: output_state,
            stop_flag,
            thread,
        }
    }

    pub fn primary_writer(&self) -> DeckWriter {
        self.primary.clone()
    }

    pub fn secondary_writer(&self) -> DeckWriter {
        self.secondary.clone()
    }

    pub fn set_deck_gain(&self, deck: DeckId, gain: f32) {
        let _ = self.control_tx.send(MixerControl::SetDeckGain {
            deck,
            gain: gain.clamp(0.0, 2.0),
        });
    }

    pub fn clear_deck(&self, deck: DeckId) {
        let writer = match deck {
            DeckId::Primary => &self.primary,
            DeckId::Secondary => &self.secondary,
        };
        let generation = writer.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let flush_epoch = writer.flush_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        writer.queued_samples.store(0, Ordering::Release);
        let (ack_tx, ack_rx) = mpsc::channel();
        self.primary.control_epoch.fetch_add(1, Ordering::AcqRel);
        let _ = self.control_tx.send(MixerControl::ClearDeck {
            deck,
            generation,
            flush_epoch,
            clear_output: false,
            ack: ack_tx,
        });
        let _ = ack_rx.recv_timeout(Duration::from_millis(200));
    }

    pub fn start_crossfade(
        &self,
        outgoing: DeckId,
        incoming: DeckId,
        duration_secs: f64,
        sample_rate: u32,
        channels: u16,
        params: CrossfadeParams,
    ) {
        let frames = (duration_secs.max(0.05) * sample_rate.max(1) as f64).ceil() as usize;
        let duration_samples = frames.saturating_mul(channels.max(1) as usize).max(1);
        let _ = self.control_tx.send(MixerControl::StartCrossfade {
            outgoing,
            incoming,
            duration_samples,
            params,
        });
    }

    pub fn clear_all(&self) {
        let primary_generation = self.primary.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let primary_flush_epoch = self.primary.flush_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        let secondary_generation = self.secondary.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let secondary_flush_epoch = self.secondary.flush_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.primary.queued_samples.store(0, Ordering::Release);
        self.secondary.queued_samples.store(0, Ordering::Release);
        let (ack_tx, ack_rx) = mpsc::channel();
        self.primary.control_epoch.fetch_add(1, Ordering::AcqRel);
        let _ = self.control_tx.send(MixerControl::ClearAll {
            primary_generation,
            primary_flush_epoch,
            secondary_generation,
            secondary_flush_epoch,
            ack: ack_tx,
        });
        let _ = ack_rx.recv_timeout(Duration::from_millis(200));
    }

    pub fn replace_output(&self, output: OutputWriter) -> bool {
        let (ack_tx, ack_rx) = mpsc::channel();
        self.primary.control_epoch.fetch_add(1, Ordering::AcqRel);
        if self
            .control_tx
            .send(MixerControl::ReplaceOutput {
                output: output.clone(),
                ack: ack_tx,
            })
            .is_err()
        {
            return false;
        }

        if ack_rx.recv_timeout(Duration::from_millis(200)).is_err() {
            return false;
        }

        *self.output.write() = output;
        true
    }

    pub fn set_dsp(&self, config: DspConfig) {
        let _ = self.control_tx.send(MixerControl::SetDsp(config));
    }

    pub fn queued_samples(&self) -> usize {
        self.primary.queued_samples.load(Ordering::Acquire)
            + self.secondary.queued_samples.load(Ordering::Acquire)
            + self.output.read().queued_samples()
    }
}

impl Drop for DeckMixer {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
        let _ = self.control_tx.send(MixerControl::Stop);
        if let Some(thread) = self.thread.take() {
            join_mixer_thread_async(thread);
        }
    }
}

fn join_mixer_thread_async(handle: thread::JoinHandle<()>) {
    let _ = thread::Builder::new()
        .name("audio-deck-mixer-join".into())
        .spawn(move || {
            let _ = handle.join();
        });
}

struct DeckRuntime {
    rx: mpsc::Receiver<DeckBlock>,
    current_block: Vec<f32>,
    current_index: usize,
    accepted_generation: u64,
    accepted_flush_epoch: u64,
    queued_samples: Arc<AtomicUsize>,
    paused: Arc<AtomicBool>,
    recycle_tx: mpsc::SyncSender<Vec<f32>>,
    gain: f32,
}

impl DeckRuntime {
    fn new(
        rx: mpsc::Receiver<DeckBlock>,
        queued_samples: Arc<AtomicUsize>,
        paused: Arc<AtomicBool>,
        recycle_tx: mpsc::SyncSender<Vec<f32>>,
        gain: f32,
    ) -> Self {
        Self {
            rx,
            current_block: Vec::new(),
            current_index: 0,
            accepted_generation: 0,
            accepted_flush_epoch: 0,
            queued_samples,
            paused,
            recycle_tx,
            gain,
        }
    }

    fn clear(&mut self, generation: u64, flush_epoch: u64) {
        let spent = std::mem::take(&mut self.current_block);
        recycle_deck_buffer(&self.recycle_tx, spent);
        self.current_index = 0;
        self.accepted_generation = generation;
        self.accepted_flush_epoch = flush_epoch;
        self.queued_samples.store(0, Ordering::Release);
    }

    fn next_sample(&mut self, consumed: &mut usize) -> Option<f32> {
        if self.current_index >= self.current_block.len() {
            loop {
                match self.rx.try_recv() {
                    Ok(block) => {
                        if block.generation != self.accepted_generation
                            || block.flush_epoch != self.accepted_flush_epoch
                        {
                            recycle_deck_buffer(&self.recycle_tx, block.samples);
                            continue;
                        }
                        let spent = std::mem::replace(&mut self.current_block, block.samples);
                        recycle_deck_buffer(&self.recycle_tx, spent);
                        self.current_index = 0;
                        if self.current_block.is_empty() {
                            continue;
                        }
                        break;
                    }
                    Err(_) => return None,
                }
            }
        }

        let sample = self.current_block[self.current_index];
        self.current_index += 1;
        *consumed += 1;
        Some(sample)
    }

    /// Decrement the shared queued-sample counter once per mix block instead of
    /// once per sample, turning ~one CAS-loop-per-sample into one per block.
    #[inline]
    fn commit_consumed(&self, consumed: usize) {
        if consumed > 0 {
            saturating_sub(&self.queued_samples, consumed);
        }
    }

    /// Bulk-fill up to `samples` samples from this deck into `dst`, applying
    /// `gain` and clamping over contiguous runs (which autovectorizes). Pulls
    /// fresh blocks from the queue as needed and stops early on underrun.
    /// Returns the number of samples written and adds it to `consumed`; the
    /// queued-sample counter is committed once per block by the caller.
    fn fill_scaled(
        &mut self,
        dst: &mut Vec<f32>,
        samples: usize,
        gain: f32,
        consumed: &mut usize,
    ) -> usize {
        let mut written = 0;
        while written < samples {
            if self.current_index >= self.current_block.len() {
                match self.rx.try_recv() {
                    Ok(block) => {
                        if block.generation != self.accepted_generation
                            || block.flush_epoch != self.accepted_flush_epoch
                        {
                            recycle_deck_buffer(&self.recycle_tx, block.samples);
                            continue;
                        }
                        let spent = std::mem::replace(&mut self.current_block, block.samples);
                        recycle_deck_buffer(&self.recycle_tx, spent);
                        self.current_index = 0;
                        if self.current_block.is_empty() {
                            continue;
                        }
                    }
                    Err(_) => break,
                }
            }

            let avail = self.current_block.len() - self.current_index;
            let run = (samples - written).min(avail);
            let src = &self.current_block[self.current_index..self.current_index + run];
            if gain == 1.0 {
                for &s in src {
                    dst.push(s.clamp(-1.0, 1.0));
                }
            } else {
                for &s in src {
                    dst.push((s * gain).clamp(-1.0, 1.0));
                }
            }
            self.current_index += run;
            written += run;
            *consumed += run;
        }
        written
    }
}

struct CrossfadeRuntime {
    outgoing: DeckId,
    incoming: DeckId,
    duration_samples: usize,
    elapsed_samples: usize,
    params: CrossfadeParams,
}

#[derive(Clone, Copy)]
struct CrossfadeBlockRamp {
    outgoing: DeckId,
    active_frames: usize,
    outgoing_start: f32,
    incoming_start: f32,
    outgoing_step: f32,
    incoming_step: f32,
    outgoing_final: f32,
    incoming_final: f32,
}

impl CrossfadeBlockRamp {
    #[inline]
    fn gains(&self, frame: usize) -> (f32, f32) {
        if frame >= self.active_frames {
            return (self.outgoing_final, self.incoming_final);
        }

        let frame = frame as f32;
        (
            self.outgoing_start + self.outgoing_step * frame,
            self.incoming_start + self.incoming_step * frame,
        )
    }
}

struct MixerWorker {
    output: OutputWriter,
    output_channels: usize,
    primary: DeckRuntime,
    secondary: DeckRuntime,
    crossfade: Option<CrossfadeRuntime>,
    dsp: DspChain,
    control_rx: mpsc::Receiver<MixerControl>,
    control_epoch: Arc<AtomicU64>,
    stop_flag: Arc<AtomicBool>,
}

impl MixerWorker {
    fn run(&mut self) {
        let channels = self.output_channels.max(1);
        let frame_capacity = MIX_BLOCK_FRAMES * channels;
        let mut idle_retry_count = 0;

        while !self.stop_flag.load(Ordering::Acquire) {
            if !self.drain_controls() {
                break;
            }

            if self.output.has_failed() {
                producer_retry_backoff(&mut idle_retry_count);
                continue;
            }

            // Paused state changes far slower than the sample rate — sample it
            // once per block instead of doing an atomic load per sample.
            let primary_paused = self.primary.paused.load(Ordering::Acquire);
            let secondary_paused = self.secondary.paused.load(Ordering::Acquire);

            // Reserve exact capacity and fill by pushing — no zero-fill pass,
            // since every slot is written below. The buffer is recycled from
            // the output callback when available, so steady playback does not
            // allocate here (and the callback does not free).
            let mut block = self.output.take_recycled_buffer(frame_capacity);
            let has_audio = self.mix_block(&mut block, channels, primary_paused, secondary_paused);

            if has_audio {
                idle_retry_count = 0;
                let control_epoch = self.control_epoch.load(Ordering::Acquire);
                if !self.dsp.is_bypassed() {
                    self.dsp.process_interleaved(&mut block);
                }
                if !self.output.push_block(
                    block,
                    PushCancel::with_interrupt_epoch(
                        self.stop_flag.as_ref(),
                        self.control_epoch.as_ref(),
                        control_epoch,
                    ),
                ) {
                    if self.stop_flag.load(Ordering::Acquire) {
                        break;
                    }
                    continue;
                }
            } else {
                producer_retry_backoff(&mut idle_retry_count);
            }
        }
    }

    /// Mix one output block worth of samples into `block`, returning whether
    /// any audio was written.
    ///
    /// Fast path: with no crossfade in progress the deck gains are constant for
    /// the whole block, so when exactly one deck is contributing we bulk-copy
    /// it (gain + clamp over contiguous runs, which autovectorizes) instead of
    /// running the per-sample two-deck mixer. The per-sample path still runs
    /// during crossfades (gains move per frame) and in the rare case where both
    /// decks are simultaneously active without a crossfade.
    fn mix_block(
        &mut self,
        block: &mut Vec<f32>,
        channels: usize,
        primary_paused: bool,
        secondary_paused: bool,
    ) -> bool {
        let want = MIX_BLOCK_FRAMES * channels;

        if self.crossfade.is_none() {
            let primary_active = !primary_paused && self.primary.gain != 0.0;
            let secondary_active = !secondary_paused && self.secondary.gain != 0.0;

            if primary_active != secondary_active {
                let written = if primary_active {
                    let gain = self.primary.gain;
                    let mut consumed = 0;
                    let written = self.primary.fill_scaled(block, want, gain, &mut consumed);
                    self.primary.commit_consumed(consumed);
                    written
                } else {
                    let gain = self.secondary.gain;
                    let mut consumed = 0;
                    let written = self.secondary.fill_scaled(block, want, gain, &mut consumed);
                    self.secondary.commit_consumed(consumed);
                    written
                };
                // Pad the tail with silence on underrun so the block is a whole
                // number of frames.
                block.resize(block.len() + (want - written), 0.0);
                return written > 0;
            }

            if !primary_active && !secondary_active {
                // Nothing to render — leave `block` empty; the caller sleeps.
                return false;
            }
            // Both active without a crossfade — fall through to the mixer.
        }

        // General path: advance crossfade gains once per frame and mix both
        // decks sample by sample.
        let mut has_audio = false;
        let mut consumed_primary = 0usize;
        let mut consumed_secondary = 0usize;

        // Crossfade shaping contains transcendental math (sin/cos/sqrt/powf).
        // Evaluate it at short segment endpoints, then use a smooth linear gain
        // ramp per frame. At 48 kHz this cuts complex gain calculations by ~32x
        // without allocations, while a 64-frame segment keeps even the minimum
        // 50 ms fade closely aligned with its requested curve.
        let mut mixed_frames = 0;
        while mixed_frames < MIX_BLOCK_FRAMES {
            let segment_frames = (MIX_BLOCK_FRAMES - mixed_frames).min(CROSSFADE_RAMP_FRAMES);
            let crossfade_ramp = self.crossfade_block_ramp(segment_frames);

            for frame_index in 0..segment_frames {
                let (primary_gain, secondary_gain) = match crossfade_ramp {
                    Some(ramp) => {
                        let (outgoing_gain, incoming_gain) = ramp.gains(frame_index);
                        match ramp.outgoing {
                            DeckId::Primary => (outgoing_gain, incoming_gain),
                            DeckId::Secondary => (incoming_gain, outgoing_gain),
                        }
                    }
                    None => (self.primary.gain, self.secondary.gain),
                };

                for _ in 0..channels {
                    let primary = if primary_paused {
                        None
                    } else {
                        self.primary.next_sample(&mut consumed_primary)
                    };
                    let secondary = if secondary_paused {
                        None
                    } else {
                        self.secondary.next_sample(&mut consumed_secondary)
                    };
                    if primary.is_some() || secondary.is_some() {
                        has_audio = true;
                    }
                    let mixed = primary.unwrap_or(0.0) * primary_gain
                        + secondary.unwrap_or(0.0) * secondary_gain;
                    block.push(mixed.clamp(-1.0, 1.0));
                }
            }

            if crossfade_ramp.is_some() {
                self.commit_crossfade_block(segment_frames);
            }
            mixed_frames += segment_frames;
        }

        self.primary.commit_consumed(consumed_primary);
        self.secondary.commit_consumed(consumed_secondary);
        has_audio
    }

    fn drain_controls(&mut self) -> bool {
        while let Ok(control) = self.control_rx.try_recv() {
            match control {
                MixerControl::FlushDeck {
                    deck,
                    generation,
                    flush_epoch,
                    clear_output,
                    ack,
                } => {
                    self.deck_mut(deck).clear(generation, flush_epoch);
                    if self
                        .crossfade
                        .as_ref()
                        .is_some_and(|fade| fade.outgoing == deck || fade.incoming == deck)
                    {
                        self.crossfade = None;
                    }
                    if clear_output {
                        self.output.flush();
                    }
                    let _ = ack.send(());
                }
                MixerControl::ClearDeck {
                    deck,
                    generation,
                    flush_epoch,
                    clear_output,
                    ack,
                } => {
                    self.deck_mut(deck).clear(generation, flush_epoch);
                    if self
                        .crossfade
                        .as_ref()
                        .is_some_and(|fade| fade.outgoing == deck || fade.incoming == deck)
                    {
                        self.crossfade = None;
                    }
                    if clear_output {
                        self.output.clear();
                    }
                    let _ = ack.send(());
                }
                MixerControl::ClearAll {
                    primary_generation,
                    primary_flush_epoch,
                    secondary_generation,
                    secondary_flush_epoch,
                    ack,
                } => {
                    self.primary.clear(primary_generation, primary_flush_epoch);
                    self.secondary
                        .clear(secondary_generation, secondary_flush_epoch);
                    self.crossfade = None;
                    self.output.clear();
                    let _ = ack.send(());
                }
                MixerControl::SetDeckGain { deck, gain } => {
                    self.deck_mut(deck).gain = gain.clamp(0.0, 2.0);
                }
                MixerControl::StartCrossfade {
                    outgoing,
                    incoming,
                    duration_samples,
                    params,
                } => {
                    self.set_deck_gain_direct(outgoing, params.outgoing_gain);
                    self.set_deck_gain_direct(incoming, 0.0);
                    self.crossfade = Some(CrossfadeRuntime {
                        outgoing,
                        incoming,
                        duration_samples: duration_samples.max(1),
                        elapsed_samples: 0,
                        params,
                    });
                }
                MixerControl::ReplaceOutput { output, ack } => {
                    self.output.retire();
                    self.output = output;
                    let _ = ack.send(());
                }
                MixerControl::SetDsp(config) => {
                    self.dsp.set_dsp(&config);
                }
                MixerControl::Stop => return false,
            }
        }
        true
    }

    fn crossfade_block_ramp(&self, frames: usize) -> Option<CrossfadeBlockRamp> {
        let Some(fade) = self.crossfade.as_ref() else {
            return None;
        };

        let step = self.output_channels.max(1);
        let remaining_samples = fade.duration_samples.saturating_sub(fade.elapsed_samples);
        let active_frames = remaining_samples.div_ceil(step).min(frames.max(1));
        let start_progress =
            (fade.elapsed_samples as f32 / fade.duration_samples as f32).clamp(0.0, 1.0);
        let (outgoing_start, incoming_start) =
            balanced_crossfade_gains(start_progress, fade.params);
        let completes = remaining_samples <= active_frames.saturating_mul(step);
        let (outgoing_end, incoming_end) = if completes {
            (0.0, fade.params.incoming_gain)
        } else {
            let end_elapsed = fade
                .elapsed_samples
                .saturating_add((active_frames.saturating_sub(1)).saturating_mul(step));
            let end_progress = (end_elapsed as f32 / fade.duration_samples as f32).clamp(0.0, 1.0);
            balanced_crossfade_gains(end_progress, fade.params)
        };
        let ramp_steps = active_frames.saturating_sub(1) as f32;
        let (outgoing_step, incoming_step) = if ramp_steps > 0.0 {
            (
                (outgoing_end - outgoing_start) / ramp_steps,
                (incoming_end - incoming_start) / ramp_steps,
            )
        } else {
            (0.0, 0.0)
        };

        Some(CrossfadeBlockRamp {
            outgoing: fade.outgoing,
            active_frames,
            outgoing_start: if active_frames == 1 && completes {
                outgoing_end
            } else {
                outgoing_start
            },
            incoming_start: if active_frames == 1 && completes {
                incoming_end
            } else {
                incoming_start
            },
            outgoing_step,
            incoming_step,
            outgoing_final: 0.0,
            incoming_final: fade.params.incoming_gain,
        })
    }

    fn commit_crossfade_block(&mut self, frames: usize) {
        let Some(fade) = self.crossfade.as_mut() else {
            return;
        };

        let elapsed_samples = fade
            .elapsed_samples
            .saturating_add(frames.saturating_mul(self.output_channels.max(1)));
        if elapsed_samples >= fade.duration_samples {
            let outgoing = fade.outgoing;
            let incoming = fade.incoming;
            let final_in_gain = fade.params.incoming_gain;
            self.set_deck_gain_direct(outgoing, 0.0);
            self.set_deck_gain_direct(incoming, final_in_gain);
            self.crossfade = None;
        } else {
            fade.elapsed_samples = elapsed_samples;
        }
    }

    fn set_deck_gain_direct(&mut self, deck: DeckId, gain: f32) {
        self.deck_mut(deck).gain = gain.clamp(0.0, 2.0);
    }

    fn deck_mut(&mut self, deck: DeckId) -> &mut DeckRuntime {
        match deck {
            DeckId::Primary => &mut self.primary,
            DeckId::Secondary => &mut self.secondary,
        }
    }
}

fn saturating_sub(counter: &AtomicUsize, amount: usize) {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let next = current.saturating_sub(amount);
        match counter.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(observed) => current = observed,
        }
    }
}

fn recycle_deck_buffer(recycle_tx: &mpsc::SyncSender<Vec<f32>>, mut block: Vec<f32>) {
    if block.capacity() == 0 {
        return;
    }
    block.clear();
    let _ = recycle_tx.try_send(block);
}

fn producer_retry_backoff(retry_count: &mut u32) {
    if *retry_count < PRODUCER_YIELD_RETRIES {
        *retry_count += 1;
        thread::yield_now();
        return;
    }

    let shift = (*retry_count - PRODUCER_YIELD_RETRIES).min(6);
    let park_us = (PRODUCER_MIN_PARK_US << shift).min(PRODUCER_MAX_PARK_US);
    *retry_count = (*retry_count).saturating_add(1);
    thread::park_timeout(Duration::from_micros(park_us));
}

fn balanced_crossfade_gains(progress: f32, params: CrossfadeParams) -> (f32, f32) {
    let t = progress.clamp(0.0, 1.0);
    let (out_vol, in_vol) = crossfade_values(t, params.curve);
    let outgoing_target = params.outgoing_gain.clamp(0.0, 2.0);
    let incoming_target = params.incoming_gain.clamp(0.0, 2.0);

    let mut out_gain = out_vol * outgoing_target;
    let mut in_gain = in_vol * incoming_target;

    let current_power = out_gain * out_gain + in_gain * in_gain;
    if current_power > 1e-8 {
        let target_power =
            outgoing_target * outgoing_target * (1.0 - t) + incoming_target * incoming_target * t;
        let power_scale = (target_power.max(0.0) / current_power).sqrt();
        let overlap_shape = (t * std::f32::consts::PI).sin();
        let headroom_scale = 10f32.powf((params.overlap_headroom_db * overlap_shape) / 20.0);
        let scale = power_scale.min(1.1) * headroom_scale;
        out_gain *= scale;
        in_gain *= scale;
    }

    (out_gain, in_gain)
}

fn crossfade_values(progress: f32, curve: CrossfadeCurve) -> (f32, f32) {
    let t = progress.clamp(0.0, 1.0);
    match curve {
        CrossfadeCurve::Linear => (1.0 - t, t),
        CrossfadeCurve::EqualPower => {
            let angle = t * std::f32::consts::FRAC_PI_2;
            (angle.cos(), angle.sin())
        }
        CrossfadeCurve::SCurve => {
            let s = t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
            let angle = s * std::f32::consts::FRAC_PI_2;
            (angle.cos(), angle.sin())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck_from_blocks(blocks: Vec<Vec<f32>>) -> (DeckRuntime, mpsc::SyncSender<DeckBlock>) {
        let (tx, rx) = mpsc::sync_channel(16);
        let (recycle_tx, _recycle_rx) = mpsc::sync_channel(16);
        for samples in blocks {
            tx.send(DeckBlock {
                samples,
                generation: 0,
                flush_epoch: 0,
            })
            .unwrap();
        }
        let deck = DeckRuntime::new(
            rx,
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicBool::new(false)),
            recycle_tx,
            1.0,
        );
        (deck, tx)
    }

    #[test]
    fn fill_scaled_bulk_copies_with_gain_and_clamp() {
        let (mut deck, _tx) = deck_from_blocks(vec![vec![0.5, -0.5, 2.0, -2.0]]);
        let mut dst = Vec::new();
        let mut consumed = 0;
        let written = deck.fill_scaled(&mut dst, 4, 0.5, &mut consumed);
        assert_eq!(written, 4);
        assert_eq!(consumed, 4);
        // 2.0 * 0.5 = 1.0 and -2.0 * 0.5 = -1.0 stay inside the clamp range.
        assert_eq!(dst, vec![0.25, -0.25, 1.0, -1.0]);
    }

    #[test]
    fn fill_scaled_stops_on_underrun() {
        let (mut deck, tx) = deck_from_blocks(vec![vec![0.1, 0.2]]);
        drop(tx); // no further blocks will arrive
        let mut dst = Vec::new();
        let mut consumed = 0;
        let written = deck.fill_scaled(&mut dst, 8, 1.0, &mut consumed);
        assert_eq!(written, 2);
        assert_eq!(consumed, 2);
        assert_eq!(dst, vec![0.1, 0.2]);
    }

    #[test]
    fn fill_scaled_skips_stale_generation_blocks() {
        let (tx, rx) = mpsc::sync_channel(16);
        let (recycle_tx, _recycle_rx) = mpsc::sync_channel(16);
        tx.send(DeckBlock {
            samples: vec![9.0, 9.0],
            generation: 0,
            flush_epoch: 0,
        })
        .unwrap();
        tx.send(DeckBlock {
            samples: vec![0.3, 0.4],
            generation: 1,
            flush_epoch: 0,
        })
        .unwrap();
        let mut deck = DeckRuntime::new(
            rx,
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicBool::new(false)),
            recycle_tx,
            1.0,
        );
        deck.accepted_generation = 1; // only accept generation 1

        let mut dst = Vec::new();
        let mut consumed = 0;
        let written = deck.fill_scaled(&mut dst, 2, 1.0, &mut consumed);
        assert_eq!(written, 2);
        assert_eq!(dst, vec![0.3, 0.4]); // the stale generation-0 block is dropped
    }

    #[test]
    fn short_crossfade_ramps_track_exact_curves() {
        let duration_samples = 2_400usize * 2;
        let params = [
            CrossfadeCurve::Linear,
            CrossfadeCurve::EqualPower,
            CrossfadeCurve::SCurve,
        ]
        .map(|curve| CrossfadeParams {
            curve,
            incoming_gain: 1.15,
            outgoing_gain: 0.85,
            overlap_headroom_db: -0.8,
        });

        for params in params {
            let start = duration_samples / 3;
            let end = start + (CROSSFADE_RAMP_FRAMES - 1) * 2;
            let start_progress = start as f32 / duration_samples as f32;
            let end_progress = end as f32 / duration_samples as f32;
            let (out_start, in_start) = balanced_crossfade_gains(start_progress, params);
            let (out_end, in_end) = balanced_crossfade_gains(end_progress, params);
            let denominator = (CROSSFADE_RAMP_FRAMES - 1) as f32;
            let ramp = CrossfadeBlockRamp {
                outgoing: DeckId::Primary,
                active_frames: CROSSFADE_RAMP_FRAMES,
                outgoing_start: out_start,
                incoming_start: in_start,
                outgoing_step: (out_end - out_start) / denominator,
                incoming_step: (in_end - in_start) / denominator,
                outgoing_final: 0.0,
                incoming_final: params.incoming_gain,
            };

            for frame in 0..CROSSFADE_RAMP_FRAMES {
                let progress = (start + frame * 2) as f32 / duration_samples as f32;
                let exact = balanced_crossfade_gains(progress, params);
                let interpolated = ramp.gains(frame);
                assert!((exact.0 - interpolated.0).abs() < 0.001);
                assert!((exact.1 - interpolated.1).abs() < 0.001);
            }
        }
    }
}
