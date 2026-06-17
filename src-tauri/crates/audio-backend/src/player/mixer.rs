use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use crate::decoder::PlaybackSink;
use crate::output::OutputWriter;
use crate::types::CrossfadeCurve;

const DECK_QUEUE_BLOCKS: usize = 16;
const MIX_BLOCK_FRAMES: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeckId {
    Primary,
    Secondary,
}

enum MixerControl {
    ClearDeck {
        deck: DeckId,
        ack: mpsc::Sender<()>,
    },
    ClearAll {
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
    Stop,
}

#[derive(Clone)]
pub struct DeckWriter {
    deck: DeckId,
    data_tx: mpsc::SyncSender<Vec<f32>>,
    control_tx: mpsc::Sender<MixerControl>,
    output: OutputWriter,
    paused: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    queued_samples: Arc<AtomicUsize>,
    clear_output_on_clear: bool,
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
    fn push_block(&self, mut block: Vec<f32>, cancel: &AtomicBool) -> bool {
        if block.is_empty() {
            return true;
        }

        let generation = self.generation.load(Ordering::Acquire);
        loop {
            if cancel.load(Ordering::Acquire)
                || self.generation.load(Ordering::Acquire) != generation
            {
                return false;
            }

            if self.paused.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(2));
                continue;
            }

            let block_len = block.len();
            self.queued_samples.fetch_add(block_len, Ordering::Release);
            match self.data_tx.try_send(block) {
                Ok(()) => return true,
                Err(mpsc::TrySendError::Full(returned)) => {
                    saturating_sub(&self.queued_samples, block_len);
                    block = returned;
                    thread::sleep(Duration::from_millis(2));
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    saturating_sub(&self.queued_samples, block_len);
                    return false;
                }
            }
        }
    }

    fn clear(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.queued_samples.store(0, Ordering::Release);
        let (ack_tx, ack_rx) = mpsc::channel();
        let _ = self.control_tx.send(MixerControl::ClearDeck {
            deck: self.deck,
            ack: ack_tx,
        });
        let _ = ack_rx.recv_timeout(Duration::from_millis(200));
        if self.clear_output_on_clear {
            self.output.clear();
        }
    }

    fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
    }

    fn queued_samples(&self) -> usize {
        self.queued_samples.load(Ordering::Acquire) + self.output.queued_samples()
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

pub struct DeckMixer {
    primary: DeckWriter,
    secondary: DeckWriter,
    control_tx: mpsc::Sender<MixerControl>,
    stop_flag: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl DeckMixer {
    pub fn new(output: OutputWriter, output_channels: u16) -> Self {
        let (primary_tx, primary_rx) = mpsc::sync_channel::<Vec<f32>>(DECK_QUEUE_BLOCKS);
        let (secondary_tx, secondary_rx) = mpsc::sync_channel::<Vec<f32>>(DECK_QUEUE_BLOCKS);
        let (control_tx, control_rx) = mpsc::channel::<MixerControl>();

        let stop_flag = Arc::new(AtomicBool::new(false));
        let primary_paused = Arc::new(AtomicBool::new(false));
        let secondary_paused = Arc::new(AtomicBool::new(true));
        let primary_generation = Arc::new(AtomicU64::new(0));
        let secondary_generation = Arc::new(AtomicU64::new(0));
        let primary_queued = Arc::new(AtomicUsize::new(0));
        let secondary_queued = Arc::new(AtomicUsize::new(0));
        let primary = DeckWriter {
            deck: DeckId::Primary,
            data_tx: primary_tx,
            control_tx: control_tx.clone(),
            output: output.clone(),
            paused: Arc::clone(&primary_paused),
            generation: Arc::clone(&primary_generation),
            queued_samples: Arc::clone(&primary_queued),
            clear_output_on_clear: true,
        };

        let secondary = DeckWriter {
            deck: DeckId::Secondary,
            data_tx: secondary_tx,
            control_tx: control_tx.clone(),
            output: output.clone(),
            paused: Arc::clone(&secondary_paused),
            generation: Arc::clone(&secondary_generation),
            queued_samples: Arc::clone(&secondary_queued),
            clear_output_on_clear: false,
        };

        let mixer_stop = Arc::clone(&stop_flag);
        let thread = thread::Builder::new()
            .name("audio-deck-mixer".into())
            .spawn(move || {
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
                    control_rx,
                    stop_flag: mixer_stop,
                };
                worker.run();
            })
            .ok();

        Self {
            primary,
            secondary,
            control_tx,
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
        writer.generation.fetch_add(1, Ordering::AcqRel);
        writer.queued_samples.store(0, Ordering::Release);
        let (ack_tx, ack_rx) = mpsc::channel();
        let _ = self
            .control_tx
            .send(MixerControl::ClearDeck { deck, ack: ack_tx });
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
        self.primary.generation.fetch_add(1, Ordering::AcqRel);
        self.secondary.generation.fetch_add(1, Ordering::AcqRel);
        self.primary.queued_samples.store(0, Ordering::Release);
        self.secondary.queued_samples.store(0, Ordering::Release);
        let (ack_tx, ack_rx) = mpsc::channel();
        let _ = self.control_tx.send(MixerControl::ClearAll { ack: ack_tx });
        let _ = ack_rx.recv_timeout(Duration::from_millis(200));
        self.primary.output.clear();
    }
}

impl Drop for DeckMixer {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
        let _ = self.control_tx.send(MixerControl::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct DeckRuntime {
    rx: mpsc::Receiver<Vec<f32>>,
    current_block: Vec<f32>,
    current_index: usize,
    queued_samples: Arc<AtomicUsize>,
    paused: Arc<AtomicBool>,
    gain: f32,
}

impl DeckRuntime {
    fn new(
        rx: mpsc::Receiver<Vec<f32>>,
        queued_samples: Arc<AtomicUsize>,
        paused: Arc<AtomicBool>,
        gain: f32,
    ) -> Self {
        Self {
            rx,
            current_block: Vec::new(),
            current_index: 0,
            queued_samples,
            paused,
            gain,
        }
    }

    fn clear(&mut self) {
        while self.rx.try_recv().is_ok() {}
        self.current_block.clear();
        self.current_index = 0;
        self.queued_samples.store(0, Ordering::Release);
    }

    fn next_sample(&mut self) -> Option<f32> {
        if self.paused.load(Ordering::Acquire) {
            return None;
        }

        if self.current_index >= self.current_block.len() {
            match self.rx.try_recv() {
                Ok(block) => {
                    self.current_block = block;
                    self.current_index = 0;
                }
                Err(_) => return None,
            }
        }

        let sample = self.current_block[self.current_index];
        self.current_index += 1;
        saturating_sub(&self.queued_samples, 1);
        Some(sample)
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
    control_rx: mpsc::Receiver<MixerControl>,
    stop_flag: Arc<AtomicBool>,
}

impl MixerWorker {
    fn run(&mut self) {
        while !self.stop_flag.load(Ordering::Acquire) {
            if !self.drain_controls() {
                break;
            }

            let mut block = vec![0.0; MIX_BLOCK_FRAMES * self.output_channels];
            let mut has_audio = false;
            for sample in &mut block {
                let primary = self.primary.next_sample();
                let secondary = self.secondary.next_sample();
                self.advance_crossfade();
                let mixed = primary.unwrap_or(0.0) * self.primary.gain
                    + secondary.unwrap_or(0.0) * self.secondary.gain;
                if primary.is_some() || secondary.is_some() {
                    has_audio = true;
                }
                *sample = mixed.clamp(-1.0, 1.0);
            }

            if has_audio {
                if !self.output.push_block(block, self.stop_flag.as_ref()) {
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
                MixerControl::ClearDeck { deck, ack } => {
                    self.deck_mut(deck).clear();
                    if self
                        .crossfade
                        .as_ref()
                        .is_some_and(|fade| fade.outgoing == deck || fade.incoming == deck)
                    {
                        self.crossfade = None;
                    }
                    let _ = ack.send(());
                }
                MixerControl::ClearAll { ack } => {
                    self.primary.clear();
                    self.secondary.clear();
                    self.crossfade = None;
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
                MixerControl::Stop => return false,
            }
        }
        true
    }

    fn advance_crossfade(&mut self) {
        let Some(fade) = self.crossfade.as_ref() else {
            return;
        };
        let progress = (fade.elapsed_samples as f32 / fade.duration_samples as f32).clamp(0.0, 1.0);
        let (out_gain, in_gain) = balanced_crossfade_gains(progress, fade.params);
        let outgoing = fade.outgoing;
        let incoming = fade.incoming;
        let elapsed_samples = fade.elapsed_samples.saturating_add(1);
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
