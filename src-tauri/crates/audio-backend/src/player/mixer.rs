use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::thread;
use std::time::Duration;

use parking_lot::{Mutex, RwLock};

use crate::decoder::PlaybackSink;
use crate::effects::{BiquadCoeffs, BiquadState, DspChain, LimiterRuntime};
use crate::output::{stream_samples_per_sec, OutputWriter, ProducerWait, PushCancel};
use crate::types::{CrossfadeCurve, DspConfig, LimiterConfig};

#[cfg(any(target_os = "android", target_os = "linux"))]
// Match the deeper Android/Linux output queue so decode jitter does not
// immediately become zero-padded mixer blocks (Linux: Pulse/PipeWire server
// scheduling adds the same kind of jitter as the Android scheduler).
const DECK_QUEUE_BLOCKS: usize = 48;
#[cfg(not(any(target_os = "android", target_os = "linux")))]
const DECK_QUEUE_BLOCKS: usize = 8;
const DECK_RECYCLE_QUEUE_BLOCKS: usize = DECK_QUEUE_BLOCKS + 4;

/// Samples a deck's queue reliably holds once full: one block short of capacity, since the last
/// block of a run can be partial. Decoder and mix blocks are both `MIX_BLOCK_FRAMES`.
pub(super) fn deck_queue_full_samples(channels: u16) -> usize {
    (DECK_QUEUE_BLOCKS - 1) * MIX_BLOCK_FRAMES * channels as usize
}
const MIX_BLOCK_FRAMES: usize = 512;
const CROSSFADE_RAMP_FRAMES: usize = 64;
/// Bass swap: a low shelf deep enough to take a kick and bassline out of the mix.
const BASS_SWAP_HZ: f32 = 180.0;
const BASS_SWAP_KILL_DB: f32 = -24.0;
/// Band morph corner frequencies, the Web `SpectralEQ`'s and the analysis bands' edges.
const MORPH_LOW_HZ: f32 = 300.0;
const MORPH_MID_HZ: f32 = 1_100.0;
const MORPH_MID_Q: f32 = 0.7;
const MORPH_HIGH_HZ: f32 = 4_000.0;
/// How far a staged entry holds B's highs down (on the morph's high shelf) until the crossover.
const STAGE_HIGH_DB: f32 = -15.0;
/// Share of the fade over which a staged entry hands the highs from A to B.
const STAGE_HIGH_SPAN: f32 = 0.15;
/// Longest echo-out delay; the worker's delay line is sized for it once, on first use.
pub(crate) const ECHO_MAX_DELAY: f32 = 0.55;
/// Wet gain the echo envelope decays to by its end, where it is dropped (the Web's floor).
const ECHO_FLOOR: f32 = 1e-4;
/// Layouts wider than this skip deck filtering rather than allocate filter state.
const BASS_SWAP_MAX_CHANNELS: usize = 8;
/// FilterCut: the outgoing high-pass climbs 300 Hz -> 18 kHz across the fade.
const FILTER_CUT_FROM_HZ: f32 = 300.0;
const FILTER_CUT_SPAN: f32 = 60.0;
/// Both decks near full volume can clip; held just under full scale, 120 ms release.
const OVERLAP_LIMIT_DB: f32 = -0.26;
const OVERLAP_LIMIT_RELEASE_MS: f32 = 120.0;
// Both decks paused means no deadline at all: the worker has nothing to render
// until a control message arrives, so it blocks on the control channel instead
// of polling. `DeckWriter::set_paused` pokes that channel, so resuming is
// immediate and this timeout only bounds the wait for state nobody announced.
const MIXER_IDLE_WAIT: Duration = Duration::from_millis(100);
// A failed output is waiting for `ReplaceOutput`, which also arrives on the
// control channel.
const MIXER_OUTPUT_FAILED_WAIT: Duration = Duration::from_millis(20);
// Starved-but-playing is an underrun in progress: poll fast at first so the
// first refilled block is picked up immediately, then decay, because the same
// state also covers "stopped with an unpaused deck", which can last forever.
// `DeckWriter::push_block` unparks the worker when a starved deck refills, so
// the decay never delays the audible restart.
const MIXER_STARVED_SPIN_RETRIES: u32 = 2;
const MIXER_STARVED_MIN_PARK_US: u64 = 250;
const MIXER_STARVED_MAX_PARK_US: u64 = 32_000;
// Underrun telemetry window. A track's last block is legitimately short, so a
// threshold above one keeps normal track ends silent while a real starvation
// storm still reports.
const UNDERRUN_REPORT_WINDOW: Duration = Duration::from_secs(5);
const UNDERRUN_REPORT_THRESHOLD: u32 = 2;

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
        start_position: Option<f64>,
        transition: u64,
        params: CrossfadeParams,
    },
    ReplaceOutput {
        output: OutputWriter,
        ack: mpsc::Sender<()>,
    },
    SetDsp(DspConfig),
    /// The echo-out delay line, allocated on the sender's thread the first time a fade asks for
    /// an echo and kept by the worker from then on.
    EchoLine(Box<[f32]>),
    /// Carries no state: it exists so a paused worker blocked on this channel
    /// wakes for changes announced through shared atomics instead.
    Wake,
    Stop,
}

/// Handle to the mixer worker thread, published by the worker itself so both
/// the `DeckMixer` and every cloned `DeckWriter` can interrupt a park.
type WorkerHandle = Arc<OnceLock<thread::Thread>>;

fn wake_worker(handle: &WorkerHandle) {
    if let Some(worker) = handle.get() {
        worker.unpark();
    }
}

#[derive(Clone)]
pub struct DeckWriter {
    deck: DeckId,
    data_tx: mpsc::SyncSender<DeckBlock>,
    control_tx: mpsc::Sender<MixerControl>,
    control_epoch: Arc<AtomicU64>,
    worker: WorkerHandle,
    samples_per_sec: u32,
    output: Arc<RwLock<OutputWriter>>,
    paused: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    flush_epoch: Arc<AtomicU64>,
    queued_samples: Arc<AtomicUsize>,
    recycle_rx: Arc<Mutex<mpsc::Receiver<Vec<f32>>>>,
    stale: Arc<StaleBlocks>,
}

/// What the mixer discarded from a deck as stale (generation or flush epoch it no longer accepts).
/// Written only on a discard, read when an AutoMix cue finds the deck empty with audio queued.
#[derive(Default)]
pub(super) struct StaleBlocks {
    count: AtomicU64,
    /// (generation << 32) | flush_epoch of the last block discarded…
    block: AtomicU64,
    /// …and of what the deck accepted at that moment.
    accepted: AtomicU64,
}

fn pack_epochs(generation: u64, flush_epoch: u64) -> u64 {
    (generation << 32) | (flush_epoch & 0xffff_ffff)
}

fn unpack_epochs(packed: u64) -> (u64, u64) {
    (packed >> 32, packed & 0xffff_ffff)
}

struct DeckBlock {
    samples: Vec<f32>,
    generation: u64,
    flush_epoch: u64,
    position: Option<crate::output::PcmPosition>,
}

#[derive(Clone, Copy, Debug)]
pub struct CrossfadeParams {
    pub curve: CrossfadeCurve,
    pub incoming_gain: f32,
    pub outgoing_gain: f32,
    pub overlap_headroom_db: f32,
    /// Exponent on the incoming curve: above 1 holds the incoming back.
    pub in_shape: f32,
    /// Exponent on the outgoing curve: above 1 clears the outgoing sooner.
    pub out_shape: f32,
    /// Hand the low band from outgoing to incoming over this progress window.
    pub bass_swap: Option<BassSwap>,
    /// Lean each deck's tone toward the other's across the fade.
    pub morph: Option<BandMorph>,
    /// Bring B in early but thin; ignored by `FilterCut`.
    pub stage: Option<BandStage>,
    /// Throw the outgoing deck into an echo as it leaves.
    pub echo: Option<EchoOut>,
    pub style: FadeStyle,
}

/// A DJ's echo-out, with the Web `TransitionEffects` echo throw's shape: the outgoing deck is
/// tapped after its fade gain into a feedback delay from the fade's start; the repeats pass a
/// low-pass, and their wet gain rises to `wet` over the first 15% of `duration`, then decays
/// exponentially to `ECHO_FLOOR` by its end. Runs past the fade when it is longer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EchoOut {
    /// Seconds between repeats, at most `ECHO_MAX_DELAY`.
    pub delay: f32,
    pub feedback: f32,
    pub cutoff_hz: f32,
    pub wet: f32,
    /// Seconds from the fade's start.
    pub duration: f32,
}

/// A running `EchoOut`. Plain data: the delay line it writes is the worker's, so starting and
/// ending an echo allocates and frees nothing on the mixer thread.
#[derive(Clone, Copy)]
struct EchoRuntime {
    channels: usize,
    feedback: f32,
    lpf: BiquadCoeffs,
    lpf_state: [BiquadState; BASS_SWAP_MAX_CHANNELS],
    /// Samples of the line in use (delay frames x channels), and the frame being read/written.
    len: usize,
    pos: usize,
    gain: f32,
    attack_step: f32,
    decay_ratio: f32,
    attack_frames: usize,
    frames: usize,
    elapsed: usize,
}

impl EchoRuntime {
    /// `None` when the line cannot hold it: none sent yet, more channels than the filter state,
    /// or no rate.
    fn new(echo: EchoOut, rate: u32, channels: usize, line: &mut [f32]) -> Option<Self> {
        let rate_f = rate as f32;
        let delay_frames = (echo.delay.clamp(0.01, ECHO_MAX_DELAY) * rate_f) as usize;
        let len = delay_frames * channels;
        if channels == 0 || channels > BASS_SWAP_MAX_CHANNELS || len == 0 || len > line.len() {
            return None;
        }
        line[..len].fill(0.0);
        let frames = (echo.duration.max(0.05) * rate_f) as usize;
        let attack_frames = ((frames as f32 * 0.15) as usize).max(1);
        let wet = echo.wet.max(ECHO_FLOOR);
        Some(Self {
            channels,
            feedback: echo.feedback.clamp(0.0, 0.9),
            lpf: BiquadCoeffs::low_pass(rate_f, echo.cutoff_hz.clamp(200.0, rate_f * 0.45), 0.7),
            lpf_state: Default::default(),
            len,
            pos: 0,
            gain: 0.0,
            attack_step: wet / attack_frames as f32,
            decay_ratio: (ECHO_FLOOR / wet).powf(1.0 / (frames - attack_frames).max(1) as f32),
            attack_frames,
            frames,
            elapsed: 0,
        })
    }

    /// Feed one sample of the outgoing deck (after its gain) and take one sample of repeats.
    /// Channels arrive in order; the last one of a frame moves the line on.
    #[inline(always)]
    fn tap(&mut self, line: &mut [f32], channel: usize, input: f32) -> f32 {
        let i = self.pos + channel;
        let delayed = line[i];
        line[i] = input + delayed * self.feedback;
        let out = self.lpf_state[channel].process(self.lpf, delayed) * self.gain;
        if channel + 1 == self.channels {
            self.pos += self.channels;
            if self.pos >= self.len {
                self.pos = 0;
            }
            self.elapsed += 1;
            if self.elapsed <= self.attack_frames {
                self.gain += self.attack_step;
            } else {
                self.gain *= self.decay_ratio;
            }
        }
        out
    }

    fn done(&self) -> bool {
        self.elapsed >= self.frames
    }

    /// Bring the repeats down to the floor within `frames` instead of cutting them mid-ring.
    fn release(&mut self, frames: usize) {
        let frames = frames.clamp(1, self.frames.saturating_sub(self.elapsed).max(1));
        self.attack_frames = self.elapsed;
        self.frames = self.elapsed + frames;
        self.decay_ratio = (ECHO_FLOOR / self.gain.max(ECHO_FLOOR)).powf(1.0 / frames as f32);
    }
}

/// A DJ's staged entry (as in dnb-autodj's transitions): B is heard early as a body without its
/// highs (and, with a bass swap, without its lows), takes the bass at the `crossover`, and the
/// highs change hands last, over `STAGE_HIGH_SPAN` from it. Two full spectra never sit on top of
/// each other, which is what makes a long overlap muddy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandStage {
    pub crossover: f32,
}

impl BandStage {
    /// High shelf gains (outgoing, incoming) in dB at `progress`. The handover never starts
    /// before the crossover (A may still be singing) and always finishes by the end of the fade,
    /// so B is itself when the filters come off: a late crossover gets a shorter span.
    fn high_db(self, progress: f32) -> (f32, f32) {
        let start = self.crossover.clamp(0.0, 0.95);
        let span = (1.0 - start).min(STAGE_HIGH_SPAN);
        let s = smootherstep(((progress - start) / span).clamp(0.0, 1.0));
        (STAGE_HIGH_DB * s, STAGE_HIGH_DB * (1.0 - s))
    }
}

/// The tonal half of a handover, per band (low, mid, high) in dB: A's spectrum leans toward B's as
/// it leaves (0 -> `d`), and B arrives leaning toward A's and settles into its own (`-d` -> 0), so
/// the tone changes as gradually as the level. Mirrors the Web `SpectralEQ` (low shelf 300 Hz,
/// peaking 1.1 kHz, high shelf 4 kHz). The low band is left to `BassSwap` when one runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandMorph {
    pub db: [f32; 3],
}

/// How the two decks share the fade. Every style but `FilterCut` sets only the balance between
/// the decks; the level follows A's gain to B's (see `fade_gains`). Every style runs the overlap
/// limiter as a safety net: balancing is a power model, and neither two uncorrelated loud masters'
/// peaks nor a band morph's boost or an echo's repeats show up in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FadeStyle {
    /// Constant-power `curve` with `in_shape`/`out_shape`.
    Blend,
    /// A dominates until `handoff` then releases; B is at 0.6 of A by it (A still singing).
    Handoff(f32),
    /// Beat-synced: B is level with A by `handoff`, then A releases.
    BeatMix(f32),
    /// Tempos too far apart to sync: A is high-passed away, holding its level until 70%; B enters
    /// halfway.
    FilterCut,
}

/// Progress window (0..1 of the fade) over which the low band changes hands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BassSwap {
    pub start: f32,
    pub end: f32,
}

impl BassSwap {
    /// The middle fifth of the fade, for a pair whose beats are not matched.
    pub const MIDDLE: Self = Self {
        start: 0.4,
        end: 0.6,
    };

    /// Shelf gains (outgoing, incoming) in dB at `progress`. Smootherstep, so the shelf, too,
    /// starts and stops moving at rest.
    fn shelf_db(self, progress: f32) -> (f32, f32) {
        let x = ((progress - self.start) / (self.end - self.start).max(1e-6)).clamp(0.0, 1.0);
        let s = smootherstep(x);
        (BASS_SWAP_KILL_DB * s, BASS_SWAP_KILL_DB * (1.0 - s))
    }
}

impl Default for CrossfadeParams {
    fn default() -> Self {
        Self {
            curve: CrossfadeCurve::EqualPower,
            incoming_gain: 1.0,
            outgoing_gain: 1.0,
            overlap_headroom_db: -0.8,
            in_shape: 1.0,
            out_shape: 1.0,
            bass_swap: None,
            morph: None,
            stage: None,
            echo: None,
            style: FadeStyle::Blend,
        }
    }
}

impl DeckWriter {
    fn push_with_position(
        &self,
        mut block: Vec<f32>,
        position: Option<crate::output::PcmPosition>,
        cancel: PushCancel<'_>,
    ) -> bool {
        if block.is_empty() {
            return true;
        }

        let generation = self.generation.load(Ordering::Acquire);
        let flush_epoch = self.flush_epoch.load(Ordering::Acquire);
        let mut wait = ProducerWait::new(self.samples_per_sec);
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
                position,
            };
            let queued_before = self.queued_samples.fetch_add(block_len, Ordering::Release);
            match self.data_tx.try_send(deck_block) {
                Ok(()) => {
                    if cancel.is_cancelled()
                        || self.generation.load(Ordering::Acquire) != generation
                        || self.flush_epoch.load(Ordering::Acquire) != flush_epoch
                    {
                        saturating_sub(&self.queued_samples, block_len);
                        return false;
                    }
                    if queued_before == 0 {
                        // This deck just came back from empty. The worker may
                        // have decayed into a long starved park; wake it so the
                        // first block after a start, seek or underrun is mixed
                        // immediately.
                        wake_worker(&self.worker);
                    }
                    return true;
                }
                Err(mpsc::TrySendError::Full(returned)) => {
                    let queued = saturating_sub(&self.queued_samples, block_len);
                    block = returned.samples;
                    wait.backoff(block_len, queued);
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    saturating_sub(&self.queued_samples, block_len);
                    return false;
                }
            }
        }
    }
}

impl PlaybackSink for DeckWriter {
    fn push_block(&self, block: Vec<f32>, cancel: PushCancel<'_>) -> bool {
        self.push_with_position(block, None, cancel)
    }

    fn push_timed_block(
        &self,
        block: Vec<f32>,
        position: crate::output::PcmPosition,
        cancel: PushCancel<'_>,
    ) -> bool {
        self.push_with_position(block, Some(position), cancel)
    }

    fn flush_for_seek(&self) {
        self.flush_deck_for_seek();
    }

    fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
        // Pause state lives in an atomic the worker only samples per block, so
        // announce the edge as well: an idle worker is blocked on the control
        // channel and a busy one may be parked on a full output ring.
        let _ = self.control_tx.send(MixerControl::Wake);
        wake_worker(&self.worker);
    }

    fn queued_samples(&self) -> usize {
        self.queued_samples.load(Ordering::Acquire) + self.output.read().queued_samples()
    }

    fn output_latency_us(&self) -> u32 {
        self.output.read().output_latency_us()
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
    /// Discarded-block count, the last discarded block's (generation, flush epoch), what the deck
    /// accepted then, and this writer's current pair.
    pub(super) fn stale_blocks(&self) -> (u64, (u64, u64), (u64, u64), (u64, u64)) {
        (
            self.stale.count.load(Ordering::Relaxed),
            unpack_epochs(self.stale.block.load(Ordering::Relaxed)),
            unpack_epochs(self.stale.accepted.load(Ordering::Relaxed)),
            (
                self.generation.load(Ordering::Acquire),
                self.flush_epoch.load(Ordering::Acquire),
            ),
        )
    }

    /// This deck's own queue, excluding what the shared output already holds.
    pub(super) fn deck_queued_samples(&self) -> usize {
        self.queued_samples.load(Ordering::Acquire)
    }

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
        wake_worker(&self.worker);
        let _ = ack_rx.recv_timeout(Duration::from_millis(200));
    }
}

pub struct DeckMixer {
    primary: DeckWriter,
    secondary: DeckWriter,
    control_tx: mpsc::Sender<MixerControl>,
    worker: WorkerHandle,
    output: Arc<RwLock<OutputWriter>>,
    stop_flag: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    /// Whether the worker already holds its echo delay line; see `MixerControl::EchoLine`.
    echo_line_sent: AtomicBool,
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
        let primary_stale = Arc::new(StaleBlocks::default());
        let secondary_stale = Arc::new(StaleBlocks::default());
        let control_epoch = Arc::new(AtomicU64::new(0));
        let worker: WorkerHandle = Arc::new(OnceLock::new());
        let samples_per_sec = stream_samples_per_sec(output_sample_rate, output_channels);
        let output_state = Arc::new(RwLock::new(output.clone()));
        let primary = DeckWriter {
            deck: DeckId::Primary,
            data_tx: primary_tx,
            control_tx: control_tx.clone(),
            control_epoch: Arc::clone(&control_epoch),
            worker: Arc::clone(&worker),
            samples_per_sec,
            output: Arc::clone(&output_state),
            paused: Arc::clone(&primary_paused),
            generation: Arc::clone(&primary_generation),
            flush_epoch: Arc::clone(&primary_flush_epoch),
            queued_samples: Arc::clone(&primary_queued),
            recycle_rx: Arc::new(Mutex::new(primary_recycle_rx)),
            stale: Arc::clone(&primary_stale),
        };

        let secondary = DeckWriter {
            deck: DeckId::Secondary,
            data_tx: secondary_tx,
            control_tx: control_tx.clone(),
            control_epoch: Arc::clone(&control_epoch),
            worker: Arc::clone(&worker),
            samples_per_sec,
            output: Arc::clone(&output_state),
            paused: Arc::clone(&secondary_paused),
            generation: Arc::clone(&secondary_generation),
            flush_epoch: Arc::clone(&secondary_flush_epoch),
            queued_samples: Arc::clone(&secondary_queued),
            recycle_rx: Arc::new(Mutex::new(secondary_recycle_rx)),
            stale: Arc::clone(&secondary_stale),
        };

        let mixer_stop = Arc::clone(&stop_flag);
        let mixer_worker_handle = Arc::clone(&worker);
        let dsp_config = dsp_config.clone();
        let thread = thread::Builder::new()
            .name("audio-deck-mixer".into())
            .spawn(move || {
                // The mixer is the last thread between decoded PCM and the
                // device callback, so it carries the tightest deadline of the
                // two producers — and it is strictly periodic, emitting exactly
                // one MIX_BLOCK_FRAMES block per wake.
                let _priority = crate::rt_priority::promote_current_thread(
                    crate::rt_priority::AudioThreadKind::Mixer,
                    MIX_BLOCK_FRAMES,
                    output_sample_rate,
                );
                let _ = mixer_worker_handle.set(thread::current());
                let mut dsp = DspChain::new(output_sample_rate, output_channels);
                dsp.set_dsp(&dsp_config);
                let mut worker = MixerWorker {
                    overlap_limiter: None,
                    echo: None,
                    echo_line: Box::default(),
                    output,
                    output_channels: output_channels.max(1) as usize,
                    primary: DeckRuntime {
                        stale: primary_stale,
                        ..DeckRuntime::new(
                            primary_rx,
                            primary_queued,
                            primary_paused,
                            primary_recycle_tx,
                            1.0,
                        )
                    },
                    secondary: DeckRuntime {
                        stale: secondary_stale,
                        ..DeckRuntime::new(
                            secondary_rx,
                            secondary_queued,
                            secondary_paused,
                            secondary_recycle_tx,
                            0.0,
                        )
                    },
                    crossfade: None,
                    scheduled: None,
                    transition: 0,
                    phase: 0,
                    expected_timeline: None,
                    last_phase: (0, 0),
                    block_anchor: None,
                    block_target: MIX_BLOCK_FRAMES * output_channels.max(1) as usize,
                    sample_rate: output_sample_rate,
                    dsp,
                    control_rx,
                    control_epoch,
                    stop_flag: mixer_stop,
                    spare_block: None,
                    underruns_in_window: 0,
                    underrun_window_start: None,
                };
                worker.run();
            })
            .ok();

        Self {
            primary,
            secondary,
            control_tx,
            worker,
            output: output_state,
            stop_flag,
            thread,
            echo_line_sent: AtomicBool::new(false),
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
        wake_worker(&self.worker);
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
        wake_worker(&self.worker);
        let _ = ack_rx.recv_timeout(Duration::from_millis(200));
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn schedule_crossfade(
        &self,
        outgoing: DeckId,
        incoming: DeckId,
        duration_secs: f64,
        sample_rate: u32,
        channels: u16,
        params: CrossfadeParams,
        start_position: Option<f64>,
        transition: u64,
    ) {
        let frames = (duration_secs.max(0.05) * sample_rate.max(1) as f64).ceil() as usize;
        let duration_samples = frames.saturating_mul(channels.max(1) as usize).max(1);
        // Most sessions never echo: the line is paid for by the first fade that does, and here,
        // so the mixer thread neither allocates it nor ever frees one.
        if params.echo.is_some() && !self.echo_line_sent.swap(true, Ordering::Relaxed) {
            let line = echo_line(sample_rate, channels as usize);
            let _ = self.control_tx.send(MixerControl::EchoLine(line));
        }
        let _ = self.control_tx.send(MixerControl::StartCrossfade {
            outgoing,
            incoming,
            duration_samples,
            start_position,
            transition,
            params,
        });
        wake_worker(&self.worker);
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
        wake_worker(&self.worker);
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
        wake_worker(&self.worker);

        if ack_rx.recv_timeout(Duration::from_millis(200)).is_err() {
            return false;
        }

        *self.output.write() = output;
        true
    }

    pub fn set_dsp(&self, config: DspConfig) {
        let _ = self.control_tx.send(MixerControl::SetDsp(config));
        wake_worker(&self.worker);
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
        wake_worker(&self.worker);
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
    position: Option<crate::output::PcmPosition>,
    stale: Arc<StaleBlocks>,
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
            position: None,
            stale: Arc::default(),
        }
    }

    fn clear(&mut self, generation: u64, flush_epoch: u64) {
        let spent = std::mem::take(&mut self.current_block);
        recycle_deck_buffer(&self.recycle_tx, spent);
        self.current_index = 0;
        self.position = None;
        self.accepted_generation = generation;
        self.accepted_flush_epoch = flush_epoch;
        self.queued_samples.store(0, Ordering::Release);
        // Drain what was queued before the clear now, keeping the first block of the new
        // generation if one is already there. A deck that is mixed drops these within a block
        // anyway, but AutoMix's incoming deck is not read until its cue, and a full queue of
        // stale blocks would hold its new decoder off until the cue found nothing to play.
        self.current_samples();
    }

    fn current_samples(&mut self) -> &[f32] {
        while self.current_index >= self.current_block.len() {
            let Ok(block) = self.rx.try_recv() else {
                return &[];
            };
            if block.generation != self.accepted_generation
                || block.flush_epoch != self.accepted_flush_epoch
            {
                self.stale.count.fetch_add(1, Ordering::Relaxed);
                self.stale.block.store(
                    pack_epochs(block.generation, block.flush_epoch),
                    Ordering::Relaxed,
                );
                self.stale.accepted.store(
                    pack_epochs(self.accepted_generation, self.accepted_flush_epoch),
                    Ordering::Relaxed,
                );
                recycle_deck_buffer(&self.recycle_tx, block.samples);
                continue;
            }
            self.position = block.position;
            let spent = std::mem::replace(&mut self.current_block, block.samples);
            recycle_deck_buffer(&self.recycle_tx, spent);
            self.current_index = 0;
        }
        &self.current_block[self.current_index..]
    }

    fn source_position(&mut self, samples_per_second: f64) -> Option<crate::output::PcmPosition> {
        if self.current_samples().is_empty() {
            return None;
        }
        self.position.map(|mut position| {
            position.seconds += self.current_index as f64 / samples_per_second * position.rate;
            position
        })
    }

    fn skip_preroll(&mut self) {
        loop {
            if self.current_samples().is_empty()
                || self.position.is_none_or(|position| position.rate != 0.0)
            {
                break;
            }
            let count = self.current_block.len() - self.current_index;
            self.current_index += count;
            self.commit_consumed(count);
        }
    }

    #[cfg(test)]
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
    /// `gain` over contiguous runs (which autovectorizes). No clamp here: the
    /// DSP chain runs after the mix, and a limiter fed pre-clipped audio has
    /// nothing left to limit. The device callback owns the one final clamp. Pulls
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
            let src = self.current_samples();
            if src.is_empty() {
                break;
            }
            let run = (samples - written).min(src.len());
            let src = &src[..run];
            // `extend` over a slice iterator is TrustedLen-specialized: one
            // reserve, then straight writes with no per-element capacity check,
            // which keeps this run vectorizable (a push loop re-checks capacity
            // every sample).
            if gain == 1.0 {
                dst.extend_from_slice(src);
            } else {
                dst.extend(src.iter().map(|&s| s * gain));
            }
            self.current_index += run;
            written += run;
            *consumed += run;
        }
        written
    }
}

#[derive(Clone, Copy)]
struct ScheduledCrossfade {
    outgoing: DeckId,
    incoming: DeckId,
    start: f64,
    duration_samples: usize,
    params: CrossfadeParams,
    transition: u64,
}

struct CrossfadeRuntime {
    outgoing: DeckId,
    incoming: DeckId,
    duration_samples: usize,
    elapsed_samples: usize,
    params: CrossfadeParams,
    /// Deck filter state, `[primary, secondary][channel][stage]`; unused without a filter.
    filters: [[[BiquadState; 3]; BASS_SWAP_MAX_CHANNELS]; 2],
}

/// One segment's deck filter coefficients, `[primary, secondary]` per stage. A fade's stage count
/// is fixed by its params and counts only the bands it moves: a single-filter fade (FilterCut, a
/// bass swap alone, a staged entry alone) stays a single biquad per sample.
#[derive(Clone, Copy)]
enum FadeFilters {
    One([[BiquadCoeffs; 1]; 2]),
    Two([[BiquadCoeffs; 2]; 2]),
    Three([[BiquadCoeffs; 3]; 2]),
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
    /// Deck filter coefficients for this segment.
    filters: Option<FadeFilters>,
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
    scheduled: Option<ScheduledCrossfade>,
    transition: u64,
    phase: u64,
    expected_timeline: Option<crate::output::PcmPosition>,
    last_phase: (u64, u64),
    block_anchor: Option<crate::output::OutputAnchor>,
    block_target: usize,
    sample_rate: u32,
    dsp: DspChain,
    control_rx: mpsc::Receiver<MixerControl>,
    control_epoch: Arc<AtomicU64>,
    stop_flag: Arc<AtomicBool>,
    /// Mix buffer kept across an iteration that produced no audio. Dropping it
    /// would free an allocation the output callback recycled for us and force
    /// the next iteration to allocate again.
    spare_block: Option<Vec<f32>>,
    /// Limits an overlap that holds both decks up; kept until it has released.
    overlap_limiter: Option<LimiterRuntime>,
    /// The fade's echo-out, kept until its repeats have died away.
    echo: Option<EchoRuntime>,
    /// `ECHO_MAX_DELAY` of output once a fade has asked for an echo; empty until then.
    echo_line: Box<[f32]>,
    /// Windowed underrun telemetry; see `note_underrun`.
    underruns_in_window: u32,
    underrun_window_start: Option<std::time::Instant>,
}

impl MixerWorker {
    fn run(&mut self) {
        let channels = self.output_channels.max(1);
        let frame_capacity = MIX_BLOCK_FRAMES * channels;
        let mut starved_retries = 0u32;

        while !self.stop_flag.load(Ordering::Acquire) {
            if !self.drain_controls() {
                break;
            }

            if self.output.has_failed() {
                if !self.wait_for_controls(MIXER_OUTPUT_FAILED_WAIT) {
                    break;
                }
                continue;
            }

            // Paused state changes far slower than the sample rate — sample it
            // once per block instead of doing an atomic load per sample.
            let primary_paused = self.primary.paused.load(Ordering::Acquire);
            let secondary_paused = self.secondary.paused.load(Ordering::Acquire);

            if primary_paused && secondary_paused {
                // Nothing can be rendered until something changes, and every
                // such change is announced on the control channel.
                starved_retries = 0;
                if !self.wait_for_controls(MIXER_IDLE_WAIT) {
                    break;
                }
                continue;
            }

            // Whether anything was *supposed* to be rendered this iteration.
            // Sampled before `mix_block`, which advances the crossfade schedule
            // and may retire it. Without this the starved case below is
            // indistinguishable from an idle deck.
            let deck_live = self.crossfade.is_some()
                || (!primary_paused && self.primary.gain != 0.0)
                || (!secondary_paused && self.secondary.gain != 0.0);

            // Reserve exact capacity and fill by pushing — no zero-fill pass,
            // since every slot is written below. The buffer is recycled from
            // the output callback when available, so steady playback does not
            // allocate here (and the callback does not free).
            let mut block = self
                .spare_block
                .take()
                .unwrap_or_else(|| self.output.take_recycled_buffer(frame_capacity));
            block.clear();
            let has_audio = self.mix_block(&mut block, channels, primary_paused, secondary_paused);

            if has_audio {
                starved_retries = 0;
                // A short block means the deck ran dry part-way through the
                // render: the output callback fades the gap, which is the
                // audible dropout. One of these per track is just the final
                // partial block, so only a sustained rate is worth reporting.
                if block.len() < self.block_target {
                    self.note_underrun();
                }
                let control_epoch = self.control_epoch.load(Ordering::Acquire);
                if !self.dsp.is_bypassed() {
                    self.dsp.process_interleaved(&mut block);
                }
                if !self.output.push_block_with_anchor(
                    block,
                    self.block_anchor,
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
                // A live deck that produced nothing at all is the *worst* kind
                // of underrun — the deck queue is completely dry, so the output
                // ring drains and the callback fades to silence. Because the
                // deck and mix block sizes match, this (not a partial block) is
                // what a decoder that cannot keep up actually looks like, so it
                // has to count. An idle deck reaches the same branch and must
                // not.
                if deck_live {
                    self.note_underrun();
                }
                self.spare_block = Some(block);
                starved_retry_backoff(&mut starved_retries);
            }
        }
    }

    /// Count a deck underrun and report a sustained rate of them.
    ///
    /// Both shapes count: a partial block (the deck ran dry part-way through a
    /// render) and a completely empty one from a deck that should have been
    /// playing. The second is the common shape — deck blocks and mix blocks are
    /// both `512 * channels` samples, so a deck that falls behind hands over
    /// nothing at all rather than a fragment, and counting only partial blocks
    /// would have made this warning fire for track ends and nothing else.
    ///
    /// This is deliberately *not* symphonia's `invalid main_data_begin`
    /// warning: that one fires whenever a Layer III decoder starts cold
    /// (including the run-up frames a seek decodes and discards, and the
    /// separate AutoMix analysis decoder that never reaches the speakers), so
    /// it says nothing about whether the output stream broke.
    ///
    /// The window is tumbling and evaluated on entry, so the count and the span
    /// reported always describe each other. A genuine stall keeps this method
    /// being called from the starved branch above, so it crosses a window
    /// boundary and reports while the stall is still happening; a burst shorter
    /// than one window is only summarised when the next underrun arrives, which
    /// is why the elapsed time is logged rather than assumed.
    fn note_underrun(&mut self) {
        let now = std::time::Instant::now();
        let window_start = *self.underrun_window_start.get_or_insert(now);
        let elapsed = now.duration_since(window_start);

        if elapsed >= UNDERRUN_REPORT_WINDOW {
            if self.underruns_in_window > UNDERRUN_REPORT_THRESHOLD {
                tracing::warn!(
                    "混音器欠载：{:.1}s 内 {} 次，解码跟不上输出（输出回调已淡出，听感为断音）",
                    elapsed.as_secs_f32(),
                    self.underruns_in_window
                );
            }
            self.underruns_in_window = 0;
            self.underrun_window_start = Some(now);
        }

        self.underruns_in_window = self.underruns_in_window.saturating_add(1);
    }

    /// Mix one output block worth of samples into `block`, returning whether
    /// any audio was written.
    ///
    /// Fast path: with no crossfade in progress the deck gains are constant for
    /// the whole block, so when exactly one deck is contributing we bulk-copy
    /// it (gain over contiguous runs, which autovectorizes) instead of
    /// running the two-deck mixer. On a starved deck this path emits a *short*
    /// block so the output callback can fade the gap. Crossfades and the rare
    /// case with two constant-gain decks mix contiguous runs instead; a starved
    /// deck contributes silence because the other deck and the fade schedule
    /// must keep advancing in lockstep.
    fn mix_block(
        &mut self,
        block: &mut Vec<f32>,
        channels: usize,
        primary_paused: bool,
        secondary_paused: bool,
    ) -> bool {
        let (block_frames, position) = self.prepare_block(channels);
        let phase = (self.transition, self.phase);
        let want = block_frames * channels;
        self.block_target = want;
        self.block_anchor = None;

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
                // Hand a short block downstream instead of padding the tail with
                // silence. Padding hard-cuts to zero and back on every decode
                // hiccup, and because the block still looks full it also hides
                // the underrun from the output callback, whose
                // UNDERRUN_FADE_FRAMES ramp is the thing that keeps that edge
                // from clicking. A short block is frame-aligned, plays out what
                // we actually have, and lets `render.rs` own the fade.
                let frame_aligned = written - (written % channels);
                block.truncate(frame_aligned);
                self.run_echo_tail(block, channels);
                // Skipping the limiter here, as this path used to, dropped whatever reduction a
                // limited fade left to unity in one block: a jump the moment it completed.
                self.run_overlap_limiter(block, channels);
                self.record_timeline(position, frame_aligned, phase);
                return frame_aligned > 0;
            }

            if !primary_active && !secondary_active {
                // Nothing to render — leave `block` empty; the caller sleeps.
                return false;
            }
            // Both active without a crossfade — fall through to the mixer.
        }

        // General path: evaluate gains per frame, but consume PCM in runs.
        let mut has_audio = false;
        let mut consumed_primary = 0usize;
        let mut consumed_secondary = 0usize;

        // Crossfade shaping contains transcendental math (sin/cos/sqrt/powf).
        // Evaluate it at short segment endpoints, then use a smooth linear gain
        // ramp per frame. At 48 kHz this cuts complex gain calculations by ~32x
        // without allocations, while a 64-frame segment keeps even the minimum
        // 50 ms fade closely aligned with its requested curve.
        let mut mixed_frames = 0;
        while mixed_frames < block_frames {
            let segment_frames = (block_frames - mixed_frames).min(CROSSFADE_RAMP_FRAMES);
            let crossfade_ramp = self.crossfade_block_ramp(segment_frames);

            let paused = (primary_paused, secondary_paused);
            let filters = crossfade_ramp
                .and_then(|ramp| ramp.filters)
                .zip(self.crossfade.as_mut());
            let segment_start = block.len();
            // During a fade the echo taps the outgoing deck inside the mix; after it, it rings.
            let echo = self
                .echo
                .as_mut()
                .filter(|_| crossfade_ramp.is_some())
                .map(|echo| (echo, &mut *self.echo_line));
            let decks = (&mut self.primary, &mut self.secondary);
            let segment = (
                &mut *block,
                channels,
                segment_frames,
                paused,
                crossfade_ramp,
            );
            let (primary, secondary) = match filters {
                Some((FadeFilters::One(coeffs), fade)) => mix_segment_echoing(
                    decks,
                    echo,
                    segment,
                    BiquadFilters::new(&mut fade.filters, coeffs),
                ),
                Some((FadeFilters::Two(coeffs), fade)) => mix_segment_echoing(
                    decks,
                    echo,
                    segment,
                    BiquadFilters::new(&mut fade.filters, coeffs),
                ),
                Some((FadeFilters::Three(coeffs), fade)) => mix_segment_echoing(
                    decks,
                    echo,
                    segment,
                    BiquadFilters::new(&mut fade.filters, coeffs),
                ),
                None => mix_segment_echoing(decks, echo, segment, NoFilter),
            };
            has_audio |= primary > 0 || secondary > 0;
            consumed_primary += primary;
            consumed_secondary += secondary;

            if crossfade_ramp.is_none() {
                self.run_echo_tail(&mut block[segment_start..], channels);
            } else if self.echo.is_some_and(|echo| echo.done()) {
                self.echo = None;
            }
            self.run_overlap_limiter(&mut block[segment_start..], channels);
            if crossfade_ramp.is_some() {
                self.commit_crossfade_block(segment_frames);
            }
            mixed_frames += segment_frames;
        }

        self.primary.commit_consumed(consumed_primary);
        self.secondary.commit_consumed(consumed_secondary);
        self.record_timeline(position, block.len(), phase);
        has_audio
    }

    fn prepare_block(&mut self, channels: usize) -> (usize, Option<crate::output::PcmPosition>) {
        let samples_per_second = self.sample_rate as f64 * channels as f64;
        let mut frames = MIX_BLOCK_FRAMES;
        if let Some(schedule) = self.scheduled {
            if let Some(position) = self
                .deck_mut(schedule.outgoing)
                .source_position(samples_per_second)
            {
                if position.rate > 0.0 {
                    let remaining = (schedule.start - position.seconds) * self.sample_rate as f64
                        / position.rate;
                    if remaining < 0.5 {
                        self.scheduled = None;
                        self.deck_mut(schedule.incoming).skip_preroll();
                        if self
                            .deck_mut(schedule.incoming)
                            .current_samples()
                            .is_empty()
                        {
                            self.transition = schedule.transition;
                            self.phase = 3;
                        } else {
                            self.begin_crossfade(schedule);
                        }
                    } else {
                        frames = frames.min(remaining.round() as usize).max(1);
                    }
                }
            }
        }
        let active = if let Some(fade) = &self.crossfade {
            frames = frames
                .min((fade.duration_samples - fade.elapsed_samples).div_ceil(channels))
                .max(1);
            let outgoing = fade.outgoing;
            let incoming = fade.incoming;
            self.deck_mut(incoming).skip_preroll();
            outgoing
        } else if self.primary.gain != 0.0 {
            DeckId::Primary
        } else {
            DeckId::Secondary
        };
        let position = self.deck_mut(active).source_position(samples_per_second);
        if position.is_some_and(|position| position.rate != 1.0) {
            let available = self.deck_mut(active).current_samples().len() / channels;
            frames = frames.min(available).max(1);
        }
        (frames, position)
    }

    /// Whatever is left of a fade's overlap limiter. It limits only while a fade that needs it
    /// runs; otherwise it just releases and then goes. Limiting on after the fade would hold a
    /// zero-attack limiter over the next track, which can sit above its ceiling at its matched
    /// gain, and dropping it at once would snap the level back up.
    fn run_overlap_limiter(&mut self, samples: &mut [f32], channels: usize) {
        let limiting = self.crossfade.is_some();
        let Some(limiter) = self.overlap_limiter.as_mut() else {
            return;
        };
        if limiting {
            limiter.process_interleaved(samples, channels);
        } else {
            limiter.release_interleaved(samples, channels);
            if limiter.gain >= 0.9999 {
                self.overlap_limiter = None;
            }
        }
    }

    /// What is left of an echo-out once its fade has ended: the repeats ring on into the block,
    /// fed nothing new, until the envelope reaches its floor.
    fn run_echo_tail(&mut self, samples: &mut [f32], channels: usize) {
        let Some(echo) = self.echo.as_mut() else {
            return;
        };
        for frame in samples.chunks_exact_mut(channels) {
            for (channel, sample) in frame.iter_mut().enumerate() {
                *sample += echo.tap(&mut self.echo_line, channel, 0.0);
            }
        }
        if echo.done() {
            self.echo = None;
        }
    }

    fn begin_crossfade(&mut self, schedule: ScheduledCrossfade) {
        self.set_deck_gain_direct(schedule.outgoing, schedule.params.outgoing_gain);
        self.set_deck_gain_direct(schedule.incoming, 0.0);
        self.transition = schedule.transition;
        self.phase = 1;
        self.crossfade = Some(CrossfadeRuntime {
            outgoing: schedule.outgoing,
            incoming: schedule.incoming,
            duration_samples: schedule.duration_samples.max(1),
            elapsed_samples: 0,
            params: schedule.params,
            filters: Default::default(),
        });
        self.echo = schedule.params.echo.and_then(|echo| {
            EchoRuntime::new(
                echo,
                self.sample_rate,
                self.output_channels,
                &mut self.echo_line,
            )
        });
        if self.overlap_limiter.is_none() {
            let config = LimiterConfig {
                enabled: true,
                threshold_db: OVERLAP_LIMIT_DB,
                ceiling_db: OVERLAP_LIMIT_DB,
                release_ms: OVERLAP_LIMIT_RELEASE_MS,
            };
            self.overlap_limiter = Some(LimiterRuntime::new(config, self.sample_rate as f32));
        }
    }

    fn record_timeline(
        &mut self,
        position: Option<crate::output::PcmPosition>,
        samples: usize,
        phase: (u64, u64),
    ) {
        let Some(position) = position.filter(|_| samples > 0) else {
            return;
        };
        let changed = self.expected_timeline.is_none_or(|expected| {
            expected.playback_id != position.playback_id
                || (expected.rate - position.rate).abs() > 1e-9
                || (expected.seconds - position.seconds).abs() > 2.0 / self.sample_rate as f64
        });
        if changed || self.last_phase != phase {
            self.block_anchor = Some(crate::output::OutputAnchor {
                source: position,
                transition: phase.0,
                phase: phase.1,
            });
        }
        self.last_phase = phase;
        self.expected_timeline = Some(crate::output::PcmPosition {
            seconds: position.seconds
                + samples as f64 / (self.sample_rate as f64 * self.output_channels as f64)
                    * position.rate,
            ..position
        });
    }

    fn drain_controls(&mut self) -> bool {
        while let Ok(control) = self.control_rx.try_recv() {
            if !self.apply_control(control) {
                return false;
            }
        }
        true
    }

    /// Block until a control message arrives or `timeout` elapses. Used for the
    /// states the worker cannot leave on its own (paused, failed output), so
    /// idling costs one wakeup per timeout instead of a poll cadence.
    fn wait_for_controls(&mut self, timeout: Duration) -> bool {
        match self.control_rx.recv_timeout(timeout) {
            Ok(control) => self.apply_control(control) && self.drain_controls(),
            Err(mpsc::RecvTimeoutError::Timeout) => true,
            Err(mpsc::RecvTimeoutError::Disconnected) => false,
        }
    }

    fn apply_control(&mut self, control: MixerControl) -> bool {
        if matches!(
            &control,
            MixerControl::FlushDeck { .. }
                | MixerControl::ClearDeck { .. }
                | MixerControl::ClearAll { .. }
        ) {
            self.scheduled = None;
            self.expected_timeline = None;
            self.transition = 0;
            self.phase = 0;
            // A cancelled fade or a seek lets the repeats die away over 50 ms rather than cutting
            // them; a replaced queue takes them with everything else.
            if matches!(&control, MixerControl::ClearAll { .. }) {
                self.echo = None;
            } else if let Some(echo) = self.echo.as_mut() {
                echo.release(self.sample_rate as usize / 20);
            }
        }
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
                start_position,
                transition,
            } => {
                let schedule = ScheduledCrossfade {
                    outgoing,
                    incoming,
                    duration_samples,
                    params,
                    start: start_position.unwrap_or(0.0),
                    transition,
                };
                if start_position.is_some() {
                    self.scheduled = Some(schedule);
                } else {
                    self.deck_mut(incoming).skip_preroll();
                    self.begin_crossfade(schedule);
                }
            }
            MixerControl::ReplaceOutput { output, ack } => {
                self.output.retire();
                self.output = output;
                self.expected_timeline = None;
                let _ = ack.send(());
            }
            MixerControl::SetDsp(config) => {
                self.dsp.set_dsp(&config);
            }
            // Sent once per worker, replacing the empty default: nothing is freed here.
            MixerControl::EchoLine(line) => self.echo_line = line,
            MixerControl::Wake => {}
            MixerControl::Stop => return false,
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
        let (outgoing_start, incoming_start) = fade_gains(start_progress, fade.params);
        let completes = remaining_samples <= active_frames.saturating_mul(step);
        let (outgoing_end, incoming_end) = if completes {
            (0.0, fade.params.incoming_gain)
        } else {
            let end_elapsed = fade
                .elapsed_samples
                .saturating_add((active_frames.saturating_sub(1)).saturating_mul(step));
            let end_progress = (end_elapsed as f32 / fade.duration_samples as f32).clamp(0.0, 1.0);
            fade_gains(end_progress, fade.params)
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
            filters: self.deck_filters(fade, start_progress),
        })
    }

    /// Coefficients for this segment: FilterCut's sweep on the outgoing deck, else whichever of
    /// the low (bass swap or morph), mid (morph) and high (morph and staged entry) bands move,
    /// else none.
    fn deck_filters(&self, fade: &CrossfadeRuntime, progress: f32) -> Option<FadeFilters> {
        if self.output_channels > BASS_SWAP_MAX_CHANNELS {
            return None;
        }
        deck_filters(
            self.sample_rate as f32,
            fade.params,
            fade.outgoing,
            progress,
        )
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
            self.phase = 2;
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

// Queue and pause checks belong at contiguous PCM boundaries, not at every
// sample. A missing deck contributes silence until the next run/64-frame segment.
#[allow(clippy::too_many_arguments)]
fn mix_segment<L: DeckFilter>(
    primary: &mut DeckRuntime,
    secondary: &mut DeckRuntime,
    block: &mut Vec<f32>,
    channels: usize,
    frames: usize,
    paused: (bool, bool),
    ramp: Option<CrossfadeBlockRamp>,
    filter: &mut L,
) -> (usize, usize) {
    let gains = (primary.gain, secondary.gain);
    let start = block.len();
    let samples = frames * channels;
    block.resize(start + samples, 0.0);
    let dst = &mut block[start..];
    let mut written = 0;
    let mut consumed = (0, 0);

    while written < samples {
        let p = if paused.0 {
            &[]
        } else {
            primary.current_samples()
        };
        let s = if paused.1 {
            &[]
        } else {
            secondary.current_samples()
        };
        let available = (!p.is_empty(), !s.is_empty());
        let mut run = samples - written;
        if available.0 {
            run = run.min(p.len());
        }
        if available.1 {
            run = run.min(s.len());
        }
        // Decoder blocks and every consumption boundary contain whole frames.
        debug_assert_eq!(run % channels, 0);
        let output = &mut dst[written..written + run];
        let frame_offset = written / channels;
        match available {
            (true, true) => mix_pcm_run::<true, true, L>(
                p,
                s,
                output,
                channels,
                frame_offset,
                gains,
                ramp,
                filter,
            ),
            (true, false) => mix_pcm_run::<true, false, L>(
                p,
                s,
                output,
                channels,
                frame_offset,
                gains,
                ramp,
                filter,
            ),
            (false, true) => mix_pcm_run::<false, true, L>(
                p,
                s,
                output,
                channels,
                frame_offset,
                gains,
                ramp,
                filter,
            ),
            (false, false) => break,
        }
        if available.0 {
            primary.current_index += run;
            consumed.0 += run;
        }
        if available.1 {
            secondary.current_index += run;
            consumed.1 += run;
        }
        written += run;
    }
    consumed
}

/// A per-deck filter ahead of the gain: a no-op outside a swap or sweep, so the
/// ordinary mix monomorphises to exactly what it was.
trait DeckFilter {
    fn apply(&mut self, deck: usize, channel: usize, sample: f32) -> f32;

    /// Whether `tap` is called at all; a const so a fade without an echo compiles without it
    /// (`x + 0.0` is not `x` in IEEE arithmetic, so a no-op tap would still cost an add).
    const TAPS: bool = false;

    /// Post-gain tap of the outgoing deck during a fade: takes its contribution to this sample
    /// and returns what to add to the mix. Nothing outside an echo-out.
    #[inline(always)]
    fn tap(&mut self, _channel: usize, _outgoing: f32) -> f32 {
        0.0
    }
}

/// A fade's deck filter with an echo-out on the outgoing deck's post-gain signal.
struct Echoing<'a, F> {
    filter: F,
    echo: &'a mut EchoRuntime,
    line: &'a mut [f32],
}

impl<F: DeckFilter> DeckFilter for Echoing<'_, F> {
    const TAPS: bool = true;

    #[inline(always)]
    fn apply(&mut self, deck: usize, channel: usize, sample: f32) -> f32 {
        self.filter.apply(deck, channel, sample)
    }

    #[inline(always)]
    fn tap(&mut self, channel: usize, outgoing: f32) -> f32 {
        self.echo.tap(self.line, channel, outgoing)
    }
}

/// `mix_segment` with the echo-out wrapped around the filter when one is running, so a fade
/// without one monomorphises to exactly what it was.
#[allow(clippy::type_complexity)]
fn mix_segment_echoing<F: DeckFilter>(
    (primary, secondary): (&mut DeckRuntime, &mut DeckRuntime),
    echo: Option<(&mut EchoRuntime, &mut [f32])>,
    (block, channels, frames, paused, ramp): (
        &mut Vec<f32>,
        usize,
        usize,
        (bool, bool),
        Option<CrossfadeBlockRamp>,
    ),
    mut filter: F,
) -> (usize, usize) {
    match echo {
        Some((echo, line)) => mix_segment(
            primary,
            secondary,
            block,
            channels,
            frames,
            paused,
            ramp,
            &mut Echoing { filter, echo, line },
        ),
        None => mix_segment(
            primary,
            secondary,
            block,
            channels,
            frames,
            paused,
            ramp,
            &mut filter,
        ),
    }
}

/// The worker's echo delay line: `ECHO_MAX_DELAY` of output frames.
fn echo_line(rate: u32, channels: usize) -> Box<[f32]> {
    vec![0.0; (ECHO_MAX_DELAY * rate as f32) as usize * channels.max(1)].into_boxed_slice()
}

struct NoFilter;

impl DeckFilter for NoFilter {
    #[inline(always)]
    fn apply(&mut self, _: usize, _: usize, sample: f32) -> f32 {
        sample
    }
}

/// `N` stages in series per deck; the count is a const so the per-sample loop is unrolled and a
/// one-stage fade pays for one biquad.
struct BiquadFilters<'a, const N: usize> {
    state: &'a mut [[[BiquadState; 3]; BASS_SWAP_MAX_CHANNELS]; 2],
    coeffs: [[BiquadCoeffs; N]; 2],
    /// Decks with a stage that is not the pass-through. Most of a staged entry or a bass swap
    /// leaves one deck's whole chain at identity (A before its band moves, B after), and an
    /// identity biquad's state stays at zero, so skipping it is exact.
    active: [bool; 2],
}

impl<'a, const N: usize> BiquadFilters<'a, N> {
    fn new(
        state: &'a mut [[[BiquadState; 3]; BASS_SWAP_MAX_CHANNELS]; 2],
        coeffs: [[BiquadCoeffs; N]; 2],
    ) -> Self {
        let active = coeffs.map(|deck| deck.iter().any(|stage| !stage.is_identity()));
        Self {
            state,
            coeffs,
            active,
        }
    }
}

impl<const N: usize> DeckFilter for BiquadFilters<'_, N> {
    #[inline(always)]
    fn apply(&mut self, deck: usize, channel: usize, sample: f32) -> f32 {
        if !self.active[deck] {
            return sample;
        }
        let state = &mut self.state[deck][channel];
        let mut sample = sample;
        for stage in 0..N {
            sample = state[stage].process(self.coeffs[deck][stage], sample);
        }
        sample
    }
}

/// See `MixerWorker::deck_filters`.
fn deck_filters(
    rate: f32,
    params: CrossfadeParams,
    outgoing: DeckId,
    progress: f32,
) -> Option<FadeFilters> {
    fn order<T>(outgoing: DeckId, out: T, inc: T) -> [T; 2] {
        match outgoing {
            DeckId::Primary => [out, inc],
            DeckId::Secondary => [inc, out],
        }
    }
    let swap_shelves = |swap: BassSwap| {
        let (out_db, in_db) = swap.shelf_db(progress);
        // Outside its window one side is exactly 0 dB: no design, and skippable.
        let shelf = |db: f32| {
            if db == 0.0 {
                BiquadCoeffs::IDENTITY
            } else {
                BiquadCoeffs::low_shelf(rate, BASS_SWAP_HZ, 0.707, db)
            }
        };
        (shelf(out_db), shelf(in_db))
    };
    if params.style == FadeStyle::FilterCut {
        let hz = (FILTER_CUT_FROM_HZ * FILTER_CUT_SPAN.powf(progress)).clamp(10.0, rate * 0.45);
        return Some(FadeFilters::One(order(
            outgoing,
            [BiquadCoeffs::high_pass(rate, hz, 0.707)],
            [BiquadCoeffs::IDENTITY],
        )));
    }
    let morph = params.morph.map_or([0.0; 3], |morph| morph.db);
    // Only the bands the fade moves are filtered. Decided from the params, not from this
    // segment's values, so a stage never changes slot (and filter state) part way through.
    let active = [
        params.bass_swap.is_some() || morph[0] != 0.0,
        morph[1] != 0.0,
        morph[2] != 0.0 || params.stage.is_some(),
    ];
    // Outgoing leans 0 -> d, incoming -d -> 0, on the same eased progress as the gains; `extra`
    // (the staged entry) adds on in dB. A side that does not move is an exact pass-through.
    let lean = (eased_progress(progress), eased_progress(1.0 - progress));
    fn band(
        db: f32,
        (lean_out, lean_in): (f32, f32),
        (extra_out, extra_in): (f32, f32),
        design: impl Fn(f32) -> BiquadCoeffs,
    ) -> (BiquadCoeffs, BiquadCoeffs) {
        let side = |db: f32| {
            if db == 0.0 {
                BiquadCoeffs::IDENTITY
            } else {
                design(db)
            }
        };
        (
            side(db * lean_out + extra_out),
            side(-db * lean_in + extra_in),
        )
    }
    let low = match params.bass_swap {
        Some(swap) => swap_shelves(swap),
        None => band(morph[0], lean, (0.0, 0.0), |db| {
            BiquadCoeffs::low_shelf(rate, MORPH_LOW_HZ, 0.707, db)
        }),
    };
    let mid = band(morph[1], lean, (0.0, 0.0), |db| {
        BiquadCoeffs::peaking(rate, MORPH_MID_HZ, MORPH_MID_Q, db)
    });
    let staged = params
        .stage
        .map_or((0.0, 0.0), |stage| stage.high_db(progress));
    let high = band(morph[2], lean, staged, |db| {
        BiquadCoeffs::high_shelf(rate, MORPH_HIGH_HZ, 0.707, db)
    });
    let (mut out, mut inc) = ([BiquadCoeffs::IDENTITY; 3], [BiquadCoeffs::IDENTITY; 3]);
    let mut stages = 0;
    for (stage, on) in [low, mid, high].into_iter().zip(active) {
        if on {
            (out[stages], inc[stages]) = stage;
            stages += 1;
        }
    }
    Some(match stages {
        0 => return None,
        1 => FadeFilters::One(order(outgoing, [out[0]], [inc[0]])),
        2 => FadeFilters::Two(order(outgoing, [out[0], out[1]], [inc[0], inc[1]])),
        _ => FadeFilters::Three(order(outgoing, out, inc)),
    })
}

#[inline]
#[allow(clippy::too_many_arguments)]
fn mix_pcm_run<const PRIMARY: bool, const SECONDARY: bool, L: DeckFilter>(
    primary: &[f32],
    secondary: &[f32],
    output: &mut [f32],
    channels: usize,
    frame_offset: usize,
    gains: (f32, f32),
    ramp: Option<CrossfadeBlockRamp>,
    filter: &mut L,
) {
    let outgoing_primary = ramp.is_some_and(|ramp| ramp.outgoing == DeckId::Primary);
    for (frame_index, frame) in output.chunks_exact_mut(channels).enumerate() {
        let (primary_gain, secondary_gain) = match ramp {
            Some(ramp) => {
                let (outgoing, incoming) = ramp.gains(frame_offset + frame_index);
                match ramp.outgoing {
                    DeckId::Primary => (outgoing, incoming),
                    DeckId::Secondary => (incoming, outgoing),
                }
            }
            None => gains,
        };
        let start = frame_index * channels;
        for (channel, sample) in frame.iter_mut().enumerate() {
            let p = if PRIMARY {
                filter.apply(0, channel, primary[start + channel])
            } else {
                0.0
            };
            let s = if SECONDARY {
                filter.apply(1, channel, secondary[start + channel])
            } else {
                0.0
            };
            let (p, s) = (p * primary_gain, s * secondary_gain);
            *sample = p + s;
            if L::TAPS {
                *sample += filter.tap(channel, if outgoing_primary { p } else { s });
            }
        }
    }
}

fn saturating_sub(counter: &AtomicUsize, amount: usize) -> usize {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let next = current.saturating_sub(amount);
        match counter.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return next,
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

/// Wait for a deck that is playing but has no data queued. The first retries
/// stay tight so a real underrun recovers on the very next block; sustained
/// starvation (no track loaded, decoder gone) decays to a low idle cadence.
/// A refilling deck unparks this thread, so the decay costs no start latency.
fn starved_retry_backoff(retry_count: &mut u32) {
    let retries = *retry_count;
    *retry_count = retries.saturating_add(1);
    if retries < MIXER_STARVED_SPIN_RETRIES {
        thread::yield_now();
        return;
    }

    let shift = (retries - MIXER_STARVED_SPIN_RETRIES).min(8);
    let park_us = MIXER_STARVED_MIN_PARK_US
        .saturating_mul(1u64 << shift)
        .min(MIXER_STARVED_MAX_PARK_US);
    thread::park_timeout(Duration::from_micros(park_us));
}

/// Share of a fade, at each end, over which its progress eases in and out.
const FADE_EDGE: f32 = 0.06;

/// A fade's progress warped so every curve built on it starts and ends at rest: linear through
/// the middle, with raised-cosine velocity ramps over the first and last `FADE_EDGE`. On plain `t`
/// an equal-power fade sets off at full speed (slope pi/2) and a piecewise style kinks wherever
/// two pieces meet; built on this, gains have zero slope and continuous curvature at both ends of
/// every piece. Monotone, with `w(0) = 0` and `w(1) = 1` exactly (the tail is the head mirrored,
/// so 1.0 is not left to rounding).
fn eased_progress(t: f32) -> f32 {
    use std::f32::consts::PI;
    let t = t.clamp(0.0, 1.0);
    if t > 0.5 {
        return 1.0 - eased_progress(1.0 - t);
    }
    let speed = 1.0 / (1.0 - FADE_EDGE);
    if t < FADE_EDGE {
        speed * (0.5 * t - FADE_EDGE / (2.0 * PI) * (PI * t / FADE_EDGE).sin())
    } else {
        speed * (t - 0.5 * FADE_EDGE)
    }
}

fn smootherstep(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// `min(x, 1.1)` without the corner: unchanged up to 1.0, then easing into 1.1 with matching
/// slope. Bounds how far the balancing may lift a pair whose shapes dip.
fn soft_power_cap(x: f32) -> f32 {
    if x <= 1.0 {
        x
    } else {
        1.0 + 0.1 * (1.0 - (-(x - 1.0) / 0.1).exp())
    }
}

/// Deck gains at `progress` for the fade's style.
///
/// Every style but `FilterCut` only decides the balance — who is heard how much, when — and
/// `balance` then sets the level: the pair's summed power follows A's level to B's level along the
/// eased progress. Handoff and BeatMix used to keep both decks near full in the overlap, a
/// loudness bump of up to +3 dB that the overlap limiter squashed and then released as A left.
/// FilterCut keeps its own levels: its high-pass removes most of A's energy, which a gain model
/// cannot see, so balancing it would duck B.
fn fade_gains(progress: f32, params: CrossfadeParams) -> (f32, f32) {
    use std::f32::consts::{FRAC_PI_2, PI};
    let t = progress.clamp(0.0, 1.0);
    // A holds until `handoff` and then releases; `r` is how far through the release it is.
    let release = |handoff: f32| {
        let h = handoff.clamp(0.05, 0.95);
        let r = ((t - h) / (1.0 - h)).max(0.0);
        (h, r, (eased_progress(1.0 - r) * FRAC_PI_2).sin())
    };
    let rise = |share: f32| (eased_progress(share) * FRAC_PI_2).sin();
    let (out_vol, in_vol) = match params.style {
        FadeStyle::Blend => return balanced_crossfade_gains(t, params),
        FadeStyle::Handoff(handoff) => {
            let (h, r, out) = release(handoff);
            let inc = if t < h {
                0.6 * rise(t / h)
            } else {
                0.6 + 0.4 * rise(r)
            };
            (out, inc)
        }
        FadeStyle::BeatMix(handoff) => {
            let (h, _, out) = release(handoff);
            (out, rise(t / h))
        }
        FadeStyle::FilterCut => {
            let out = (eased_progress((1.0 - t) / 0.3) * FRAC_PI_2).sin();
            let inc = rise((t - 0.5) / 0.5);
            let headroom = 10f32.powf(params.overlap_headroom_db * (t * PI).sin().powi(2) / 20.0);
            return (
                out.max(0.0) * params.outgoing_gain.clamp(0.0, 2.0) * headroom,
                inc.max(0.0) * params.incoming_gain.clamp(0.0, 2.0) * headroom,
            );
        }
    };
    balance(t, out_vol, in_vol, params)
}

fn balanced_crossfade_gains(progress: f32, params: CrossfadeParams) -> (f32, f32) {
    let t = progress.clamp(0.0, 1.0);
    let (out_vol, in_vol) = crossfade_values(t, &params);
    balance(t, out_vol, in_vol, params)
}

/// Gains for a balance of the two decks (`out_vol`, `in_vol` in 0..1): scaled so the pair's power
/// moves from A's gain to B's along the eased progress, with the overlap headroom dip on top.
/// The dip is shaped `sin^2(pi t)` so it, too, leaves and returns at rest.
fn balance(t: f32, out_vol: f32, in_vol: f32, params: CrossfadeParams) -> (f32, f32) {
    let outgoing_target = params.outgoing_gain.clamp(0.0, 2.0);
    let incoming_target = params.incoming_gain.clamp(0.0, 2.0);
    // f32 `cos(FRAC_PI_2)` is a hair below zero.
    let mut out_gain = out_vol.max(0.0) * outgoing_target;
    let mut in_gain = in_vol.max(0.0) * incoming_target;

    let current_power = out_gain * out_gain + in_gain * in_gain;
    if current_power > 1e-8 {
        let w = eased_progress(t);
        let target_power =
            outgoing_target * outgoing_target * (1.0 - w) + incoming_target * incoming_target * w;
        let power_scale = (target_power.max(0.0) / current_power).sqrt();
        let overlap_shape = (t * std::f32::consts::PI).sin().powi(2);
        let headroom_scale = 10f32.powf((params.overlap_headroom_db * overlap_shape) / 20.0);
        let scale = soft_power_cap(power_scale) * headroom_scale;
        out_gain *= scale;
        in_gain *= scale;
    }

    (out_gain, in_gain)
}

/// Same shaping as the Web `getCrossfadeValues`: shapes are exponents on each
/// side, and the constant-power curves are renormalised afterwards so an
/// asymmetric shape moves *when* each side is heard without a level dip.
/// `Linear` and `EqualPower` run on `eased_progress`; `SCurve` is smootherstep,
/// already at rest at both ends.
fn crossfade_values(progress: f32, params: &CrossfadeParams) -> (f32, f32) {
    use std::f32::consts::FRAC_PI_2;
    let t = progress.clamp(0.0, 1.0);
    // Each side from its own end: both warps are symmetric (`f(1 - t) = 1 - f(t)`), and taking
    // the outgoing side as `f(1 - t)` rather than `1 - f(t)` keeps its precision near zero, where
    // a shape exponent below 1 would otherwise turn f32 rounding steps into slope jumps.
    let (rest, done) = match params.curve {
        CrossfadeCurve::Linear | CrossfadeCurve::EqualPower => {
            (eased_progress(1.0 - t), eased_progress(t))
        }
        CrossfadeCurve::SCurve => (smootherstep(1.0 - t), smootherstep(t)),
    };
    let (mut out_vol, mut in_vol) = match params.curve {
        CrossfadeCurve::Linear => (rest, done),
        CrossfadeCurve::EqualPower | CrossfadeCurve::SCurve => {
            ((rest * FRAC_PI_2).sin(), (done * FRAC_PI_2).sin())
        }
    };

    // Unshaped fades are the common case; keep them free of powf and exact.
    if params.in_shape == 1.0 && params.out_shape == 1.0 {
        return (out_vol, in_vol);
    }
    // A negative base with a fractional exponent is NaN; keep the clamp even though the sides
    // are now computed as sines of non-negative angles.
    if params.out_shape != 1.0 {
        out_vol = out_vol.max(0.0).powf(params.out_shape);
    }
    if params.in_shape != 1.0 {
        in_vol = in_vol.max(0.0).powf(params.in_shape);
    }
    if params.curve != CrossfadeCurve::Linear {
        let power = out_vol * out_vol + in_vol * in_vol;
        if power > 1e-8 {
            let scale = power.sqrt().recip();
            out_vol *= scale;
            in_vol *= scale;
        }
    }
    (out_vol, in_vol)
}

#[cfg(test)]
#[path = "mixer/timing_tests.rs"]
mod timing_tests;

#[cfg(test)]
#[path = "mixer/perf_tests.rs"]
mod perf_tests;

#[cfg(test)]
#[path = "mixer/continuity_tests.rs"]
mod continuity_tests;

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
                position: None,
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
    fn fill_scaled_applies_gain_and_leaves_overs_for_the_limiter() {
        let (mut deck, _tx) = deck_from_blocks(vec![vec![0.5, -0.5, 2.0, -2.0]]);
        let mut dst = Vec::new();
        let mut consumed = 0;
        let written = deck.fill_scaled(&mut dst, 4, 0.75, &mut consumed);
        assert_eq!(written, 4);
        assert_eq!(consumed, 4);
        // Overs survive the mix: clipping them here would hide them from the
        // limiter, and the device callback clamps whatever is left.
        assert_eq!(dst, vec![0.375, -0.375, 1.5, -1.5]);
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
    fn clearing_a_deck_nobody_reads_makes_room_for_its_next_decoder() {
        // AutoMix's incoming deck is not read until its cue. A preload that filled it and was
        // then cancelled left its blocks queued; the next preload's decoder found the queue full
        // and waited until the cue drained it, leaving nothing to play there.
        let (tx, rx) = mpsc::sync_channel(4);
        let (recycle_tx, _recycle_rx) = mpsc::sync_channel(16);
        let block = |generation| DeckBlock {
            samples: vec![0.5, 0.5],
            generation,
            flush_epoch: generation,
            position: None,
        };
        for _ in 0..4 {
            tx.send(block(0)).unwrap();
        }
        let mut deck = DeckRuntime::new(
            rx,
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicBool::new(false)),
            recycle_tx,
            0.0,
        );

        deck.clear(1, 1);

        for _ in 0..4 {
            assert!(
                tx.try_send(block(1)).is_ok(),
                "stale blocks still fill the queue"
            );
        }
        assert_eq!(deck.current_samples(), &[0.5, 0.5]);
    }

    #[test]
    fn fill_scaled_skips_stale_generation_blocks() {
        let (tx, rx) = mpsc::sync_channel(16);
        let (recycle_tx, _recycle_rx) = mpsc::sync_channel(16);
        tx.send(DeckBlock {
            samples: vec![9.0, 9.0],
            generation: 0,
            flush_epoch: 0,
            position: None,
        })
        .unwrap();
        tx.send(DeckBlock {
            samples: vec![0.3, 0.4],
            generation: 1,
            flush_epoch: 0,
            position: None,
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
            ..Default::default()
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
                filters: None,
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

    #[test]
    fn every_fade_style_ends_where_completion_leaves_the_decks() {
        let near =
            |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-5 && (a.1 - b.1).abs() < 1e-5;
        for style in [
            FadeStyle::Blend,
            FadeStyle::Handoff(0.6),
            FadeStyle::BeatMix(0.5),
            FadeStyle::FilterCut,
        ] {
            let params = CrossfadeParams {
                style,
                incoming_gain: 0.9,
                outgoing_gain: 0.8,
                ..Default::default()
            };
            assert!(near(fade_gains(0.0, params), (0.8, 0.0)), "{style:?} start");
            assert!(near(fade_gains(1.0, params), (0.0, 0.9)), "{style:?} end");
        }
        let flat = CrossfadeParams {
            overlap_headroom_db: 0.0,
            ..Default::default()
        };
        let beat_mix = CrossfadeParams {
            style: FadeStyle::BeatMix(0.5),
            ..flat
        };
        let half = std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            near(fade_gains(0.5, beat_mix), (half, half)),
            "level with each other at the handoff, without stacking to +3 dB"
        );
        let cut = CrossfadeParams {
            style: FadeStyle::FilterCut,
            ..flat
        };
        assert!(
            near(fade_gains(0.49, cut), (1.0, 0.0)),
            "B waits for the middle"
        );
        assert_eq!(fade_gains(0.69, cut).0, 1.0, "A holds until 70%");
    }

    #[test]
    fn a_bass_swap_hands_the_low_band_over_and_leaves_the_highs() {
        let swap = BassSwap {
            start: 0.4,
            end: 0.6,
        };
        assert_eq!(swap.shelf_db(0.0), (0.0, BASS_SWAP_KILL_DB));
        assert_eq!(swap.shelf_db(1.0), (BASS_SWAP_KILL_DB, 0.0));
        // Steady-state level of a tone through the outgoing deck's shelf.
        let level = |hz: f32, progress: f32| {
            let rate = 48_000.0;
            let coeffs =
                BiquadCoeffs::low_shelf(rate, BASS_SWAP_HZ, 0.707, swap.shelf_db(progress).0);
            let mut state = [[[BiquadState::default(); 3]; BASS_SWAP_MAX_CHANNELS]; 2];
            let mut band = BiquadFilters::<1>::new(&mut state, [[coeffs], [coeffs]]);
            let mut peak = 0.0f32;
            for i in 0..48_000 {
                let x = (std::f32::consts::TAU * hz * i as f32 / rate).sin();
                let y = band.apply(0, 0, x);
                if i > 24_000 {
                    peak = peak.max(y.abs());
                }
            }
            20.0 * peak.log10()
        };
        assert!(
            level(50.0, 0.0).abs() < 0.1,
            "before the swap the bass is untouched"
        );
        assert!(level(50.0, 1.0) < -18.0, "after it the kick band is gone");
        assert!(level(4_000.0, 1.0).abs() < 0.5, "the highs never move");
    }

    #[test]
    fn every_curve_and_style_moves_without_corners() {
        // Each piece of a fade is smooth, so a corner can only sit where pieces meet: at the ends
        // and at a style's joins. Check the slope matches across each, and that each side moves
        // one way only.
        const D: f32 = 1e-4;
        let check = |name: &str, gains: &dyn Fn(f32) -> (f32, f32), joins: &[f32]| {
            for side in 0..2 {
                let at = |t: f32| {
                    let (out, inc) = gains(t);
                    if side == 0 {
                        out
                    } else {
                        inc
                    }
                };
                let slope = |from: f32| (at(from + D) - at(from)) / D;
                assert!(
                    slope(0.0).abs() < 0.05 && slope(1.0 - D).abs() < 0.05,
                    "{name} side {side} starts and ends at rest: {} {}",
                    slope(0.0),
                    slope(1.0 - D)
                );
                for &join in joins {
                    let step = slope(join) - slope(join - D);
                    assert!(
                        step.abs() < 0.05,
                        "{name} side {side} kinks at {join}: {step}"
                    );
                }
                let mut last = at(0.0);
                for i in 1..=2_000 {
                    let value = at(i as f32 / 2_000.0);
                    let moved = if side == 0 {
                        last - value
                    } else {
                        value - last
                    };
                    assert!(moved > -1e-6, "{name} side {side} turns back at {i}");
                    last = value;
                }
            }
        };
        // Equal gains, so the level path is flat and each side is monotone.
        let base = CrossfadeParams {
            outgoing_gain: 1.0,
            incoming_gain: 1.0,
            overlap_headroom_db: 0.0,
            ..Default::default()
        };
        let edges = [FADE_EDGE, 1.0 - FADE_EDGE];
        for curve in [
            CrossfadeCurve::Linear,
            CrossfadeCurve::EqualPower,
            CrossfadeCurve::SCurve,
        ] {
            for (in_shape, out_shape) in [(1.0, 1.0), (1.3, 0.7), (0.7, 1.3)] {
                let params = CrossfadeParams {
                    curve,
                    in_shape,
                    out_shape,
                    ..base
                };
                check(
                    &format!("{curve:?} {in_shape}/{out_shape}"),
                    &|t| fade_gains(t, params),
                    &edges,
                );
            }
        }
        for (style, joins) in [
            (FadeStyle::Handoff(0.6), vec![0.6]),
            (FadeStyle::Handoff(0.2), vec![0.2]),
            (FadeStyle::BeatMix(0.5), vec![0.5]),
            (FadeStyle::BeatMix(0.9), vec![0.9]),
            (FadeStyle::FilterCut, vec![0.5, 0.7]),
        ] {
            let params = CrossfadeParams { style, ..base };
            check(&format!("{style:?}"), &|t| fade_gains(t, params), &joins);
        }
        // Unequal gains and a headroom dip still leave and return at rest.
        let leveled = CrossfadeParams {
            outgoing_gain: 0.8,
            incoming_gain: 1.1,
            overlap_headroom_db: -1.2,
            ..Default::default()
        };
        for style in [
            FadeStyle::Blend,
            FadeStyle::Handoff(0.6),
            FadeStyle::BeatMix(0.5),
        ] {
            let params = CrossfadeParams { style, ..leveled };
            for side in 0..2 {
                let at = |t: f32| {
                    let g = fade_gains(t, params);
                    if side == 0 {
                        g.0
                    } else {
                        g.1
                    }
                };
                let slope_in = (at(D) - at(0.0)) / D;
                let slope_out = (at(1.0) - at(1.0 - D)) / D;
                assert!(
                    slope_in.abs() < 0.05 && slope_out.abs() < 0.05,
                    "{style:?} side {side}: {slope_in} {slope_out}"
                );
            }
        }
    }

    #[test]
    fn the_eased_progress_is_monotone_and_exact_at_its_ends() {
        assert_eq!(eased_progress(0.0), 0.0);
        assert_eq!(eased_progress(1.0), 1.0);
        assert!((eased_progress(0.5) - 0.5).abs() < 1e-6);
        let mut last = 0.0;
        for i in 1..=1000 {
            let w = eased_progress(i as f32 / 1000.0);
            assert!(w >= last, "monotone at {i}");
            last = w;
        }
    }

    /// Steady-state level in dB of a tone through one deck's filters for this segment.
    fn tone_level(filters: FadeFilters, deck: usize, hz: f32) -> f32 {
        let rate = 48_000.0;
        let mut state = [[[BiquadState::default(); 3]; BASS_SWAP_MAX_CHANNELS]; 2];
        let mut peak = 0.0f32;
        for i in 0..48_000 {
            let x = (std::f32::consts::TAU * hz * i as f32 / rate).sin();
            let y = match filters {
                FadeFilters::One(coeffs) => {
                    BiquadFilters::new(&mut state, coeffs).apply(deck, 0, x)
                }
                FadeFilters::Two(coeffs) => {
                    BiquadFilters::new(&mut state, coeffs).apply(deck, 0, x)
                }
                FadeFilters::Three(coeffs) => {
                    BiquadFilters::new(&mut state, coeffs).apply(deck, 0, x)
                }
            };
            if i > 24_000 {
                peak = peak.max(y.abs());
            }
        }
        20.0 * peak.log10()
    }

    #[test]
    fn a_band_morph_leans_each_deck_toward_the_other_and_settles() {
        let params = CrossfadeParams {
            morph: Some(BandMorph {
                db: [0.0, 4.0, -3.0],
            }),
            ..Default::default()
        };
        let at = |progress| deck_filters(48_000.0, params, DeckId::Primary, progress).unwrap();
        let (outgoing, incoming) = (0, 1);
        let near = |a: f32, b: f32| (a - b).abs() < 0.3;
        // A starts untouched and ends leaning toward B's mids and highs.
        assert!(near(tone_level(at(0.0), outgoing, 1_100.0), 0.0));
        assert!(near(tone_level(at(1.0), outgoing, 1_100.0), 4.0));
        assert!(near(tone_level(at(1.0), outgoing, 12_000.0), -3.0));
        // B starts leaning toward A's and ends as itself.
        assert!(near(tone_level(at(0.0), incoming, 1_100.0), -4.0));
        assert!(near(tone_level(at(0.0), incoming, 12_000.0), 3.0));
        assert!(near(tone_level(at(1.0), incoming, 1_100.0), 0.0));
        assert!(near(tone_level(at(1.0), incoming, 12_000.0), 0.0));
        // The band that does not move is left alone throughout.
        assert!(near(tone_level(at(0.5), outgoing, 60.0), 0.0));
        // Swapped decks swap the roles.
        let swapped = deck_filters(48_000.0, params, DeckId::Secondary, 1.0).unwrap();
        assert!(near(tone_level(swapped, 1, 1_100.0), 4.0));
    }

    #[test]
    fn a_staged_entry_holds_b_thin_until_the_crossover_then_hands_the_highs_over() {
        let params = CrossfadeParams {
            bass_swap: Some(BassSwap {
                start: 0.4,
                end: 0.5,
            }),
            stage: Some(BandStage { crossover: 0.5 }),
            ..Default::default()
        };
        let at = |progress| deck_filters(48_000.0, params, DeckId::Primary, progress).unwrap();
        let (a, b) = (0, 1);
        let near = |got: f32, want: f32| (got - want).abs() < 0.5;
        // B arrives as a body: no highs, no lows; A is untouched.
        assert!(near(tone_level(at(0.0), b, 14_000.0), STAGE_HIGH_DB));
        assert!(near(tone_level(at(0.0), b, 40.0), BASS_SWAP_KILL_DB));
        assert!(near(tone_level(at(0.0), b, 700.0), 0.0));
        assert!(near(tone_level(at(0.0), a, 14_000.0), 0.0));
        // Still without highs at the crossover, though the bass has changed hands.
        assert!(near(tone_level(at(0.5), b, 14_000.0), STAGE_HIGH_DB));
        assert!(near(tone_level(at(0.5), b, 40.0), 0.0));
        // The highs have changed hands one span later.
        let handed = at(0.5 + STAGE_HIGH_SPAN);
        assert!(near(tone_level(handed, b, 14_000.0), 0.0));
        assert!(near(tone_level(handed, a, 14_000.0), STAGE_HIGH_DB));
        // A crossover too late for the span still lets B finish as itself.
        let late = CrossfadeParams {
            stage: Some(BandStage { crossover: 0.98 }),
            ..params
        };
        let end = deck_filters(48_000.0, late, DeckId::Primary, 1.0).unwrap();
        assert!(near(tone_level(end, b, 14_000.0), 0.0));
        // A handoff past 85% (A still singing) keeps A's highs until it.
        let held = CrossfadeParams {
            stage: Some(BandStage { crossover: 0.9 }),
            ..params
        };
        let before = deck_filters(48_000.0, held, DeckId::Primary, 0.9).unwrap();
        assert!(near(tone_level(before, a, 14_000.0), 0.0));
        // On top of a morph the two add in dB.
        let morphed = CrossfadeParams {
            morph: Some(BandMorph {
                db: [0.0, 0.0, -3.0],
            }),
            ..params
        };
        let end = deck_filters(48_000.0, morphed, DeckId::Primary, 1.0).unwrap();
        assert!(near(tone_level(end, a, 14_000.0), STAGE_HIGH_DB - 3.0));
        // FilterCut has its own sweep.
        let cut = CrossfadeParams {
            style: FadeStyle::FilterCut,
            ..params
        };
        assert!(matches!(
            deck_filters(48_000.0, cut, DeckId::Primary, 0.0),
            Some(FadeFilters::One(_))
        ));
    }

    #[test]
    fn an_echo_out_repeats_at_its_delay_and_dies_away() {
        let rate = 48_000;
        let mut line = echo_line(rate, 1);
        let echo = EchoOut {
            delay: 0.25,
            feedback: 0.4,
            cutoff_hz: 4_000.0,
            wet: 0.5,
            duration: 2.0,
        };
        let mut runtime = EchoRuntime::new(echo, rate, 1, &mut line).unwrap();
        // An impulse in; the repeats out, with the wet envelope divided back out.
        let mut heard = Vec::new();
        for n in 0..2 * rate as usize {
            let gain = runtime.gain;
            let out = runtime.tap(&mut line, 0, if n == 0 { 1.0 } else { 0.0 });
            heard.push(if gain > 0.0 { out / gain } else { 0.0 });
        }
        let delay = rate as usize / 4;
        assert!(
            heard[..delay].iter().all(|x| *x == 0.0),
            "nothing before the first repeat"
        );
        for k in 1..=4 {
            let repeat: f32 = heard[k * delay..k * delay + 400].iter().sum();
            let want = 0.4f32.powi(k as i32 - 1);
            assert!(
                (repeat - want).abs() < want * 0.02,
                "repeat {k}: {repeat} vs {want}"
            );
        }
        assert!(runtime.done());
        assert!((runtime.gain - ECHO_FLOOR).abs() < ECHO_FLOOR * 0.05);

        // Released mid-ring, it reaches the floor in the frames given and no later.
        let mut runtime = EchoRuntime::new(echo, rate, 1, &mut line).unwrap();
        for _ in 0..rate / 2 {
            runtime.tap(&mut line, 0, 0.1);
        }
        runtime.release(2_400);
        for _ in 0..2_400 {
            assert!(!runtime.done());
            runtime.tap(&mut line, 0, 0.0);
        }
        assert!(runtime.done());
        assert!((runtime.gain - ECHO_FLOOR).abs() < ECHO_FLOOR * 0.05);
    }

    #[test]
    fn a_single_filter_fade_stays_a_single_stage() {
        let swap_only = CrossfadeParams {
            bass_swap: Some(BassSwap::MIDDLE),
            ..Default::default()
        };
        let cut = CrossfadeParams {
            style: FadeStyle::FilterCut,
            morph: Some(BandMorph { db: [1.0; 3] }),
            ..Default::default()
        };
        let staged_only = CrossfadeParams {
            stage: Some(BandStage { crossover: 0.5 }),
            ..Default::default()
        };
        for params in [swap_only, cut, staged_only] {
            assert!(matches!(
                deck_filters(48_000.0, params, DeckId::Primary, 0.5),
                Some(FadeFilters::One(_))
            ));
        }
        // Before the crossover A's chain is the pass-through, and is skipped.
        let Some(FadeFilters::One(coeffs)) =
            deck_filters(48_000.0, staged_only, DeckId::Primary, 0.2)
        else {
            panic!("one stage");
        };
        let mut state = Default::default();
        assert_eq!(BiquadFilters::new(&mut state, coeffs).active, [false, true]);
        // Only the bands that move are filtered.
        let two_bands = CrossfadeParams {
            morph: Some(BandMorph {
                db: [0.0, 4.0, -3.0],
            }),
            ..Default::default()
        };
        assert!(matches!(
            deck_filters(48_000.0, two_bands, DeckId::Primary, 0.5),
            Some(FadeFilters::Two(_))
        ));
        assert!(deck_filters(48_000.0, CrossfadeParams::default(), DeckId::Primary, 0.5).is_none());
    }

    #[test]
    fn unit_shapes_leave_every_curve_untouched() {
        for curve in [
            CrossfadeCurve::Linear,
            CrossfadeCurve::EqualPower,
            CrossfadeCurve::SCurve,
        ] {
            let params = CrossfadeParams {
                curve,
                ..Default::default()
            };
            for step in 0..=20 {
                let t = step as f32 / 20.0;
                let (out_vol, in_vol) = crossfade_values(t, &params);
                let quarter = std::f32::consts::FRAC_PI_2;
                let (expected_out, expected_in) = match curve {
                    CrossfadeCurve::Linear => (eased_progress(1.0 - t), eased_progress(t)),
                    CrossfadeCurve::EqualPower => (
                        (eased_progress(1.0 - t) * quarter).sin(),
                        (eased_progress(t) * quarter).sin(),
                    ),
                    CrossfadeCurve::SCurve => (
                        (smootherstep(1.0 - t) * quarter).sin(),
                        (smootherstep(t) * quarter).sin(),
                    ),
                };
                assert_eq!(out_vol.to_bits(), expected_out.to_bits());
                assert_eq!(in_vol.to_bits(), expected_in.to_bits());
            }
        }
    }

    #[test]
    fn shaped_constant_power_curves_keep_their_power_and_endpoints() {
        let held_back = CrossfadeParams {
            curve: CrossfadeCurve::EqualPower,
            in_shape: 1.2,
            ..Default::default()
        };
        let plain = CrossfadeParams::default();

        for step in 0..=20 {
            let t = step as f32 / 20.0;
            let (out_vol, in_vol) = crossfade_values(t, &held_back);
            assert!((out_vol * out_vol + in_vol * in_vol - 1.0).abs() < 1e-4);
        }
        assert_eq!(crossfade_values(0.0, &held_back), (1.0, 0.0));
        let (end_out, end_in) = crossfade_values(1.0, &held_back);
        assert!(end_out.abs() < 1e-6 && (end_in - 1.0).abs() < 1e-6);
        let cleared = CrossfadeParams {
            out_shape: 1.15,
            ..held_back
        };
        let (end_out, _) = crossfade_values(1.0, &cleared);
        assert!(
            !end_out.is_nan(),
            "a shaped outgoing side must not go NaN at the end"
        );
        // Held back: quieter than the unshaped incoming half way through.
        assert!(crossfade_values(0.5, &held_back).1 < crossfade_values(0.5, &plain).1);
    }
}
