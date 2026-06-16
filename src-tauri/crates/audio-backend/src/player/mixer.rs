use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use crate::decoder::PlaybackSink;
use crate::output::OutputWriter;

const DECK_QUEUE_BLOCKS: usize = 8;
const MIX_BLOCK_FRAMES: usize = 512;

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
        let primary_gain = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let secondary_gain = Arc::new(AtomicU32::new(0.0f32.to_bits()));

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
                    primary: DeckRuntime::new(
                        primary_rx,
                        primary_queued,
                        primary_paused,
                        primary_gain,
                    ),
                    secondary: DeckRuntime::new(
                        secondary_rx,
                        secondary_queued,
                        secondary_paused,
                        secondary_gain,
                    ),
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

    #[allow(dead_code)]
    pub fn secondary_writer(&self) -> DeckWriter {
        self.secondary.clone()
    }

    #[allow(dead_code)]
    pub fn set_deck_gain(&self, deck: DeckId, gain: f32) {
        let _ = self.control_tx.send(MixerControl::SetDeckGain {
            deck,
            gain: gain.clamp(0.0, 2.0),
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
    gain_bits: Arc<AtomicU32>,
}

impl DeckRuntime {
    fn new(
        rx: mpsc::Receiver<Vec<f32>>,
        queued_samples: Arc<AtomicUsize>,
        paused: Arc<AtomicBool>,
        gain_bits: Arc<AtomicU32>,
    ) -> Self {
        Self {
            rx,
            current_block: Vec::new(),
            current_index: 0,
            queued_samples,
            paused,
            gain_bits,
        }
    }

    fn gain(&self) -> f32 {
        f32::from_bits(self.gain_bits.load(Ordering::Acquire))
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
        Some(sample * self.gain())
    }
}

struct MixerWorker {
    output: OutputWriter,
    output_channels: usize,
    primary: DeckRuntime,
    secondary: DeckRuntime,
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
                let mixed = primary.unwrap_or(0.0) + secondary.unwrap_or(0.0);
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
                    let _ = ack.send(());
                }
                MixerControl::ClearAll { ack } => {
                    self.primary.clear();
                    self.secondary.clear();
                    let _ = ack.send(());
                }
                MixerControl::SetDeckGain { deck, gain } => {
                    self.deck_mut(deck)
                        .gain_bits
                        .store(gain.clamp(0.0, 2.0).to_bits(), Ordering::Release);
                }
                MixerControl::Stop => return false,
            }
        }
        true
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
