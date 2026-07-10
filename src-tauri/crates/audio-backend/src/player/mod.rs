/// AMLL-style message-driven audio player.
///
/// Architecture (mirrors `amll-player-core`):
/// - `Player` — public API: sends `AudioThreadMessage` to the internal player thread.
/// - `PlayerHandle` — cloneable handle for sending messages from anywhere.
/// - `AudioPlayer` — internal event loop that processes messages + emits events.
///
/// Message flow:  frontend → WebSocket → Player::send_msg() → AudioPlayer → decoder/output
/// Event flow:   AudioPlayer → callback → WebSocket broadcast → frontend
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::sync::RwLock as TokioRwLock;
use tracing::{info, warn};

use crate::analysis::{self, AnalysisCommand};
use crate::decoder::{self, PlaybackSink};
use crate::error::{AudioError, AudioResult};
use crate::output::{self, LowLatencyOutput};
use crate::types::*;

mod api;
mod automix;
mod clock;
mod mixer;
mod platform;
pub mod queue;

#[allow(unused_imports)]
pub use api::{EventBuffer, Player, PlayerHandle, PlayerShared};

use api::{join_thread_async, SeekRequest};
use automix::AutoMixManager;
use clock::PlayerClock;
use mixer::{CrossfadeParams, DeckId, DeckMixer};
use platform::{
    output_refresh_target, output_target_for_source, output_target_matches, seek_prebuffer_samples,
    start_prebuffer_samples, SEEK_PREBUFFER_WAIT_MS, START_PREBUFFER_WAIT_MS,
};
use queue::PlaybackQueue;

// ═══════════════════════════════════════════════════════════════════
//  Internal AudioPlayer
// ═══════════════════════════════════════════════════════════════════

struct AudioPlayer {
    // Channels
    msg_receiver: mpsc::UnboundedReceiver<AudioThreadEventMessage<AudioThreadMessage>>,
    seek_rx: mpsc::UnboundedReceiver<SeekRequest>,
    evt_sender: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
    // Self-message channel: lets `run()` re-enter `process_message` for
    // auto-advance, matching the AMLL reference's NextSongGapless pattern.
    self_msg_tx: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadMessage>>,
    self_msg_rx: mpsc::UnboundedReceiver<AudioThreadEventMessage<AudioThreadMessage>>,

    // CPAL playback. Decoding runs on a worker thread and pushes PCM blocks
    // into the deck mixer; the mixer pushes mixed PCM blocks to this output queue.
    output: LowLatencyOutput,
    output_selector: output::OutputDeviceSelector,
    deck_mixer: DeckMixer,
    active_deck: DeckId,
    /// Persistent per-track loudness-normalization gain currently applied to the
    /// active deck (the AutoMix `volume_norm` adjustment, range ~[0.1, 2.0]).
    /// Master volume rides separately on the output writer (`set_volume`), so
    /// this must survive crossfades, cancels and output rebuilds unchanged —
    /// otherwise the deck gain snaps back to 1.0 and the track jumps in level.
    /// Mirrors the mixer's active-deck gain during steady-state playback.
    active_norm_gain: f32,
    dsp_config: DspConfig,
    automix: AutoMixManager,
    volume: f64,
    /// What the user wants the sink to be doing. Used so auto-advance and
    /// seek transitions preserve the playing/paused state even though the
    /// underlying sink momentarily becomes empty/stopped during the swap.
    playback_intent: PlaybackIntent,
    clock: Arc<parking_lot::Mutex<PlayerClock>>,

    // Current track
    current_file_path: Option<String>,
    /// Resolved local path: same as `current_file_path` for local files, or
    /// the temp-file path created when streaming a remote URL.
    current_local_path: Option<PathBuf>,
    /// RAII guard for the downloaded temp file — dropped when a new track
    /// loads, which deletes the temp file from disk.
    current_temp_file: Option<tempfile::TempPath>,
    current_decoder_handle: Option<decoder::DecoderHandle>,
    pending_seek: Option<SeekRequest>,
    secondary_decoder_handle: Option<decoder::DecoderHandle>,
    secondary_temp_file: Option<tempfile::TempPath>,
    secondary_local_path: Option<PathBuf>,
    secondary_song: Option<SongData>,
    secondary_duration: f64,
    /// Normalization gain of the preloaded/incoming (secondary) deck's track.
    /// Captured at crossfade start and promoted to `active_norm_gain` when the
    /// crossfade completes, so the incoming level is continuous into steady
    /// state and into the next crossfade's outgoing side.
    secondary_norm_gain: f32,
    secondary_display_info: Option<DisplayAudioInfo>,
    secondary_quality: Option<AudioQuality>,
    secondary_playback_id: Option<u64>,
    decoder_event_tx: mpsc::UnboundedSender<decoder::DecoderEvent>,
    decoder_event_rx: mpsc::UnboundedReceiver<decoder::DecoderEvent>,
    automix_prepare_tx: mpsc::UnboundedSender<automix::AutoMixPrepareResult>,
    automix_prepare_rx: mpsc::UnboundedReceiver<automix::AutoMixPrepareResult>,
    output_refresh_tx: mpsc::UnboundedSender<OutputRefreshEvent>,
    output_refresh_rx: mpsc::UnboundedReceiver<OutputRefreshEvent>,
    output_refresh_pending: bool,
    output_refresh_generation: u64,
    output_epoch: u64,
    output_refresh_dirty: bool,
    output_refresh_dirty_force: bool,
    output_refresh_dirty_rebuild_chain: bool,
    output_refresh_failures: u8,
    output_refresh_backoff_until: Option<std::time::Instant>,
    output_health_last_samples: u64,
    output_health_stalled_ticks: u8,
    last_output_error: Option<String>,
    decoder_playback_id: u64,
    native_crossfade_generation: u64,
    native_crossfade_active: bool,
    native_crossfade_transition_id: Option<u64>,
    automix_prepare_generation: u64,

    // Playlist
    playback_queue: PlaybackQueue,
    playlist: Vec<SongData>,
    playlist_inited: bool,
    current_play_index: usize,
    current_song: Option<SongData>,

    // Shared state snapshots
    current_audio_info: Arc<TokioRwLock<DisplayAudioInfo>>,
    current_position: Arc<TokioRwLock<f64>>,
    current_audio_quality: Arc<TokioRwLock<AudioQuality>>,

    // Background tasks
    tasks: Vec<tokio::task::JoinHandle<()>>,

    /// Sender to the dedicated `audio-analysis` OS thread (see
    /// `crate::analysis`). Cloned into each `FFTFeedSource` so the audio
    /// callback thread can push interleaved PCM via the channel — replaces
    /// the previous `Arc<ParkingLotRwLock<AudioProcessor>>` lock pattern.
    analysis_tx: std_mpsc::Sender<AnalysisCommand>,
    /// JoinHandle for the analysis thread. Held so `Drop for AudioPlayer`
    /// can join after dropping the `Sender` (which closes the channel and
    /// signals the thread to exit). Wrapped in `Option` so `take()` works
    /// in `Drop`.
    analysis_thread: Option<std::thread::JoinHandle<()>>,
    analysis_enabled: Arc<AtomicBool>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum PlaybackIntent {
    Playing,
    Paused,
}

enum OutputRefreshEvent {
    Unchanged {
        generation: u64,
        output_epoch: u64,
    },
    Stale {
        generation: u64,
        output_epoch: u64,
    },
    Opened {
        generation: u64,
        output_epoch: u64,
        output: LowLatencyOutput,
        force_replace: bool,
        rebuild_chain: bool,
    },
    DecoderReady {
        generation: u64,
        output_epoch: u64,
        playback_id: u64,
        position: f64,
        output: LowLatencyOutput,
        deck_mixer: DeckMixer,
        result: AudioResult<decoder::DecoderHandle>,
    },
    Failed {
        generation: u64,
        output_epoch: u64,
        error: String,
    },
}

impl OutputRefreshEvent {
    fn generation(&self) -> u64 {
        match self {
            Self::Unchanged { generation, .. }
            | Self::Stale { generation, .. }
            | Self::Opened { generation, .. }
            | Self::DecoderReady { generation, .. }
            | Self::Failed { generation, .. } => *generation,
        }
    }

    fn output_epoch(&self) -> u64 {
        match self {
            Self::Unchanged { output_epoch, .. }
            | Self::Stale { output_epoch, .. }
            | Self::Opened { output_epoch, .. }
            | Self::DecoderReady { output_epoch, .. }
            | Self::Failed { output_epoch, .. } => *output_epoch,
        }
    }
}

impl AudioPlayer {
    async fn new(
        msg_receiver: mpsc::UnboundedReceiver<AudioThreadEventMessage<AudioThreadMessage>>,
        seek_rx: mpsc::UnboundedReceiver<SeekRequest>,
        evt_sender: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
    ) -> AudioResult<Self> {
        let output_selector = output::OutputDeviceSelector::Default;
        let output =
            output::open_output(output_selector.clone(), None).map_err(AudioError::Output)?;
        output.writer().set_paused(true);
        let dsp_config = DspConfig::default();
        let deck_mixer = DeckMixer::new(
            output.writer(),
            output.config().channels,
            output.config().sample_rate,
            &dsp_config,
        );
        let automix = AutoMixManager::new();
        let clock = Arc::new(parking_lot::Mutex::new(PlayerClock::new()));
        {
            let writer = output.writer();
            let config = output.config();
            clock.lock().set_render_clock(
                writer.render_clock(),
                config.sample_rate,
                config.channels,
            );
        }

        info!("音频输出设备 准备就绪");

        let current_audio_info = Arc::new(TokioRwLock::new(DisplayAudioInfo::default()));
        let current_position = Arc::new(TokioRwLock::new(0.0));
        let current_audio_quality = Arc::new(TokioRwLock::new(AudioQuality::default()));

        // Dedicated `audio-analysis` OS thread owns the `AudioProcessor` so
        // FFT work doesn't compete with the player's `current_thread` tokio
        // runtime. PCM flows in via the returned `Sender`; FFT/LowFreq events
        // are emitted through the existing `evt_sender` (multi-`Send`).
        let (analysis_tx, analysis_thread) = analysis::spawn_analysis_thread(evt_sender.clone())
            .map_err(|e| AudioError::ThreadError(format!("spawn audio-analysis thread: {e}")))?;
        let analysis_thread = Some(analysis_thread);
        let analysis_enabled = Arc::new(AtomicBool::new(true));

        let mut tasks = Vec::new();

        // ── Position tracking task ───────────────────────────────────
        //
        // The frontend extrapolates intermediate position locally from the
        // same clock anchor. Rust publishes immediate anchors on play/pause/
        // seek and a low-rate heartbeat for reconciliation.
        let position_writer = current_position.clone();
        let clock_reader = Arc::clone(&clock);
        let emitter_pos = EventEmitter::new(evt_sender.clone());

        tasks.push(tokio::task::spawn(async move {
            let mut time_it = tokio::time::interval(Duration::from_secs(1));
            time_it.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                time_it.tick().await;
                let (is_playing, current_pos) = {
                    let clock = clock_reader.lock();
                    (clock.is_playing(), clock.position())
                };
                if is_playing {
                    *position_writer.write().await = current_pos;
                    let _ = emitter_pos
                        .emit(AudioThreadEvent::PlayPosition {
                            position: current_pos,
                        })
                        .await;
                }
            }
        }));

        // ── FFT + lowFreq broadcast ──────────────────────────────────
        //
        // Moved out of this runtime entirely — see `crate::analysis`. The
        // dedicated OS thread owns the `AudioProcessor`, receives PCM via
        // `analysis_tx`, and emits `FFTData` + `LowFrequencyVolume` events
        // through the same `evt_sender` we use here.

        let (self_msg_tx, self_msg_rx) = mpsc::unbounded_channel();
        let (decoder_event_tx, decoder_event_rx) = mpsc::unbounded_channel();
        let (automix_prepare_tx, automix_prepare_rx) = mpsc::unbounded_channel();
        let (output_refresh_tx, output_refresh_rx) = mpsc::unbounded_channel();

        Ok(Self {
            msg_receiver,
            seek_rx,
            evt_sender,
            self_msg_tx,
            self_msg_rx,
            output,
            output_selector,
            deck_mixer,
            active_deck: DeckId::Primary,
            active_norm_gain: 1.0,
            dsp_config,
            automix,
            volume: 1.0,
            playback_intent: PlaybackIntent::Paused,
            clock,
            current_file_path: None,
            current_local_path: None,
            current_temp_file: None,
            current_decoder_handle: None,
            pending_seek: None,
            secondary_decoder_handle: None,
            secondary_temp_file: None,
            secondary_local_path: None,
            secondary_song: None,
            secondary_duration: 0.0,
            secondary_norm_gain: 1.0,
            secondary_display_info: None,
            secondary_quality: None,
            secondary_playback_id: None,
            decoder_event_tx,
            decoder_event_rx,
            automix_prepare_tx,
            automix_prepare_rx,
            output_refresh_tx,
            output_refresh_rx,
            output_refresh_pending: false,
            output_refresh_generation: 0,
            output_epoch: 0,
            output_refresh_dirty: false,
            output_refresh_dirty_force: false,
            output_refresh_dirty_rebuild_chain: false,
            output_refresh_failures: 0,
            output_refresh_backoff_until: None,
            output_health_last_samples: 0,
            output_health_stalled_ticks: 0,
            last_output_error: None,
            decoder_playback_id: 0,
            native_crossfade_generation: 0,
            native_crossfade_active: false,
            native_crossfade_transition_id: None,
            automix_prepare_generation: 0,
            playback_queue: PlaybackQueue::new(),
            playlist: Vec::new(),
            playlist_inited: false,
            current_play_index: 0,
            current_song: None,
            current_audio_info,
            current_position,
            current_audio_quality,
            tasks,
            analysis_tx,
            analysis_thread,
            analysis_enabled,
        })
    }

    fn emitter(&self) -> EventEmitter {
        EventEmitter::new(self.evt_sender.clone())
    }

    fn ensure_output_for_source(&mut self, audio_info: &AudioInfo) -> anyhow::Result<bool> {
        let target = output_target_for_source(audio_info);
        if self.output.selector() == &self.output_selector
            && output_target_matches(self.output.target(), target)
            && !self.output.has_failed()
        {
            return Ok(false);
        }

        self.cancel_pending_output_refresh();
        let output = output::open_output(self.output_selector.clone(), target)
            .map_err(AudioError::Output)?;
        let changed =
            output.config() != self.output.config() || output.device() != self.output.device();
        self.install_output(output);
        Ok(changed)
    }

    fn cancel_pending_output_refresh(&mut self) {
        self.output_refresh_generation = self.output_refresh_generation.wrapping_add(1);
        self.output_refresh_pending = false;
        self.output_refresh_dirty = false;
        self.output_refresh_dirty_force = false;
        self.output_refresh_dirty_rebuild_chain = false;
    }

    fn mark_output_chain_committed(&mut self) {
        self.output_epoch = self.output_epoch.wrapping_add(1);
        self.reset_output_refresh_backoff();
    }

    fn install_output(&mut self, output: LowLatencyOutput) {
        output.writer().set_volume(self.volume as f32);
        output
            .writer()
            .set_paused(self.playback_intent == PlaybackIntent::Paused);
        self.output = output;
        self.mark_output_chain_committed();
        {
            let writer = self.output.writer();
            let config = self.output.config();
            self.clock.lock().set_render_clock(
                writer.render_clock(),
                config.sample_rate,
                config.channels,
            );
        }
        self.deck_mixer = DeckMixer::new(
            self.output.writer(),
            self.output.config().channels,
            self.output.config().sample_rate,
            &self.dsp_config,
        );
        self.active_deck = DeckId::Primary;
        self.secondary_playback_id = None;
        self.native_crossfade_generation = self.native_crossfade_generation.wrapping_add(1);
        self.native_crossfade_active = false;
        self.native_crossfade_transition_id = None;
        self.reset_output_health();
    }

    fn replace_output_stream(&mut self, output: LowLatencyOutput) -> Result<(), LowLatencyOutput> {
        output.writer().set_volume(0.0);
        output.writer().set_paused(true);
        let writer = output.writer();
        let config = output.config();
        if !self.deck_mixer.replace_output(writer.clone()) {
            return Err(output);
        }
        self.output = output;
        self.mark_output_chain_committed();
        self.clock.lock().set_render_clock(
            writer.render_clock(),
            config.sample_rate,
            config.channels,
        );
        self.output.writer().set_volume(self.volume as f32);
        self.output
            .writer()
            .set_paused(self.playback_intent == PlaybackIntent::Paused);
        self.reset_output_health();
        Ok(())
    }

    async fn abandon_current_mixer_for_output_rebuild(&mut self) {
        self.cancel_native_automix_runtime().await;
        self.automix_prepare_generation = self.automix_prepare_generation.wrapping_add(1);
        let events = self.automix.cancel(self.current_play_index);
        self.emit_many(events).await;

        if let Some(handle) = self.current_decoder_handle.take() {
            handle.stop();
        }
        if let Some(handle) = self.secondary_decoder_handle.take() {
            handle.stop();
        }
        self.secondary_local_path = None;
        self.secondary_temp_file = None;
        self.secondary_song = None;
        self.secondary_duration = 0.0;
        self.secondary_display_info = None;
        self.secondary_quality = None;
        self.secondary_playback_id = None;

        self.deck_mixer = DeckMixer::new(
            self.output.writer(),
            self.output.config().channels,
            self.output.config().sample_rate,
            &self.dsp_config,
        );
        self.active_deck = DeckId::Primary;
        // The current track survives the output rebuild (it is re-decoded into the
        // fresh Primary deck), so carry its normalization gain across instead of
        // resetting to 1.0.
        self.deck_mixer
            .set_deck_gain(DeckId::Primary, self.active_norm_gain);
        self.deck_mixer.set_deck_gain(DeckId::Secondary, 0.0);
        self.secondary_norm_gain = 1.0;
        self.native_crossfade_generation = self.native_crossfade_generation.wrapping_add(1);
        self.native_crossfade_active = false;
        self.native_crossfade_transition_id = None;
        self.output_epoch = self.output_epoch.wrapping_add(1);
        self.reset_output_health();
    }

    fn reset_output_health(&mut self) {
        self.output_health_last_samples = self.output.writer().render_clock().rendered_samples();
        self.output_health_stalled_ticks = 0;
    }

    fn reset_output_refresh_backoff(&mut self) {
        self.output_refresh_failures = 0;
        self.output_refresh_backoff_until = None;
    }

    fn record_output_refresh_failure(&mut self) {
        self.output_refresh_failures = self.output_refresh_failures.saturating_add(1).min(6);
        let exponent = self.output_refresh_failures.saturating_sub(1) as u32;
        let delay_ms = 250u64.saturating_mul(1u64 << exponent).min(4_000);
        self.output_refresh_backoff_until =
            Some(std::time::Instant::now() + Duration::from_millis(delay_ms));
    }

    fn output_render_stalled(&mut self) -> bool {
        let writer = self.output.writer();
        let rendered_samples = writer.render_clock().rendered_samples();
        let queued_samples = self.deck_mixer.queued_samples();
        if self.playback_intent != PlaybackIntent::Playing
            || self.current_song.is_none()
            || self.current_decoder_handle.is_none()
        {
            self.output_health_last_samples = rendered_samples;
            self.output_health_stalled_ticks = 0;
            return false;
        }

        if rendered_samples != self.output_health_last_samples {
            self.output_health_last_samples = rendered_samples;
            self.output_health_stalled_ticks = 0;
            return false;
        }

        self.output_health_stalled_ticks = self.output_health_stalled_ticks.saturating_add(1);
        let stalled_tick_limit = if queued_samples == 0 { 30 } else { 15 };
        self.output_health_stalled_ticks >= stalled_tick_limit
    }

    fn request_output_refresh(&mut self, force_replace: bool, rebuild_chain: bool) {
        if self.output_refresh_pending {
            self.output_refresh_dirty = true;
            self.output_refresh_dirty_force |= force_replace;
            self.output_refresh_dirty_rebuild_chain |= rebuild_chain;
            return;
        }
        if let Some(backoff_until) = self.output_refresh_backoff_until {
            if std::time::Instant::now() < backoff_until {
                return;
            }
            self.output_refresh_backoff_until = None;
        }
        self.output_refresh_pending = true;
        self.output_refresh_generation = self.output_refresh_generation.wrapping_add(1);

        let generation = self.output_refresh_generation;
        let output_epoch = self.output_epoch;
        let selector = self.output_selector.clone();
        let current_device = self.output.device().clone();
        let force_replace = force_replace || self.output.has_failed();
        let rebuild_chain = rebuild_chain && self.current_song.is_some();
        let target = output_refresh_target(self.output.config());
        let tx = self.output_refresh_tx.clone();
        tokio::task::spawn_blocking(move || {
            let opened_event =
                |output: LowLatencyOutput| match output::selected_output_device_key(&selector) {
                    Ok(selected_device) if selected_device == output.device().clone() => {
                        OutputRefreshEvent::Opened {
                            generation,
                            output_epoch,
                            output,
                            force_replace,
                            rebuild_chain,
                        }
                    }
                    Ok(_) => OutputRefreshEvent::Stale {
                        generation,
                        output_epoch,
                    },
                    Err(error) => OutputRefreshEvent::Failed {
                        generation,
                        output_epoch,
                        error,
                    },
                };

            let event = if force_replace || rebuild_chain {
                match output::open_output(selector.clone(), target) {
                    Ok(output) => opened_event(output),
                    Err(error) => OutputRefreshEvent::Failed {
                        generation,
                        output_epoch,
                        error,
                    },
                }
            } else {
                match output::selected_output_device_key(&selector) {
                    Ok(selected_device) if selected_device == current_device => {
                        OutputRefreshEvent::Unchanged {
                            generation,
                            output_epoch,
                        }
                    }
                    Ok(_) => match output::open_output(selector.clone(), target) {
                        Ok(output) => opened_event(output),
                        Err(error) => OutputRefreshEvent::Failed {
                            generation,
                            output_epoch,
                            error,
                        },
                    },
                    Err(error) => OutputRefreshEvent::Failed {
                        generation,
                        output_epoch,
                        error,
                    },
                }
            };
            let _ = tx.send(event);
        });
    }

    fn complete_output_refresh(&mut self) {
        self.output_refresh_pending = false;
        if self.output_refresh_dirty {
            let force_replace = self.output_refresh_dirty_force;
            let rebuild_chain = self.output_refresh_dirty_rebuild_chain;
            self.output_refresh_dirty = false;
            self.output_refresh_dirty_force = false;
            self.output_refresh_dirty_rebuild_chain = false;
            self.request_output_refresh(force_replace, rebuild_chain);
        }
    }

    async fn emit_output_changed(&self, output: &LowLatencyOutput) {
        let config = output.config();
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::AudioOutputChanged {
                device_name: output.device().name().to_string(),
                is_default: output.selector().is_default(),
                channels: config.channels,
                sample_rate: config.sample_rate,
                sample_format: format!("{:?}", config.sample_format),
            })
            .await;
    }

    async fn emit_output_error_once(&mut self, error: String) {
        if self.last_output_error.as_deref() == Some(error.as_str()) {
            return;
        }
        self.last_output_error = Some(error.clone());
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::AudioOutputError {
                error,
                recoverable: true,
            })
            .await;
    }

    fn clear_output_error(&mut self) {
        self.last_output_error = None;
    }

    async fn handle_output_refresh_event(&mut self, event: OutputRefreshEvent) {
        if event.generation() != self.output_refresh_generation
            || event.output_epoch() != self.output_epoch
        {
            return;
        }

        match event {
            OutputRefreshEvent::Unchanged { .. } => {
                self.clear_output_error();
                self.reset_output_refresh_backoff();
                self.reset_output_health();
                self.complete_output_refresh();
            }
            OutputRefreshEvent::Stale { .. } => {
                warn!("刷新音频输出设备结果已过期，将重试");
                self.output_refresh_dirty = true;
                self.complete_output_refresh();
            }
            OutputRefreshEvent::Failed { error, .. } => {
                warn!("刷新音频输出设备失败：{error}");
                self.record_output_refresh_failure();
                self.complete_output_refresh();
                self.emit_output_error_once(error).await;
            }
            OutputRefreshEvent::Opened {
                generation,
                output_epoch,
                output,
                force_replace,
                rebuild_chain,
            } => {
                if output.device() == self.output.device()
                    && output.config() == self.output.config()
                    && !force_replace
                    && !rebuild_chain
                {
                    self.complete_output_refresh();
                    return;
                }

                let old_device = self.output.device().name().to_string();
                let new_device = output.device().name().to_string();
                let position = self.output_rebuild_position().await;
                let is_playing = self.playback_intent == PlaybackIntent::Playing;
                let position = self.clock.lock().set_anchor(is_playing, position);
                self.clear_output_error();
                info!("音频输出设备变化：{old_device} -> {new_device}");
                if output_audio_layout_matches(self.output.config(), output.config())
                    && !rebuild_chain
                {
                    match self.replace_output_stream(output) {
                        Ok(()) => {
                            self.complete_output_refresh();
                            self.emit_output_changed(&self.output).await;
                            self.publish_position_anchor(is_playing, position).await;
                            if is_playing {
                                let _ = self
                                    .emitter()
                                    .emit(AudioThreadEvent::PlayStatus { is_playing: true })
                                    .await;
                            }
                            self.sync_ui().await;
                        }
                        Err(output) => {
                            warn!("替换音频输出 writer 超时，将丢弃当前 mixer 并重建播放链路");
                            self.abandon_current_mixer_for_output_rebuild().await;
                            if self.current_song.is_some() {
                                self.spawn_reconfigured_decoder(
                                    generation,
                                    self.output_epoch,
                                    output,
                                    position,
                                );
                            } else {
                                self.install_output(output);
                                self.complete_output_refresh();
                                self.emit_output_changed(&self.output).await;
                                self.sync_ui().await;
                            }
                        }
                    }
                    return;
                }

                let old_config = self.output.config();
                let new_config = output.config();
                if rebuild_chain {
                    warn!(
                        "音频输出渲染停滞，重建当前播放链路：config={:?}",
                        old_config
                    );
                } else {
                    warn!(
                        "音频输出 layout 变化，必须重建解码器：old={:?} new={:?}",
                        old_config, new_config
                    );
                }

                if self.current_song.is_some() {
                    self.cancel_native_automix_runtime().await;
                    self.automix_prepare_generation =
                        self.automix_prepare_generation.wrapping_add(1);
                    let events = self.automix.cancel(self.current_play_index);
                    self.emit_many(events).await;
                    self.secondary_local_path = None;
                    self.secondary_temp_file = None;
                    self.secondary_song = None;
                    self.secondary_duration = 0.0;
                    self.secondary_display_info = None;
                    self.secondary_quality = None;
                    self.secondary_playback_id = None;
                    self.spawn_reconfigured_decoder(generation, output_epoch, output, position);
                } else {
                    self.install_output(output);
                    self.complete_output_refresh();
                    self.emit_output_changed(&self.output).await;
                    self.sync_ui().await;
                }
            }
            OutputRefreshEvent::DecoderReady {
                playback_id,
                position,
                output,
                deck_mixer,
                result,
                ..
            } => match result {
                Ok(handle) => {
                    if playback_id != self.decoder_playback_id.wrapping_add(1) {
                        self.complete_output_refresh();
                        return;
                    }
                    let is_playing = self.playback_intent == PlaybackIntent::Playing;
                    let pending_seek = self.pending_seek.take();
                    let commit_position = if let Some(seek) = pending_seek.as_ref() {
                        seek.position
                    } else {
                        self.output_rebuild_position().await
                    };
                    let commit_position = self.clock.lock().set_anchor(is_playing, commit_position);
                    if (commit_position - position).abs() > 0.025 {
                        let seek_ack = handle.seek(Duration::from_secs_f64(commit_position));
                        let seek_result = match seek_ack {
                            Ok(ack) => ack.wait().await,
                            Err(err) => Err(err),
                        };
                        if let Err(err) = seek_result {
                            warn!(
                                "热重建提交前同步 seek 失败，将按最新位置重新准备解码器: {err:?}"
                            );
                            handle.stop();
                            self.pending_seek = pending_seek;
                            self.spawn_reconfigured_decoder(
                                self.output_refresh_generation,
                                self.output_epoch,
                                output,
                                commit_position,
                            );
                            return;
                        }
                    }
                    if let Some(handle) = self.current_decoder_handle.take() {
                        handle.stop();
                    }
                    if let Some(handle) = self.secondary_decoder_handle.take() {
                        handle.stop();
                    }
                    self.decoder_playback_id = playback_id;
                    let writer = output.writer();
                    let config = output.config();
                    self.output = output;
                    self.mark_output_chain_committed();
                    self.deck_mixer = deck_mixer;
                    self.deck_mixer.set_dsp(self.dsp_config.clone());
                    self.active_deck = DeckId::Primary;
                    self.secondary_playback_id = None;
                    self.native_crossfade_generation =
                        self.native_crossfade_generation.wrapping_add(1);
                    self.native_crossfade_active = false;
                    self.native_crossfade_transition_id = None;
                    self.clock.lock().set_render_clock(
                        writer.render_clock(),
                        config.sample_rate,
                        config.channels,
                    );
                    self.output.writer().set_volume(self.volume as f32);
                    if is_playing {
                        self.current_decoder_handle = Some(handle);
                        self.resume_audio_output().await;
                    } else {
                        self.output.writer().set_paused(true);
                        let _ = handle.set_paused(true);
                        self.current_decoder_handle = Some(handle);
                    }
                    let _ = self.analysis_tx.send(AnalysisCommand::Clear);
                    self.reset_output_health();
                    self.complete_output_refresh();
                    self.clear_output_error();
                    self.emit_output_changed(&self.output).await;
                    self.publish_position_anchor(is_playing, commit_position)
                        .await;
                    if let Some(seek) = pending_seek {
                        self.emit_seek_committed(seek).await;
                    }
                    if is_playing {
                        let _ = self
                            .emitter()
                            .emit(AudioThreadEvent::PlayStatus { is_playing: true })
                            .await;
                    }
                    self.sync_ui().await;
                }
                Err(err) => {
                    self.complete_output_refresh();
                    warn!("切换音频输出后准备新解码器失败，将保留当前播放链路并重试: {err:?}");
                    let is_playing = self.playback_intent == PlaybackIntent::Playing;
                    let position = self.output_rebuild_position().await;
                    self.publish_position_anchor(is_playing, position).await;
                    self.sync_ui().await;
                }
            },
        }
    }

    fn spawn_reconfigured_decoder(
        &mut self,
        generation: u64,
        output_epoch: u64,
        output: LowLatencyOutput,
        position: f64,
    ) {
        let Some(path) = self.current_local_path.clone() else {
            self.complete_output_refresh();
            return;
        };

        let playback_id = self.decoder_playback_id.wrapping_add(1);
        output.writer().set_volume(0.0);
        output.writer().set_paused(true);
        let output_config = output.config();
        let deck_mixer = DeckMixer::new(
            output.writer(),
            output_config.channels,
            output_config.sample_rate,
            &self.dsp_config,
        );
        let output_writer = deck_mixer.primary_writer();
        let analysis_tx = self.analysis_tx.clone();
        let analysis_enabled = Arc::clone(&self.analysis_enabled);
        let decoder_event_tx = self.decoder_event_tx.clone();
        let start_paused = true;
        let seek_position = position.max(0.0);
        let tx = self.output_refresh_tx.clone();

        tokio::task::spawn_blocking(move || {
            let result = decoder::spawn_playback_decoder(
                &path,
                (seek_position > 0.0).then_some(seek_position),
                output_writer,
                output_config.channels,
                output_config.sample_rate,
                analysis_tx,
                analysis_enabled,
                decoder_event_tx,
                playback_id,
                start_paused,
            );
            let _ = tx.send(OutputRefreshEvent::DecoderReady {
                generation,
                output_epoch,
                playback_id,
                position: seek_position,
                output,
                deck_mixer,
                result,
            });
        });
    }

    fn clock_position(&self) -> f64 {
        self.clock.lock().position()
    }

    async fn output_rebuild_position(&self) -> f64 {
        if let Some(seek) = self.pending_seek.as_ref() {
            return seek.position;
        }
        self.clock_position()
    }

    async fn publish_position_anchor(&self, is_playing: bool, position: f64) {
        let position = {
            let mut clock = self.clock.lock();
            clock.set_anchor(is_playing, position)
        };
        *self.current_position.write().await = position;
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::PlayPosition { position })
            .await;
    }

    async fn emit_seek_committed(&self, seek: SeekRequest) {
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::SeekCommitted {
                request_id: seek.request_id,
                position: seek.position,
            })
            .await;
    }

    async fn emit_seek_failed(&self, seek: SeekRequest, error: impl Into<String>) {
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::SeekFailed {
                request_id: seek.request_id,
                position: seek.position,
                error: error.into(),
            })
            .await;
    }

    async fn sync_ui(&self) {
        let audio_info = self.current_audio_info.read().await.clone();
        let (position, is_playing) = {
            let clock = self.clock.lock();
            (clock.position(), clock.is_playing())
        };
        *self.current_position.write().await = position;
        let quality = self.current_audio_quality.read().await.clone();
        let duration = audio_info.duration;

        let status_event = AudioThreadEvent::SyncStatus {
            music_id: self
                .current_song
                .as_ref()
                .map(|s| s.get_id())
                .unwrap_or_default(),
            music_info: audio_info,
            is_playing,
            duration,
            position,
            volume: self.volume,
            load_position: 0.0,
            playlist_inited: self.playlist_inited,
            playlist: self.playlist.clone(),
            current_play_index: self.current_play_index,
            quality,
        };
        let _ = self.emitter().emit(status_event).await;
    }

    async fn run(mut self) {
        let mut output_device_check = tokio::time::interval(Duration::from_secs(1));
        output_device_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut output_health_check = tokio::time::interval(Duration::from_millis(100));
        output_health_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
              biased;

              seek = self.seek_rx.recv() => {
                if let Some(first_seek) = seek {
                  let seek = self.drain_latest_seek(first_seek);
                  if let Err(err) = self.process_seek_request(seek).await {
                    warn!("处理 seek 消息时出错：{err:?}");
                  }
                } else {
                  break;
                }
              }

              msg = self.msg_receiver.recv() => {
                if let Some(msg) = msg {
                  if let Some(AudioThreadMessage::Close) = &msg.data { break; }
                  if let Err(err) = self.process_message(msg).await {
                    warn!("处理音频线程消息时出错：{err:?}");
                  }
                } else { break; }
              }

              msg = self.self_msg_rx.recv() => {
                if let Some(msg) = msg {
                  if let Err(err) = self.process_message(msg).await {
                    warn!("处理内部音频线程消息时出错：{err:?}");
                  }
                }
              }

              event = self.decoder_event_rx.recv() => {
                if let Some(decoder::DecoderEvent::Finished { playback_id }) = event {
                  self.handle_decoder_finished(playback_id).await;
                }
              }

              result = self.automix_prepare_rx.recv() => {
                if let Some(result) = result {
                  self.handle_automix_prepare_result(result).await;
                }
              }

              _ = output_device_check.tick() => {
                self.request_output_refresh(false, false);
              }

              _ = output_health_check.tick() => {
                let output_failed = self.output.has_failed();
                let output_stalled = self.output_render_stalled();
                if output_failed || output_stalled {
                  self.request_output_refresh(true, output_stalled);
                }
              }

              event = self.output_refresh_rx.recv() => {
                if let Some(event) = event {
                  self.handle_output_refresh_event(event).await;
                }
              }
            }
        }

        // Cleanup
        for task in &self.tasks {
            task.abort();
        }
        // Dropping `self.analysis_tx` (when `AudioPlayer` drops below) signals
        // the analysis thread to exit via `mpsc::RecvTimeoutError::Disconnected`.
        // The `Drop for AudioPlayer` impl joins the thread.
    }

    fn drain_latest_seek(&mut self, first_seek: SeekRequest) -> SeekRequest {
        let mut seek = first_seek;
        while let Ok(next_seek) = self.seek_rx.try_recv() {
            seek = next_seek;
        }
        seek
    }

    async fn handle_decoder_finished(&mut self, playback_id: u64) {
        if self.native_crossfade_active
            && (playback_id == self.decoder_playback_id
                || Some(playback_id) == self.secondary_playback_id)
        {
            return;
        }

        if playback_id != self.decoder_playback_id || self.current_song.is_none() {
            return;
        }

        let finished_id = self
            .current_song
            .as_ref()
            .map(|s| s.get_id())
            .unwrap_or_default();
        self.current_decoder_handle = None;
        self.publish_position_anchor(false, 0.0).await;
        self.current_song = None;
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::AudioPlayFinished {
                music_id: finished_id,
            })
            .await;

        if !self.playlist.is_empty() {
            let _ = self.self_msg_tx.send(AudioThreadEventMessage::new(
                "".into(),
                Some(AudioThreadMessage::NextSongGapless),
            ));
        }
    }

    fn seek_matches_current_song(&self, seek: &SeekRequest) -> bool {
        let Some(expected) = seek.expected_music_id.as_deref() else {
            return true;
        };
        self.current_song
            .as_ref()
            .is_some_and(|song| song.get_id() == expected)
    }

    async fn apply_pending_seek_if_ready(&mut self) -> anyhow::Result<bool> {
        let Some(seek) = self.pending_seek.clone() else {
            return Ok(false);
        };
        if !self.seek_matches_current_song(&seek) {
            self.pending_seek = None;
            warn!(
                "忽略已过期 seek 请求: {:.3}s, expected={:?}",
                seek.position, seek.expected_music_id
            );
            self.emit_seek_failed(seek, "seek 请求所属歌曲已不是当前歌曲")
                .await;
            self.sync_ui().await;
            return Ok(true);
        }
        let Some(handle) = self.current_decoder_handle.as_ref() else {
            return Ok(false);
        };

        let was_playing = self.playback_intent == PlaybackIntent::Playing;
        if was_playing {
            self.output.writer().set_paused(true);
        }

        let seek_ack = handle.seek(Duration::from_secs_f64(seek.position));
        let seek_result = match seek_ack {
            Ok(ack) => ack.wait().await,
            Err(err) => Err(err),
        };
        if let Err(err) = seek_result {
            if was_playing {
                self.output.writer().set_paused(false);
            }
            self.pending_seek = None;
            let error = err.to_string();
            warn!("decoder 原地 seek 失败: {:.3}s, {error}", seek.position);
            self.emit_seek_failed(seek, error).await;
            self.sync_ui().await;
            return Ok(true);
        }
        self.pending_seek = None;
        let _ = self.analysis_tx.send(AnalysisCommand::Clear);
        if was_playing {
            // Keep the deep Android queues for steady-state playback, while
            // using a much smaller first-audio watermark after an in-place seek.
            self.wait_for_seek_prebuffer().await;
            self.output.writer().set_paused(false);
        }
        self.publish_position_anchor(was_playing, seek.position)
            .await;
        self.reset_output_health();
        self.emit_seek_committed(seek).await;
        self.sync_ui().await;
        Ok(true)
    }

    async fn process_seek_request(&mut self, request: SeekRequest) -> anyhow::Result<()> {
        let seek = request.normalized();
        if self.current_song.is_none() {
            self.pending_seek = None;
            warn!("没有当前歌曲, 忽略 seek 请求: {:.3}s", seek.position);
            self.emit_seek_failed(seek, "没有当前歌曲可 seek").await;
            return Ok(());
        }

        if !self.seek_matches_current_song(&seek) {
            warn!(
                "忽略非当前歌曲 seek 请求: {:.3}s, expected={:?}, current={:?}",
                seek.position,
                seek.expected_music_id,
                self.current_song.as_ref().map(SongData::get_id)
            );
            self.emit_seek_failed(seek, "seek 请求所属歌曲已不是当前歌曲")
                .await;
            self.sync_ui().await;
            return Ok(());
        }

        self.cancel_native_automix_runtime().await;
        self.automix_prepare_generation = self.automix_prepare_generation.wrapping_add(1);
        let events = self.automix.cancel(self.current_play_index);
        self.emit_many(events).await;

        self.pending_seek = Some(seek.clone());
        match self.apply_pending_seek_if_ready().await {
            Ok(true) => {}
            Ok(false) => {
                let is_playing = self.playback_intent == PlaybackIntent::Playing;
                self.publish_position_anchor(is_playing, seek.position)
                    .await;
                self.sync_ui().await;
                warn!("解码器暂不可用, 已延后 seek 到 {:.3}s", seek.position);
            }
            Err(err) => {
                self.pending_seek = None;
                let error = err.to_string();
                warn!("执行 seek 失败: {error}");
                self.emit_seek_failed(seek, error).await;
                self.sync_ui().await;
            }
        }
        Ok(())
    }

    async fn resume_audio_output(&self) {
        if let Some(handle) = &self.current_decoder_handle {
            let _ = handle.set_paused(false);
        }
        if let Some(handle) = &self.secondary_decoder_handle {
            let _ = handle.set_paused(false);
        }

        self.wait_for_start_prebuffer().await;
        self.output.writer().set_paused(false);
    }

    async fn wait_for_start_prebuffer(&self) {
        self.wait_for_prebuffer(
            start_prebuffer_samples,
            START_PREBUFFER_WAIT_MS,
            "音频输出预缓冲不足",
        )
        .await;
    }

    async fn wait_for_seek_prebuffer(&self) {
        self.wait_for_prebuffer(
            seek_prebuffer_samples,
            SEEK_PREBUFFER_WAIT_MS,
            "seek 快速预缓冲不足",
        )
        .await;
    }

    async fn wait_for_prebuffer(
        &self,
        target_samples_for: fn(usize, u32) -> usize,
        wait_ms: u64,
        warning: &str,
    ) {
        if self.current_decoder_handle.is_none() && self.secondary_decoder_handle.is_none() {
            return;
        }

        let writer = self.output.writer();
        let output_config = self.output.config();
        let channels = output_config.channels.max(1) as usize;
        let target_samples = target_samples_for(channels, output_config.sample_rate);
        for _ in 0..wait_ms {
            if writer.queued_samples() >= target_samples {
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let queued = writer.queued_samples();
        if queued < target_samples {
            warn!(
                "{}: queued_samples={} target_samples={}",
                warning, queued, target_samples
            );
        }
    }

    async fn process_message(
        &mut self,
        msg: AudioThreadEventMessage<AudioThreadMessage>,
    ) -> anyhow::Result<()> {
        let emitter = self.emitter();
        if let Some(ref data) = msg.data {
            match data {
                AudioThreadMessage::ResumeAudio => {
                    self.playback_intent = PlaybackIntent::Playing;
                    self.resume_audio_output().await;
                    let current_pos = self.clock_position();
                    self.publish_position_anchor(true, current_pos).await;
                    let _ = emitter
                        .emit(AudioThreadEvent::PlayStatus { is_playing: true })
                        .await;
                }
                AudioThreadMessage::PauseAudio => {
                    self.playback_intent = PlaybackIntent::Paused;
                    let current_pos = self.clock_position();
                    self.output.writer().set_paused(true);
                    if let Some(handle) = &self.current_decoder_handle {
                        let _ = handle.set_paused(true);
                    }
                    if let Some(handle) = &self.secondary_decoder_handle {
                        let _ = handle.set_paused(true);
                    }
                    self.publish_position_anchor(false, current_pos).await;
                    let _ = emitter
                        .emit(AudioThreadEvent::PlayStatus { is_playing: false })
                        .await;
                }
                AudioThreadMessage::ResumeOrPauseAudio => {
                    let was_paused = self.playback_intent == PlaybackIntent::Paused;
                    let current_pos = self.clock_position();
                    if was_paused {
                        self.playback_intent = PlaybackIntent::Playing;
                        self.resume_audio_output().await;
                    } else {
                        self.playback_intent = PlaybackIntent::Paused;
                        self.output.writer().set_paused(true);
                        if let Some(handle) = &self.current_decoder_handle {
                            let _ = handle.set_paused(true);
                        }
                        if let Some(handle) = &self.secondary_decoder_handle {
                            let _ = handle.set_paused(true);
                        }
                    }
                    self.publish_position_anchor(was_paused, current_pos).await;
                    let _ = emitter
                        .emit(AudioThreadEvent::PlayStatus {
                            is_playing: was_paused,
                        })
                        .await;
                }
                AudioThreadMessage::SeekAudio {
                    position,
                    request_id,
                    expected_music_id,
                } => {
                    self.process_seek_request(SeekRequest::new(
                        *position,
                        *request_id,
                        expected_music_id.clone(),
                    ))
                    .await?;
                }
                AudioThreadMessage::SetVolume { volume } => {
                    self.volume = volume.clamp(0.0, 1.0);
                    self.output.writer().set_volume(self.volume as f32);
                    let _ = emitter
                        .emit(AudioThreadEvent::VolumeChanged {
                            volume: self.volume,
                        })
                        .await;
                }
                AudioThreadMessage::SetVolumeRelative { volume } => {
                    self.volume = (self.volume + volume).clamp(0.0, 1.0);
                    self.output.writer().set_volume(self.volume as f32);
                    let _ = emitter
                        .emit(AudioThreadEvent::VolumeChanged {
                            volume: self.volume,
                        })
                        .await;
                }
                AudioThreadMessage::NextSong => {
                    if self.playback_queue.next().is_none() || !self.sync_current_from_queue() {
                        return self.finish_message(msg).await;
                    }
                    self.start_playing_song(true, None).await?;
                }
                AudioThreadMessage::NextSongGapless => {
                    if self.playback_queue.next().is_none() || !self.sync_current_from_queue() {
                        return self.finish_message(msg).await;
                    }
                    self.start_playing_song(true, None).await?;
                }
                AudioThreadMessage::PrevSong => {
                    if self.playback_queue.prev().is_none() || !self.sync_current_from_queue() {
                        return self.finish_message(msg).await;
                    }
                    self.start_playing_song(true, None).await?;
                }
                AudioThreadMessage::JumpToSong { song_index } => {
                    if self.playback_queue.set_index(*song_index).is_some()
                        && self.sync_current_from_queue()
                    {
                        self.start_playing_song(true, None).await?;
                    }
                }
                AudioThreadMessage::JumpToSongAt {
                    song_index,
                    position,
                } => {
                    if self.playback_queue.set_index(*song_index).is_some()
                        && self.sync_current_from_queue()
                    {
                        self.start_playing_song(true, Some(*position)).await?;
                    }
                }
                AudioThreadMessage::SetPlaylist { songs } => {
                    self.playback_queue.set_playlist(songs.clone());
                    self.playlist = self.playback_queue.playlist_cloned();
                    self.sync_current_from_queue();
                    self.playlist_inited = true;
                    let _ = emitter
                        .emit(AudioThreadEvent::PlayListChanged {
                            playlist: self.playlist.clone(),
                            current_play_index: self.current_play_index,
                        })
                        .await;
                }
                AudioThreadMessage::SetAnalysis { enabled } => {
                    self.analysis_enabled.store(*enabled, Ordering::Release);
                    let _ = self
                        .analysis_tx
                        .send(AnalysisCommand::SetEnabled { enabled: *enabled });
                }
                AudioThreadMessage::SetFFT { enabled } => {
                    let _ = self
                        .analysis_tx
                        .send(AnalysisCommand::SetFftEnabled { enabled: *enabled });
                }
                AudioThreadMessage::SetFFTRange { from_freq, to_freq } => {
                    let _ = self.analysis_tx.send(AnalysisCommand::SetFreqRange {
                        from: *from_freq,
                        to: *to_freq,
                    });
                }
                AudioThreadMessage::SetEqualizer { config } => {
                    self.dsp_config.equalizer = config.clone();
                    self.dsp_config.enabled = dsp_config_is_active(&self.dsp_config);
                    self.deck_mixer.set_dsp(self.dsp_config.clone());
                }
                AudioThreadMessage::SetDsp { config } => {
                    self.dsp_config = config.clone();
                    self.deck_mixer.set_dsp(self.dsp_config.clone());
                }
                AudioThreadMessage::SetAudioOutput { name } => {
                    let selector = output::OutputDeviceSelector::from_name(name);
                    if selector != self.output_selector {
                        self.cancel_pending_output_refresh();
                        self.output_selector = selector;
                    }
                    self.reset_output_refresh_backoff();
                    self.clear_output_error();
                    self.request_output_refresh(true, false);
                }
                AudioThreadMessage::SyncStatus => {
                    // Explicit snapshot request from the frontend — emit it here.
                    // `finish_message` no longer emits SyncStatus by default.
                    self.sync_ui().await;
                }
                AudioThreadMessage::Close => {
                    // Handled in run() loop before reaching here.
                }
                AudioThreadMessage::SetMediaControlsEnabled { .. } => {
                    // OS media controls require platform-specific glue (SMTC etc.) —
                    // not yet wired into this backend.
                }
                AudioThreadMessage::AutomixSetEnabled { enabled } => {
                    let events = self.automix.set_enabled(*enabled, self.current_play_index);
                    if !enabled {
                        self.automix_prepare_generation =
                            self.automix_prepare_generation.wrapping_add(1);
                        self.cancel_native_automix_runtime().await;
                    }
                    self.emit_many(events).await;
                }
                AudioThreadMessage::AutomixConfigure { config } => {
                    let events = self
                        .automix
                        .configure(config.clone(), self.current_play_index);
                    if !config.enabled {
                        self.automix_prepare_generation =
                            self.automix_prepare_generation.wrapping_add(1);
                        self.cancel_native_automix_runtime().await;
                    }
                    self.emit_many(events).await;
                }
                AudioThreadMessage::AutomixPrepareNext {
                    current_index,
                    next_index,
                    next_song,
                    transition_id,
                } => {
                    info!(
                        "AutoMix prepare requested: current_index={}, next_index={}, transition_id={:?}",
                        current_index, next_index, transition_id
                    );
                    self.automix_prepare_generation =
                        self.automix_prepare_generation.wrapping_add(1);
                    let generation = self.automix_prepare_generation;
                    self.cancel_native_automix_runtime().await;
                    let current_song = self.current_song.clone();
                    let current_source_path = self.current_local_path.clone();
                    let current_duration = Some(self.current_audio_info.read().await.duration)
                        .filter(|duration| *duration > 0.0);
                    let (events, request) = self.automix.begin_prepare_next(
                        generation,
                        *transition_id,
                        *current_index,
                        *next_index,
                        current_song,
                        current_duration,
                        current_source_path,
                        next_song.clone(),
                    );
                    if let Some(request) = request {
                        self.spawn_automix_prepare_task(request);
                    }
                    self.emit_many(events).await;
                }
                AudioThreadMessage::AutomixCancel => {
                    self.automix_prepare_generation =
                        self.automix_prepare_generation.wrapping_add(1);
                    self.cancel_native_automix_runtime().await;
                    let events = self.automix.cancel(self.current_play_index);
                    self.emit_many(events).await;
                }
                AudioThreadMessage::AutomixForceStart { generation } => {
                    if let Some(generation) = generation {
                        if *generation != self.native_crossfade_generation {
                            return self.finish_message(msg).await;
                        }
                    }
                    if self.native_crossfade_active {
                        return self.finish_message(msg).await;
                    }
                    if let Err(err) = self.start_native_automix_crossfade().await {
                        let events = self
                            .automix
                            .mark_failed(err.to_string(), self.current_play_index);
                        self.emit_many(events).await;
                    }
                }
                AudioThreadMessage::AutomixCompleteNative {
                    generation,
                    current_index,
                    position,
                } => {
                    if *generation != self.native_crossfade_generation {
                        return self.finish_message(msg).await;
                    }
                    self.complete_native_automix(*current_index, *position)
                        .await;
                }
            }
        }
        self.finish_message(msg).await
    }

    async fn emit_many(&self, events: Vec<AudioThreadEvent>) {
        let emitter = self.emitter();
        for event in events {
            let _ = emitter.emit(event).await;
        }
    }

    fn sync_current_from_queue(&mut self) -> bool {
        let Some(song) = self.playback_queue.current_song() else {
            self.current_song = None;
            self.current_play_index = 0;
            return false;
        };
        self.current_play_index = self.playback_queue.current_index();
        self.current_song = Some(song);
        true
    }

    fn spawn_automix_prepare_task(&self, request: automix::AutoMixPrepareRequest) {
        let tx = self.automix_prepare_tx.clone();
        tokio::spawn(async move {
            let generation = request.generation;
            let transition_id = request.transition_id;
            let current_index = request.current_index;
            let current_id = request.current_id.clone();
            let task =
                tokio::task::spawn_blocking(move || automix::run_prepare_request_blocking(request));
            let result = match tokio::time::timeout(Duration::from_secs(60), task).await {
                Ok(Ok(result)) => result,
                Ok(Err(err)) => automix::AutoMixPrepareResult {
                    generation,
                    transition_id,
                    current_index,
                    current_id,
                    result: Err(format!("AutoMix prepare task failed: {err}")),
                },
                Err(_) => automix::AutoMixPrepareResult {
                    generation,
                    transition_id,
                    current_index,
                    current_id,
                    result: Err("AutoMix prepare timed out".to_string()),
                },
            };
            let _ = tx.send(result);
        });
    }

    async fn handle_automix_prepare_result(&mut self, result: automix::AutoMixPrepareResult) {
        if result.generation != self.automix_prepare_generation {
            return;
        }

        let status_index = result.current_index;
        let events = self.automix.finish_prepare(result, status_index);
        let status = self.automix.status(status_index);
        self.emit_many(events).await;

        if status.state != AutoMixNativeState::Waiting {
            return;
        }

        if let Err(err) = self.preload_native_automix_deck().await {
            warn!("AutoMix preload failed: {err:?}");
            self.cancel_native_automix_runtime().await;
            let events = self
                .automix
                .mark_failed(format!("AutoMix preload failed: {err}"), status_index);
            self.emit_many(events).await;
            return;
        }

        if let Some(start_time) = status.crossfade_start {
            info!(
                "AutoMix prepared: next_index={:?}, start_time={:.3}, duration={:?}",
                status.next_index, start_time, status.crossfade_duration
            );
            self.schedule_native_automix_trigger(start_time);
        } else {
            warn!(
                "AutoMix prepared without crossfade_start; frontend force-start fallback is required"
            );
        }
    }

    fn inactive_deck(&self) -> DeckId {
        match self.active_deck {
            DeckId::Primary => DeckId::Secondary,
            DeckId::Secondary => DeckId::Primary,
        }
    }

    async fn cancel_native_automix_runtime(&mut self) {
        self.native_crossfade_generation = self.native_crossfade_generation.wrapping_add(1);
        self.native_crossfade_active = false;
        self.native_crossfade_transition_id = None;
        self.secondary_playback_id = None;

        if let Some(handle) = self.secondary_decoder_handle.take() {
            handle.stop();
        }
        self.secondary_local_path = None;
        self.secondary_temp_file = None;
        self.secondary_song = None;
        self.secondary_duration = 0.0;
        self.secondary_display_info = None;
        self.secondary_quality = None;

        self.deck_mixer.clear_deck(self.inactive_deck());
        // The current track keeps playing — only the incoming automix deck is
        // torn down — so restore the active deck to its persistent normalization
        // gain rather than snapping it back to 1.0.
        self.deck_mixer
            .set_deck_gain(self.active_deck, self.active_norm_gain);
        self.deck_mixer.set_deck_gain(self.inactive_deck(), 0.0);
        self.secondary_norm_gain = 1.0;
    }

    fn schedule_native_automix_trigger(&mut self, start_time: f64) {
        self.native_crossfade_generation = self.native_crossfade_generation.wrapping_add(1);
        let generation = self.native_crossfade_generation;
        let clock = Arc::clone(&self.clock);
        let tx = self.self_msg_tx.clone();
        tokio::spawn(async move {
            loop {
                let position = clock.lock().position();
                if position + 0.025 >= start_time {
                    let _ = tx.send(AudioThreadEventMessage::new(
                        String::new(),
                        Some(AudioThreadMessage::AutomixForceStart {
                            generation: Some(generation),
                        }),
                    ));
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });
    }

    fn schedule_native_automix_complete(
        &self,
        generation: u64,
        current_index: usize,
        position: f64,
    ) {
        let tx = self.self_msg_tx.clone();
        let delay = Duration::from_secs_f64(position.max(0.05));
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = tx.send(AudioThreadEventMessage::new(
                String::new(),
                Some(AudioThreadMessage::AutomixCompleteNative {
                    generation,
                    current_index,
                    position,
                }),
            ));
        });
    }

    fn secondary_matches_prepared_automix(&self, song: &SongData, local_path: &Path) -> bool {
        let song_id = song.get_id();
        self.secondary_decoder_handle.is_some()
            && self.secondary_playback_id.is_some()
            && self
                .secondary_local_path
                .as_ref()
                .is_some_and(|path| path == local_path)
            && self
                .secondary_song
                .as_ref()
                .is_some_and(|secondary_song| secondary_song.get_id() == song_id)
    }

    async fn preload_native_automix_deck(&mut self) -> anyhow::Result<()> {
        if self.native_crossfade_active {
            return Ok(());
        }

        let prepared = self
            .automix
            .take_prepared_for_preload()
            .ok_or_else(|| anyhow::anyhow!("AutoMix has no prepared next track to preload"))?;

        if let Some(handle) = self.secondary_decoder_handle.take() {
            handle.stop();
        }
        self.secondary_local_path = None;
        self.secondary_temp_file = None;
        self.secondary_song = None;
        self.secondary_duration = 0.0;
        self.secondary_display_info = None;
        self.secondary_quality = None;
        self.secondary_playback_id = None;

        let incoming_deck = self.inactive_deck();
        self.deck_mixer.clear_deck(incoming_deck);
        let incoming_writer = match incoming_deck {
            DeckId::Primary => self.deck_mixer.primary_writer(),
            DeckId::Secondary => self.deck_mixer.secondary_writer(),
        };
        let start_paused = self.playback_intent == PlaybackIntent::Paused;
        incoming_writer.set_paused(start_paused);
        self.deck_mixer.set_deck_gain(incoming_deck, 0.0);

        let analysis_tx_for_open = self.analysis_tx.clone();
        let path_for_open = prepared.local_path.clone();
        let output_config = self.output.config();
        let playback_id = self.decoder_playback_id.wrapping_add(1);
        let decoder_event_tx = self.decoder_event_tx.clone();
        let analysis_enabled_for_open = Arc::clone(&self.analysis_enabled);

        let open_result = tokio::task::spawn_blocking(move || {
            decoder::spawn_playback_decoder(
                &path_for_open,
                Some(0.0),
                incoming_writer,
                output_config.channels,
                output_config.sample_rate,
                analysis_tx_for_open,
                analysis_enabled_for_open,
                decoder_event_tx,
                playback_id,
                start_paused,
            )
        })
        .await?;

        let incoming_handle = open_result.map_err(|e| anyhow::anyhow!(e.to_string()))?;

        self.secondary_decoder_handle = Some(incoming_handle);
        self.secondary_local_path = Some(prepared.local_path);
        self.secondary_temp_file = prepared._temp_file;
        self.secondary_song = Some(prepared.song);
        self.secondary_duration = prepared.analysis_duration;
        self.secondary_display_info = Some(prepared.display_info);
        self.secondary_quality = Some(prepared.quality);
        self.secondary_playback_id = Some(playback_id);

        info!(
            "AutoMix preloaded secondary deck: next_index={}, playback_id={}",
            prepared.next_index, playback_id
        );

        Ok(())
    }

    async fn start_native_automix_crossfade(&mut self) -> anyhow::Result<()> {
        let prepared = self
            .automix
            .take_prepared_for_start()
            .ok_or_else(|| anyhow::anyhow!("AutoMix has no prepared next track"))?;

        let incoming_deck = self.inactive_deck();
        let incoming_writer = match incoming_deck {
            DeckId::Primary => self.deck_mixer.primary_writer(),
            DeckId::Secondary => self.deck_mixer.secondary_writer(),
        };
        let start_paused = self.playback_intent == PlaybackIntent::Paused;
        incoming_writer.set_paused(start_paused);
        self.deck_mixer.set_deck_gain(incoming_deck, 0.0);
        let output_config = self.output.config();
        let preloaded =
            self.secondary_matches_prepared_automix(&prepared.song, &prepared.local_path);
        let playback_id = if preloaded {
            if let Some(handle) = &self.secondary_decoder_handle {
                let _ = handle.set_paused(start_paused);
            }
            self.secondary_playback_id
                .unwrap_or_else(|| self.decoder_playback_id.wrapping_add(1))
        } else {
            if let Some(handle) = self.secondary_decoder_handle.take() {
                handle.stop();
            }
            self.secondary_local_path = None;
            self.secondary_temp_file = None;
            self.secondary_song = None;
            self.secondary_duration = 0.0;
            self.secondary_display_info = None;
            self.secondary_quality = None;
            self.secondary_playback_id = None;
            self.deck_mixer.clear_deck(incoming_deck);

            let analysis_tx_for_open = self.analysis_tx.clone();
            let path_for_open = prepared.local_path.clone();
            let playback_id = self.decoder_playback_id.wrapping_add(1);
            let decoder_event_tx = self.decoder_event_tx.clone();
            let analysis_enabled_for_open = Arc::clone(&self.analysis_enabled);

            let open_result = tokio::task::spawn_blocking(move || {
                decoder::spawn_playback_decoder(
                    &path_for_open,
                    Some(0.0),
                    incoming_writer,
                    output_config.channels,
                    output_config.sample_rate,
                    analysis_tx_for_open,
                    analysis_enabled_for_open,
                    decoder_event_tx,
                    playback_id,
                    start_paused,
                )
            })
            .await?;

            let incoming_handle = open_result.map_err(|e| anyhow::anyhow!(e.to_string()))?;
            self.secondary_decoder_handle = Some(incoming_handle);
            self.secondary_temp_file = prepared._temp_file;
            playback_id
        };
        let duration = prepared.plan.as_ref().map(|p| p.duration).unwrap_or(2.0);

        self.secondary_local_path = Some(prepared.local_path.clone());
        self.secondary_song = Some(prepared.song.clone());
        self.secondary_duration = prepared.analysis_duration;
        self.secondary_display_info = Some(prepared.display_info.clone());
        self.secondary_quality = Some(prepared.quality.clone());
        self.secondary_playback_id = Some(playback_id);
        self.native_crossfade_active = true;
        self.native_crossfade_transition_id = prepared.transition_id;
        info!(
            "AutoMix crossfade starting: next_index={}, preloaded={}, duration={:.3}",
            prepared.next_index, preloaded, duration
        );

        let crossfade_params = prepared
            .plan
            .as_ref()
            .map(|plan| CrossfadeParams {
                curve: plan.curve,
                incoming_gain: plan.incoming_gain_adjustment as f32,
                // The outgoing deck is already playing at its own persistent
                // normalization gain — feed that in as the outgoing target so the
                // crossfade starts from the current level instead of snapping the
                // outgoing track up to 1.0.
                outgoing_gain: self.active_norm_gain,
                overlap_headroom_db: plan.overlap_headroom_db as f32,
            })
            .unwrap_or_else(|| CrossfadeParams {
                outgoing_gain: self.active_norm_gain,
                ..Default::default()
            });
        // Remember the incoming track's normalization gain so it can be promoted
        // to `active_norm_gain` on completion — keeping the level continuous into
        // steady-state playback and the next crossfade's outgoing side.
        self.secondary_norm_gain = crossfade_params.incoming_gain;

        self.deck_mixer.start_crossfade(
            self.active_deck,
            incoming_deck,
            duration,
            output_config.sample_rate,
            output_config.channels,
            crossfade_params,
        );

        let _ = self
            .emitter()
            .emit(AudioThreadEvent::AutomixCrossfadeStarted {
                from_id: self
                    .current_song
                    .as_ref()
                    .map(SongData::get_id)
                    .unwrap_or_default(),
                to_id: prepared.song.get_id(),
                duration,
                transition_id: prepared.transition_id,
            })
            .await;

        let finish_index = prepared.next_index;
        self.schedule_native_automix_complete(
            self.native_crossfade_generation,
            finish_index,
            duration,
        );

        Ok(())
    }

    async fn complete_native_automix(&mut self, current_index: usize, position: f64) {
        if !self.native_crossfade_active || self.secondary_decoder_handle.is_none() {
            return;
        }

        if let Some(handle) = self.current_decoder_handle.take() {
            handle.stop();
        }

        self.current_decoder_handle = self.secondary_decoder_handle.take();
        self.current_local_path = self.secondary_local_path.take();
        self.current_temp_file = self.secondary_temp_file.take();
        let promoted_song = self.secondary_song.take();
        self.current_song = promoted_song.clone();
        if let Some(playback_id) = self.secondary_playback_id.take() {
            self.decoder_playback_id = playback_id;
        }
        self.current_play_index = current_index;
        if let Some(song) = promoted_song {
            self.playback_queue
                .replace_or_set_current(current_index, song);
            self.playlist = self.playback_queue.playlist_cloned();
            self.sync_current_from_queue();
        }
        let incoming_duration = self.secondary_duration;
        self.secondary_duration = 0.0;
        self.native_crossfade_active = false;
        let transition_id = self.native_crossfade_transition_id.take();
        self.active_deck = match self.active_deck {
            DeckId::Primary => DeckId::Secondary,
            DeckId::Secondary => DeckId::Primary,
        };
        // The incoming track (now active) is already playing at its normalization
        // gain — the mixer left the deck there at crossfade end. Adopt it as the
        // persistent active gain so cancels/rebuilds restore the right level.
        self.active_norm_gain = self.secondary_norm_gain;
        self.secondary_norm_gain = 1.0;

        let mut display_info = self.secondary_display_info.take().unwrap_or_default();
        display_info.duration = if display_info.duration > 0.0 {
            display_info.duration
        } else {
            incoming_duration
        };
        display_info.position = position;
        let duration = display_info.duration;
        *self.current_audio_info.write().await = display_info;
        if let Some(quality) = self.secondary_quality.take() {
            *self.current_audio_quality.write().await = quality;
        }

        let is_playing = self.playback_intent == PlaybackIntent::Playing;
        self.clock.lock().set_duration(incoming_duration);
        self.publish_position_anchor(is_playing, position).await;

        let music_id = self
            .current_song
            .as_ref()
            .map(SongData::get_id)
            .unwrap_or_default();

        let _ = self
            .emitter()
            .emit(AudioThreadEvent::AutomixCrossfadeComplete {
                current_index: self.current_play_index,
                music_id,
                position,
                duration,
                transition_id,
            })
            .await;
        let events = self.automix.complete(self.current_play_index);
        self.emit_many(events).await;
        self.sync_ui().await;
    }

    /// Ack the request (so the frontend's callback_id pairing resolves) and
    /// return. Specific state-change events (PlayStatus, PlayPosition,
    /// VolumeChanged, PlayListChanged) are emitted at the point of change.
    /// SyncStatus snapshots are only emitted on explicit `SyncStatus`
    /// requests or after `start_playing_song` — emitting one per command
    /// caused races where a snapshot could observe the old playback intent
    /// before a follow-on ResumeAudio had been processed.
    async fn finish_message(
        &self,
        msg: AudioThreadEventMessage<AudioThreadMessage>,
    ) -> anyhow::Result<()> {
        let _ = self.emitter().ret_none(msg).await;
        Ok(())
    }

    async fn start_playing_song(
        &mut self,
        clear_sink: bool,
        initial_position: Option<f64>,
    ) -> anyhow::Result<()> {
        self.pending_seek = None;
        let song_data = self
            .current_song
            .clone()
            .ok_or_else(|| anyhow::anyhow!("没有当前歌曲可播放"))?;

        let file_path = match &song_data {
            SongData::Local { file_path, .. } => file_path.clone(),
            _ => return Err(anyhow::anyhow!("当前实现仅支持本地文件 / HTTP(S) 流")),
        };

        // Emit LoadingAudio so the frontend can show a spinner / await load.
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::LoadingAudio {
                music_id: song_data.get_id(),
                current_play_index: self.current_play_index,
            })
            .await;

        if clear_sink {
            self.cancel_pending_output_refresh();
            self.automix_prepare_generation = self.automix_prepare_generation.wrapping_add(1);
            if let Some(handle) = self.current_decoder_handle.take() {
                handle.stop();
            }
            if let Some(handle) = self.secondary_decoder_handle.take() {
                handle.stop();
            }
            self.deck_mixer.clear_all();

            let _ = self.analysis_tx.send(AnalysisCommand::Clear);

            // Drop the previous temp file (if any) by clearing the guard. This
            // must happen BEFORE we assign the new temp path so disk usage stays
            // bounded.
            self.current_local_path = None;
            self.current_temp_file = None;
            self.secondary_local_path = None;
            self.secondary_temp_file = None;
            self.secondary_song = None;
            self.secondary_duration = 0.0;
            self.secondary_display_info = None;
            self.secondary_quality = None;
            self.secondary_playback_id = None;
            self.active_deck = DeckId::Primary;
            self.deck_mixer.set_deck_gain(DeckId::Primary, 1.0);
            self.deck_mixer.set_deck_gain(DeckId::Secondary, 0.0);
            // Fresh (non-automix) load starts unnormalized at unity gain.
            self.active_norm_gain = 1.0;
            self.secondary_norm_gain = 1.0;
            self.native_crossfade_generation = self.native_crossfade_generation.wrapping_add(1);
            self.native_crossfade_active = false;
        }

        self.current_file_path = Some(file_path.clone());

        // Resolve URL → local path (download to temp) so rodio's File-based
        // decoder can read it.
        let resolve_path = file_path.clone();
        let resolve_result = tokio::task::spawn_blocking(
            move || -> AudioResult<(PathBuf, Option<tempfile::TempPath>)> {
                if decoder::is_http_url(&resolve_path) {
                    let temp = decoder::download_to_temp_path(&resolve_path)?;
                    let path = temp.to_path_buf();
                    Ok((path, Some(temp)))
                } else {
                    Ok((PathBuf::from(&resolve_path), None))
                }
            },
        )
        .await?;

        let (local_path, temp_file) = match resolve_result {
            Ok(t) => t,
            Err(e) => {
                warn!("解析音频源失败: {e:?}");
                let _ = self
                    .emitter()
                    .emit(AudioThreadEvent::LoadError {
                        error: e.to_string(),
                    })
                    .await;
                return Err(e.into());
            }
        };

        self.current_local_path = Some(local_path.clone());
        self.current_temp_file = temp_file;

        // Read metadata before opening output so the CPAL stream can match the
        // source's channel count and sample rate where the device supports it.
        let path_for_info = local_path.clone();
        let info_result = tokio::task::spawn_blocking(move || {
            decoder::symphonia::extract_metadata_only(&path_for_info)
        })
        .await?;

        let audio_info = match info_result {
            Ok(info) => info,
            Err(e) => {
                warn!("读取音频元数据失败: {e:?}");
                let _ = self
                    .emitter()
                    .emit(AudioThreadEvent::LoadError {
                        error: e.to_string(),
                    })
                    .await;
                return Err(e.into());
            }
        };

        let _output_reopened = self.ensure_output_for_source(&audio_info)?;
        self.output.writer().set_volume(self.volume as f32);
        // Keep the device callback silent until the decoder/mixer has filled
        // the output queue; otherwise Android can underrun before the first PCM block arrives.
        self.output.writer().set_paused(true);

        // `initial_position` is applied inside the decoder worker before it
        // starts pushing PCM, avoiding a separate post-load seek round trip.
        let analysis_tx_for_open = self.analysis_tx.clone();
        let path_for_open = local_path.clone();
        let output_writer = match self.active_deck {
            DeckId::Primary => self.deck_mixer.primary_writer(),
            DeckId::Secondary => self.deck_mixer.secondary_writer(),
        };
        let output_config = self.output.config();
        self.decoder_playback_id = self.decoder_playback_id.wrapping_add(1);
        let playback_id = self.decoder_playback_id;
        let decoder_event_tx = self.decoder_event_tx.clone();
        let start_paused = self.playback_intent == PlaybackIntent::Paused;
        let seek_into_open = initial_position.filter(|p| *p > 0.0);
        let analysis_enabled_for_open = Arc::clone(&self.analysis_enabled);

        let open_result = tokio::task::spawn_blocking(move || {
            decoder::spawn_playback_decoder(
                &path_for_open,
                seek_into_open,
                output_writer,
                output_config.channels,
                output_config.sample_rate,
                analysis_tx_for_open,
                analysis_enabled_for_open,
                decoder_event_tx,
                playback_id,
                start_paused,
            )
        })
        .await?;

        let handle = match open_result {
            Ok(handle) => handle,
            Err(e) => {
                warn!("打开音频源失败: {e:?}");
                let _ = self
                    .emitter()
                    .emit(AudioThreadEvent::LoadError {
                        error: e.to_string(),
                    })
                    .await;
                return Err(e.into());
            }
        };

        self.current_decoder_handle = Some(handle);

        // The starting position the position task should anchor at.
        let anchor_pos = seek_into_open.unwrap_or(0.0);

        // Use the symphonia-extracted duration; rodio's `total_duration()` is
        // unreliable for MP3 without VBR headers and similar.
        let display_info = DisplayAudioInfo {
            name: extract_title_from_metadata(&audio_info).unwrap_or_else(|| {
                Path::new(&file_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unknown")
                    .to_string()
            }),
            artist: extract_tag(&audio_info, &["artist", "TPE1"]).unwrap_or_default(),
            album: extract_tag(&audio_info, &["album", "TALB"]).unwrap_or_default(),
            duration: audio_info.duration_secs,
            // Carry the initial position so the frontend's `_state.position` is
            // seeded correctly from the very first `LoadAudio` / `SyncStatus`.
            position: anchor_pos,
            ..Default::default()
        };

        let quality = AudioQuality {
            sample_rate: audio_info.sample_rate,
            channels: audio_info.channels,
            bitrate: audio_info.bitrate_bps.map(|b| b as u32).unwrap_or_default(),
        };

        *self.current_audio_info.write().await = display_info.clone();
        *self.current_audio_quality.write().await = quality.clone();
        self.clock.lock().set_duration(audio_info.duration_secs);

        let is_now_playing = self.playback_intent == PlaybackIntent::Playing;
        self.publish_position_anchor(is_now_playing, anchor_pos)
            .await;

        let _ = self
            .emitter()
            .emit(AudioThreadEvent::LoadAudio {
                music_id: song_data.get_id(),
                music_info: display_info,
                quality,
                current_play_index: self.current_play_index,
            })
            .await;
        if is_now_playing {
            self.resume_audio_output().await;
            let _ = self
                .emitter()
                .emit(AudioThreadEvent::PlayStatus { is_playing: true })
                .await;
        }

        self.sync_ui().await;
        Ok(())
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        if let Some(thread) = self.analysis_thread.take() {
            join_thread_async("audio-analysis-join", thread);
        }
    }
}

fn dsp_config_is_active(config: &DspConfig) -> bool {
    if config.input_gain_db.abs() >= 0.001 || config.output_gain_db.abs() >= 0.001 {
        return true;
    }
    if config.limiter.enabled {
        return true;
    }
    config.equalizer.enabled
        && (config.equalizer.preamp_db.abs() >= 0.001
            || config
                .equalizer
                .bands
                .iter()
                .any(|band| band.enabled && band.gain_db.abs() >= 0.001))
}

fn output_audio_layout_matches(a: output::OutputConfigKey, b: output::OutputConfigKey) -> bool {
    a.channels == b.channels && a.sample_rate == b.sample_rate
}

// ── Metadata helpers ─────────────────────────────────────────────

fn extract_tag(info: &AudioInfo, keys: &[&str]) -> Option<String> {
    for (k, v) in &info.metadata_tags {
        for key in keys {
            if k.eq_ignore_ascii_case(key) {
                return Some(v.clone());
            }
        }
    }
    None
}

fn extract_title_from_metadata(info: &AudioInfo) -> Option<String> {
    extract_tag(info, &["title", "TIT2"])
}

// ── EventEmitter helper (mirrors AMLL's AudioPlayerEventEmitter) ──

#[derive(Debug, Clone)]
struct EventEmitter {
    evt_sender: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
}

impl EventEmitter {
    fn new(evt_sender: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>) -> Self {
        Self { evt_sender }
    }

    async fn emit(&self, event: AudioThreadEvent) -> anyhow::Result<()> {
        self.evt_sender
            .send(AudioThreadEventMessage::new("".into(), Some(event)))
            .map_err(|_| anyhow::anyhow!("event channel closed"))?;
        Ok(())
    }

    async fn ret_none(
        &self,
        req: AudioThreadEventMessage<AudioThreadMessage>,
    ) -> anyhow::Result<()> {
        self.evt_sender
            .send(req.to_none::<AudioThreadEvent>())
            .map_err(|_| anyhow::anyhow!("event channel closed"))?;
        Ok(())
    }
}
