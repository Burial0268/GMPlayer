/// AMLL-style message-driven audio player.
///
/// Architecture (mirrors `amll-player-core`):
/// - `Player` — public API: sends `AudioThreadMessage` to the internal player thread.
/// - `PlayerHandle` — cloneable handle for sending messages from anywhere.
/// - `AudioPlayer` — internal event loop that processes messages + emits events.
///
/// Message flow:  frontend → WebSocket → Player::send_msg() → AudioPlayer → decoder/output
/// Event flow:   AudioPlayer → callback → WebSocket broadcast → frontend
///
/// The `AudioPlayer` impl is split by responsibility across sibling modules —
/// this file owns the struct, construction and the `run()` event loop:
/// - `messages`        — `AudioThreadMessage` dispatch (`process_message`)
/// - `playback`        — track loading, prebuffer gating, decoder-finished advance
/// - `seek`            — seek validation/apply/defer
/// - `output_runtime`  — device polling/health, hot-swap refresh, chain rebuilds
/// - `automix_runtime` — native AutoMix deck preload/crossfade scheduling
/// - `planner_runtime` — manifest/planner ownership, one-ahead prefetch, advance
/// - `session_controls`— play mode / favourite: the state every surface shares
/// - `status`          — `EventEmitter` + position/seek/SyncStatus publishing
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::sync::RwLock as TokioRwLock;
use tracing::{info, warn};

use crate::analysis::{self, AnalysisSender};
use crate::decoder;
use crate::error::{AudioError, AudioResult};
use crate::output::{self, LowLatencyOutput};
use crate::types::*;

mod api;
mod automix;
mod automix_runtime;
mod clock;
mod listen_together;
pub mod manifest;
mod messages;
mod metadata_fetch;
mod mixer;
mod now_playing;
mod output_runtime;
mod planner;
mod planner_runtime;
mod platform;
mod playback;
pub mod queue;
mod seek;
pub mod session_controls;
mod source_cache;
pub mod source_resolver;
mod status;

#[allow(unused_imports)]
pub use api::{EventBuffer, Player, PlayerEventSubscriber, PlayerHandle, PlayerShared, SubscriberId};

use api::{join_thread_async, SeekRequest};
use automix::AutoMixManager;
use clock::PlayerClock;
use manifest::ManifestStore;
use mixer::{DeckId, DeckMixer};
use output_runtime::OutputRefreshEvent;
use planner::Planner;
use queue::PlaybackQueue;
use source_cache::{PrefetchResult, SourceCache};
use status::EventEmitter;

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
    output_poll_stride: u32,
    output_poll_ticks: u32,
    output_health_last_samples: u64,
    output_health_stalled_ticks: u8,
    last_output_error: Option<String>,
    decoder_playback_id: u64,
    /// Shared so the AutoMix trigger poll task (`schedule_native_automix_trigger`)
    /// can observe cancellation/supersession and exit instead of polling the
    /// clock forever when a prepared transition is cancelled or playback stays
    /// paused. Bump only through `bump_native_crossfade_gen`.
    native_crossfade_generation: Arc<AtomicU64>,
    native_crossfade_active: bool,
    native_crossfade_transition_id: Option<u64>,
    automix_prepare_generation: u64,

    // Playlist
    playback_queue: PlaybackQueue,
    playlist: Vec<SongData>,
    playlist_inited: bool,
    current_play_index: usize,
    current_song: Option<SongData>,
    /// Display metadata for `current_song`, parsed once when the track was
    /// picked up from the queue. Lets the OS media session show the real title
    /// immediately instead of a decoded tag that is not available yet — or, for
    /// a stream with no tags, a CDN path stem. See `now_playing`.
    pending_display: now_playing::PendingDisplay,
    /// Track announced by the frontend but not loaded yet. Lets the OS media
    /// session swap the moment the user presses next, rather than after the
    /// URL resolve and download. See `metadata_fetch::announce_track`.
    announced_track: Option<(TrackIdentity, TrackDisplay)>,
    /// A load is in flight: source resolve, download, or decoder open.
    ///
    /// Exists for the OS media session, which otherwise shows "paused" for the
    /// entire resolve+download window — the user presses play and the button
    /// flips back, with no indication that anything is happening. Mirrors the
    /// `LoadingAudio` → `LoadAudio` event pair, and is cleared on the failure
    /// paths those events do not cover.
    load_in_flight: bool,

    // Manifest-driven planning. The bounded `playback_queue` above stays the
    // transport to the decoder; these own *what plays next* across the full
    // list, so advancement no longer depends on a live JS runtime.
    manifest: ManifestStore,
    planner: Planner,
    source_cache: SourceCache,
    resolver_config: NativeResolverConfig,
    /// Stable identity of the loaded track. Survives URL re-resolution, unlike
    /// `current_song`'s `local:<url>` id.
    current_identity: Option<TrackIdentity>,
    /// Identity the *next* `start_playing_song` should adopt. Only a
    /// planner-driven load knows the identity up front; every other caller
    /// (frontend `SetPlaylist`, legacy queue hop) leaves this `None` so
    /// `current_identity` is cleared rather than inheriting the previous
    /// track's — otherwise a reloading frontend would adopt the wrong song.
    pending_identity: Option<TrackIdentity>,
    /// Room whose server-side presence this backend keeps alive. `None`
    /// when not in a listen-together session. See `listen_together`.
    listen_together_room: Option<String>,
    /// The one authoritative copy of the user-facing session controls (play
    /// mode, favourite). Every surface reads it through `NowPlayingChanged` /
    /// `SessionControlsChanged` and writes it through a message — see
    /// `session_controls`.
    session_controls: SessionControls,
    /// A like/unlike call is in flight, so a second press is ignored rather
    /// than racing the first to the opposite value.
    favourite_in_flight: bool,
    favourite_tx: mpsc::UnboundedSender<session_controls::FavouriteResult>,
    favourite_rx: mpsc::UnboundedReceiver<session_controls::FavouriteResult>,
    /// Liked track ids for the signed-in account, fetched by the backend itself.
    ///
    /// `None` means "not known" — signed out, or the fetch has not landed. The
    /// backend needs its own copy because the notification's heart is rendered
    /// while the WebView is destroyed, which is precisely when the frontend
    /// cannot answer. Netease ids as strings, matching `TrackIdentity`.
    likelist: Option<std::collections::HashSet<String>>,
    likelist_in_flight: bool,
    likelist_tx: mpsc::UnboundedSender<session_controls::LikelistResult>,
    likelist_rx: mpsc::UnboundedReceiver<session_controls::LikelistResult>,
    prefetch_tx: mpsc::UnboundedSender<PrefetchResult>,
    prefetch_rx: mpsc::UnboundedReceiver<PrefetchResult>,
    /// Backend-side display metadata hydration. Separate channel from the
    /// resolver so a slow `/song/detail` can never delay a source resolve.
    metadata_tx: mpsc::UnboundedSender<metadata_fetch::MetadataResult>,
    metadata_rx: mpsc::UnboundedReceiver<metadata_fetch::MetadataResult>,

    // Shared state snapshots
    current_audio_info: Arc<TokioRwLock<DisplayAudioInfo>>,
    current_position: Arc<TokioRwLock<f64>>,
    current_audio_quality: Arc<TokioRwLock<AudioQuality>>,
    /// Authoritative session snapshot, shared with `PlayerShared` so the
    /// `audio_get_session` command can read it without a message round-trip.
    /// Written from `sync_ui` (full) and `publish_position_anchor` (clock only)
    /// — i.e. every point where playback identity or the timeline changes.
    session: Arc<parking_lot::Mutex<NativeSessionSnapshot>>,

    // Background tasks
    tasks: Vec<tokio::task::JoinHandle<()>>,

    /// Sender to the dedicated `audio-analysis` OS thread (see
    /// `crate::analysis`). Cloned into each `FFTFeedSource` so the audio
    /// callback thread can push interleaved PCM via the channel — replaces
    /// the previous `Arc<ParkingLotRwLock<AudioProcessor>>` lock pattern.
    analysis_tx: AnalysisSender,
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

impl AudioPlayer {
    async fn new(
        msg_receiver: mpsc::UnboundedReceiver<AudioThreadEventMessage<AudioThreadMessage>>,
        seek_rx: mpsc::UnboundedReceiver<SeekRequest>,
        evt_sender: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
        session: Arc<parking_lot::Mutex<NativeSessionSnapshot>>,
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
                let (is_playing, current_pos, timeline_epoch) = {
                    let clock = clock_reader.lock();
                    (clock.is_playing(), clock.position(), clock.epoch())
                };
                if is_playing {
                    *position_writer.write().await = current_pos;
                    let _ = emitter_pos
                        .emit(AudioThreadEvent::PlayPosition {
                            position: current_pos,
                            timeline_epoch,
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
        let (prefetch_tx, prefetch_rx) = mpsc::unbounded_channel();
        let (metadata_tx, metadata_rx) = mpsc::unbounded_channel();
        let (favourite_tx, favourite_rx) = mpsc::unbounded_channel();
        let (likelist_tx, likelist_rx) = mpsc::unbounded_channel();

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
            output_poll_stride: 1,
            output_poll_ticks: 0,
            output_health_last_samples: 0,
            output_health_stalled_ticks: 0,
            last_output_error: None,
            decoder_playback_id: 0,
            native_crossfade_generation: Arc::new(AtomicU64::new(0)),
            native_crossfade_active: false,
            native_crossfade_transition_id: None,
            automix_prepare_generation: 0,
            playback_queue: PlaybackQueue::new(),
            playlist: Vec::new(),
            playlist_inited: false,
            current_play_index: 0,
            current_song: None,
            pending_display: now_playing::PendingDisplay::default(),
            announced_track: None,
            load_in_flight: false,
            manifest: ManifestStore::new(),
            planner: Planner::new(),
            source_cache: SourceCache::new(),
            resolver_config: NativeResolverConfig::default(),
            current_identity: None,
            pending_identity: None,
            listen_together_room: None,
            session_controls: SessionControls::default(),
            favourite_in_flight: false,
            favourite_tx,
            favourite_rx,
            likelist: None,
            likelist_in_flight: false,
            likelist_tx,
            likelist_rx,
            prefetch_tx,
            prefetch_rx,
            metadata_tx,
            metadata_rx,
            current_audio_info,
            current_position,
            current_audio_quality,
            session,
            tasks,
            analysis_tx,
            analysis_thread,
            analysis_enabled,
        })
    }

    async fn run(mut self) {
        let mut output_device_check = tokio::time::interval(Duration::from_secs(1));
        output_device_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut output_health_check = tokio::time::interval(Duration::from_millis(100));
        output_health_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Listen-together keepalive. Ticks unconditionally — the handler is a
        // no-op when no room is armed, and at 30 s it is negligible next to the
        // 100 ms output-health tick already running.
        let mut listen_together_beat = tokio::time::interval(Duration::from_secs(
            listen_together::HEARTBEAT_PERIOD_SECS,
        ));
        listen_together_beat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

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

              result = self.prefetch_rx.recv() => {
                if let Some(result) = result {
                  self.handle_prefetch_result(result).await;
                }
              }

              result = self.metadata_rx.recv() => {
                if let Some(result) = result {
                  self.handle_metadata_result(result).await;
                }
              }

              result = self.favourite_rx.recv() => {
                if let Some(result) = result {
                  self.handle_favourite_result(result).await;
                }
              }

              result = self.likelist_rx.recv() => {
                if let Some(result) = result {
                  self.handle_likelist_result(result).await;
                }
              }

              _ = output_device_check.tick() => {
                self.poll_output_device_tick();
              }

              _ = output_health_check.tick() => {
                // Taken unconditionally: the flag is a one-shot edge, and leaving
                // it set while a named device is selected would fire a stale
                // refresh the moment the user switches back to "system default".
                let default_output_changed = output::take_default_output_changed();
                let output_failed = self.output.has_failed();
                let output_stalled = self.output_render_stalled();
                if output_failed || output_stalled {
                  self.request_output_refresh(true, output_stalled);
                } else if default_output_changed && self.output_selector.is_default() {
                  // The stream is bound to a concrete endpoint (see
                  // `output::platform::default_output_device`), so the OS moving
                  // the default does not disturb it — this is the only prompt
                  // notice, and the 1-3s device poll is the backstop.
                  self.reset_output_poll_stride();
                  self.request_output_refresh(false, false);
                }
              }

              event = self.output_refresh_rx.recv() => {
                if let Some(event) = event {
                  self.handle_output_refresh_event(event).await;
                }
              }

              _ = listen_together_beat.tick() => {
                self.send_listen_together_heartbeat();
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

    fn native_crossfade_gen(&self) -> u64 {
        self.native_crossfade_generation.load(Ordering::Acquire)
    }

    /// Invalidate any in-flight trigger poll task and return the new generation.
    fn bump_native_crossfade_gen(&self) -> u64 {
        self.native_crossfade_generation
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1)
    }

    fn sync_current_from_queue(&mut self) -> bool {
        let Some(song) = self.playback_queue.current_song() else {
            self.current_song = None;
            self.pending_display = now_playing::PendingDisplay::default();
            self.current_play_index = 0;
            return false;
        };
        self.current_play_index = self.playback_queue.current_index();
        // Captured here rather than at emit time: this is the earliest point at
        // which we know what is loading, so the media session can show real
        // metadata before a byte is decoded instead of a CDN path stem.
        self.pending_display = now_playing::PendingDisplay::from_song(&song);
        self.current_song = Some(song);
        true
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
