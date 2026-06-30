use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use parking_lot::RwLock;

use crate::decoder::PlaybackSink;
use crate::effects::DspChain;
use crate::output::{OutputWriter, PushCancel};
use crate::types::{CrossfadeCurve, DspConfig};

const DECK_QUEUE_BLOCKS: usize = 16;
const MIX_BLOCK_FRAMES: usize = 1024;

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
        loop {
            if cancel.is_cancelled()
                || self.generation.load(Ordering::Acquire) != generation
                || self.flush_epoch.load(Ordering::Acquire) != flush_epoch
            {
                return false;
            }

            if self.paused.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(2));
                continue;
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
                    thread::sleep(Duration::from_millis(2));
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
                    primary: DeckRuntime::new(primary_rx, primary_queued, primary_paused, 1.0),
                    secondary: DeckRuntime::new(
                        secondary_rx,
                        secondary_queued,
                        secondary_paused,
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
    gain: f32,
}

impl DeckRuntime {
    fn new(
        rx: mpsc::Receiver<DeckBlock>,
        queued_samples: Arc<AtomicUsize>,
        paused: Arc<AtomicBool>,
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
            gain,
        }
    }

    fn clear(&mut self, generation: u64, flush_epoch: u64) {
        self.current_block.clear();
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
                            continue;
                        }
                        self.current_block = block.samples;
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
}

struct CrossfadeRuntime {
    outgoing: DeckId,
    incoming: DeckId,
    duration_samples: usize,
    elapsed_samples: usize,
    params: CrossfadeParams,
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

        while !self.stop_flag.load(Ordering::Acquire) {
            if !self.drain_controls() {
                break;
            }

            if self.output.has_failed() {
                thread::sleep(Duration::from_millis(2));
                continue;
            }

            // Paused state changes far slower than the sample rate — sample it
            // once per block instead of doing an atomic load per sample.
            let primary_paused = self.primary.paused.load(Ordering::Acquire);
            let secondary_paused = self.secondary.paused.load(Ordering::Acquire);

            // Reserve exact capacity and fill by pushing — no zero-fill pass,
            // since every slot is written below.
            let mut block = Vec::with_capacity(frame_capacity);
            let mut has_audio = false;
            let mut consumed_primary = 0usize;
            let mut consumed_secondary = 0usize;

            for _ in 0..MIX_BLOCK_FRAMES {
                // Crossfade gains move over seconds, so advance them once per
                // frame (all channels in a frame share one gain) rather than
                // once per sample. This keeps the sin/sqrt/pow curve math at
                // frame rate during a fade.
                self.advance_crossfade();
                let primary_gain = self.primary.gain;
                let secondary_gain = self.secondary.gain;

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

            self.primary.commit_consumed(consumed_primary);
            self.secondary.commit_consumed(consumed_secondary);

            if has_audio {
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
                thread::sleep(Duration::from_millis(2));
            }
        }
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

    fn advance_crossfade(&mut self) {
        let Some(fade) = self.crossfade.as_ref() else {
            return;
        };
        // `advance_crossfade` is called once per frame, while the fade duration
        // is tracked in samples — step by one frame (= `output_channels`).
        let step = self.output_channels.max(1);
        let progress = (fade.elapsed_samples as f32 / fade.duration_samples as f32).clamp(0.0, 1.0);
        let (out_gain, in_gain) = balanced_crossfade_gains(progress, fade.params);
        let outgoing = fade.outgoing;
        let incoming = fade.incoming;
        let elapsed_samples = fade.elapsed_samples.saturating_add(step);
        let duration_samples = fade.duration_samples;
        let final_in_gain = fade.params.incoming_gain;

        self.set_deck_gain_direct(outgoing, out_gain);
        self.set_deck_gain_direct(incoming, in_gain);
        if elapsed_samples >= duration_samples {
            self.set_deck_gain_direct(outgoing, 0.0);
            self.set_deck_gain_direct(incoming, final_in_gain);
            self.crossfade = None;
        } else if let Some(fade) = self.crossfade.as_mut() {
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
    let mut current = counter.load(Ordering::Acquire);
    loop {
        let next = current.saturating_sub(amount);
        match counter.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return,
            Err(observed) => current = observed,
        }
    }
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
