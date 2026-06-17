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
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::time::Duration;

use tauri::{Emitter, Runtime};
use tokio::sync::mpsc;
use tokio::sync::RwLock as TokioRwLock;
use tracing::{info, warn};

use crate::analysis::{self, AnalysisCommand};
use crate::decoder::{self, PlaybackSink};
use crate::error::{AudioError, AudioResult};
use crate::output::{self, LowLatencyOutput, OutputRenderClock};
use crate::types::*;
use crate::ws_server::WsServer;

mod automix;
mod mixer;
pub mod queue;

use automix::AutoMixManager;
use mixer::{CrossfadeParams, DeckId, DeckMixer};
use queue::PlaybackQueue;

// ── EventBuffer for session-scoped polling (kept for compat) ──

use std::collections::VecDeque;

pub struct EventBuffer {
    session_id: u64,
    events: VecDeque<AudioThreadEvent>,
    max_events: usize,
}

impl EventBuffer {
    pub fn new(session_id: u64) -> Self {
        Self {
            session_id,
            events: VecDeque::new(),
            max_events: 256,
        }
    }

    pub fn push(&mut self, event: AudioThreadEvent) {
        if self.events.len() >= self.max_events {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    pub fn reset(&mut self, session_id: u64) {
        self.session_id = session_id;
        self.events.clear();
    }

    pub fn drain(&mut self, session_id: u64) -> Vec<AudioThreadEvent> {
        if session_id != self.session_id {
            self.events.clear();
            return Vec::new();
        }
        self.events.drain(..).collect()
    }
}

// ── Public Player API ───────────────────────────────────────────

pub struct Player {
    msg_tx: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadMessage>>,
    seek_tx: mpsc::UnboundedSender<f64>,
    shared: Arc<PlayerShared>,
}

pub struct PlayerShared {
    pub state: AtomicU8,
    pub position_ms: AtomicU64,
    pub duration_ms: AtomicU64,
    pub event_buf: parking_lot::Mutex<EventBuffer>,
    /// Local-loopback WebSocket bridge for low-latency event delivery. Normal
    /// native playback uses this as the single event transport; Tauri emit is
    /// only used if the WS server failed to start.
    pub ws_server: parking_lot::Mutex<Option<Arc<WsServer>>>,
}

impl Player {
    pub fn new<R: Runtime>(app_handle: tauri::AppHandle<R>) -> AudioResult<Self> {
        let (msg_tx, msg_rx) = mpsc::unbounded_channel();
        let (seek_tx, seek_rx) = mpsc::unbounded_channel();
        let (evt_tx, mut evt_rx) =
            mpsc::unbounded_channel::<AudioThreadEventMessage<AudioThreadEvent>>();

        let shared = Arc::new(PlayerShared {
            state: AtomicU8::new(PlaybackState::Stopped as u8),
            position_ms: AtomicU64::new(0),
            duration_ms: AtomicU64::new(0),
            event_buf: parking_lot::Mutex::new(EventBuffer::new(0)),
            ws_server: parking_lot::Mutex::new(None),
        });

        // Start the WS server on Tauri's async runtime so its accept loop
        // outlives this function. (An earlier version built a throwaway
        // `current_thread` runtime and used `rt.block_on(...)` — when `rt`
        // dropped, the spawned listener task was aborted with it and no WS
        // connections ever succeeded.) Tauri's `async_runtime` is a long-
        // lived multi-thread tokio runtime; `block_on` enters it just long
        // enough to bind the listener, then `WsServer::start` spawns the
        // accept task on the same runtime via `tokio::spawn`.
        let player_handle_for_ws = PlayerHandle {
            msg_tx: msg_tx.clone(),
            seek_tx: seek_tx.clone(),
        };
        let ws_server = match tauri::async_runtime::block_on(WsServer::start(player_handle_for_ws))
        {
            Ok(server) => {
                let arc = Arc::new(server);
                info!(
                    "音频 WebSocket 服务器就绪: events={} control={}",
                    arc.event_ws_url(),
                    arc.control_ws_url()
                );
                Some(arc)
            }
            Err(e) => {
                warn!("音频 WebSocket 服务器启动失败 (将仅使用 Tauri 事件): {e:?}");
                None
            }
        };
        *shared.ws_server.lock() = ws_server.clone();

        // Forward events from the internal evt channel → WS broadcast (or
        // Tauri emit only when WS failed to start) + EventBuffer. We peek at
        // certain events to update `shared` atomics so `audio_get_state` can
        // return up-to-date values without going through the message loop.
        //
        // High-rate analysis events are coalesced before forwarding. Playback
        // controls/status events must not sit behind stale 2048-bin FFT JSON
        // frames in the WS queue; keeping only the latest FFT/lowFreq sample
        // preserves visual freshness while state/control events stay realtime.
        let shared_clone = Arc::clone(&shared);
        let app = app_handle.clone();
        let ws_for_forwarder = ws_server;
        let seq_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        tauri::async_runtime::spawn(async move {
            let forward_msg = |mut evt_msg: AudioThreadEventMessage<AudioThreadEvent>| {
                evt_msg.seq = seq_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                if let Some(event) = &evt_msg.data {
                    update_shared_from_event(&shared_clone, event);
                    shared_clone.event_buf.lock().push(event.clone());
                }
                if let Some(ws) = ws_for_forwarder.as_ref() {
                    ws.broadcast_event(&evt_msg);
                } else {
                    let _ = app.emit("audio-player://event", &evt_msg);
                }
            };

            while let Some(first_msg) = evt_rx.recv().await {
                let mut realtime = Vec::new();
                let mut latest_fft = None;
                let mut latest_low_freq = None;

                collect_forward_message(
                    first_msg,
                    &mut realtime,
                    &mut latest_fft,
                    &mut latest_low_freq,
                );

                while let Ok(next_msg) = evt_rx.try_recv() {
                    collect_forward_message(
                        next_msg,
                        &mut realtime,
                        &mut latest_fft,
                        &mut latest_low_freq,
                    );
                }

                for evt_msg in realtime {
                    forward_msg(evt_msg);
                }
                if let Some(evt_msg) = latest_fft {
                    forward_msg(evt_msg);
                }
                if let Some(evt_msg) = latest_low_freq {
                    forward_msg(evt_msg);
                }
            }
        });

        // Spawn the audio player on a dedicated thread with its own tokio runtime.
        // `AudioPlayer` owns the live `OutputStream` so it can reopen the stream
        // with a source-aware channel count when tracks change.
        std::thread::Builder::new()
            .name("audio-player".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Build tokio runtime");
                rt.block_on(async move {
                    let player = match AudioPlayer::new(msg_rx, seek_rx, evt_tx).await {
                        Ok(p) => p,
                        Err(e) => {
                            warn!("创建音频播放器失败：{e:?}");
                            return;
                        }
                    };
                    player.run().await;
                });
            })
            .map_err(|e| AudioError::ThreadError(format!("spawn audio-player thread: {e}")))?;

        Ok(Player {
            msg_tx,
            seek_tx,
            shared,
        })
    }

    pub fn handle(&self) -> PlayerHandle {
        PlayerHandle {
            msg_tx: self.msg_tx.clone(),
            seek_tx: self.seek_tx.clone(),
        }
    }

    pub fn send_msg(&self, msg: AudioThreadEventMessage<AudioThreadMessage>) -> AudioResult<()> {
        self.msg_tx
            .send(msg)
            .map_err(|_| AudioError::ThreadError("player channel closed".into()))
    }

    // ── Quick state accessors (for commands that need sync reads) ──

    pub fn state(&self) -> PlaybackState {
        PlaybackState::from_u8(self.shared.state.load(Ordering::Relaxed))
    }

    pub fn position(&self) -> f64 {
        self.shared.position_ms.load(Ordering::Relaxed) as f64 / 1000.0
    }

    pub fn duration(&self) -> f64 {
        self.shared.duration_ms.load(Ordering::Relaxed) as f64 / 1000.0
    }

    pub fn is_playing(&self) -> bool {
        self.state() == PlaybackState::Playing
    }

    pub fn poll_events(&self, session_id: u64) -> Vec<AudioThreadEvent> {
        self.shared.event_buf.lock().drain(session_id)
    }

    pub fn set_session(&self, session_id: u64) {
        self.shared.event_buf.lock().reset(session_id);
    }

    /// `ws://127.0.0.1:PORT` if the WebSocket bridge bound successfully,
    /// `None` if the bind failed (native playback cannot use WS controls).
    pub fn ws_url(&self) -> Option<String> {
        self.shared.ws_server.lock().as_ref().map(|s| s.ws_url())
    }

    pub fn ws_urls(&self) -> Option<(String, String)> {
        self.shared
            .ws_server
            .lock()
            .as_ref()
            .map(|s| (s.event_ws_url(), s.control_ws_url()))
    }
}

/// Update `PlayerShared` atomics from events we'd otherwise miss because
/// state transitions happen inside `AudioPlayer` without writing through
/// here. Keeps `audio_get_state` honest.
fn update_shared_from_event(shared: &Arc<PlayerShared>, event: &AudioThreadEvent) {
    match event {
        AudioThreadEvent::PlayStatus { is_playing } => {
            let s = if *is_playing {
                PlaybackState::Playing
            } else {
                PlaybackState::Paused
            };
            s.store(&shared.state);
        }
        AudioThreadEvent::PlayPosition { position } => {
            shared
                .position_ms
                .store((position * 1000.0).max(0.0) as u64, Ordering::Relaxed);
        }
        AudioThreadEvent::SyncStatus {
            is_playing,
            position,
            duration,
            ..
        } => {
            let s = if *is_playing {
                PlaybackState::Playing
            } else {
                PlaybackState::Paused
            };
            s.store(&shared.state);
            shared
                .position_ms
                .store((position * 1000.0).max(0.0) as u64, Ordering::Relaxed);
            shared
                .duration_ms
                .store((duration * 1000.0).max(0.0) as u64, Ordering::Relaxed);
        }
        AudioThreadEvent::AudioPlayFinished { .. } => {
            PlaybackState::Ended.store(&shared.state);
        }
        _ => {}
    }
}

fn collect_forward_message(
    msg: AudioThreadEventMessage<AudioThreadEvent>,
    realtime: &mut Vec<AudioThreadEventMessage<AudioThreadEvent>>,
    latest_fft: &mut Option<AudioThreadEventMessage<AudioThreadEvent>>,
    latest_low_freq: &mut Option<AudioThreadEventMessage<AudioThreadEvent>>,
) {
    let is_fft = matches!(&msg.data, Some(AudioThreadEvent::FFTData { .. }));
    let is_low_freq = matches!(&msg.data, Some(AudioThreadEvent::LowFrequencyVolume { .. }));

    if is_fft {
        *latest_fft = Some(msg);
    } else if is_low_freq {
        *latest_low_freq = Some(msg);
    } else {
        realtime.push(msg);
    }
}

fn spawn_output_low_freq_forwarder(
    output: &mut LowLatencyOutput,
    evt_sender: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
) {
    let Some(rx) = output.take_low_freq_rx() else {
        return;
    };

    let _ = std::thread::Builder::new()
        .name("audio-lowfreq".into())
        .spawn(move || {
            while let Ok(mut volume) = rx.recv() {
                while let Ok(next) = rx.try_recv() {
                    volume = next;
                }

                if evt_sender
                    .send(AudioThreadEventMessage::new(
                        String::new(),
                        Some(AudioThreadEvent::LowFrequencyVolume {
                            volume: volume as f64,
                        }),
                    ))
                    .is_err()
                {
                    break;
                }
            }
        });
}

// ── Cloneable handle for sending messages ────────────────────────

#[derive(Clone, Debug)]
pub struct PlayerHandle {
    msg_tx: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadMessage>>,
    seek_tx: mpsc::UnboundedSender<f64>,
}

impl PlayerHandle {
    pub fn send(&self, msg: AudioThreadEventMessage<AudioThreadMessage>) -> AudioResult<()> {
        if msg.callback_id.is_empty() {
            if let Some(AudioThreadMessage::SeekAudio { position }) = msg.data.as_ref() {
                return self.send_seek(*position);
            }
        }

        self.msg_tx
            .send(msg)
            .map_err(|_| AudioError::ThreadError("player channel closed".into()))
    }

    pub async fn send_anonymous(&self, msg: AudioThreadMessage) -> AudioResult<()> {
        self.send(AudioThreadEventMessage::new("".into(), Some(msg)))
    }

    pub fn send_seek(&self, position: f64) -> AudioResult<()> {
        self.seek_tx
            .send(position)
            .map_err(|_| AudioError::ThreadError("player seek channel closed".into()))
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Internal AudioPlayer
// ═══════════════════════════════════════════════════════════════════

struct AudioPlayer {
    // Channels
    msg_receiver: mpsc::UnboundedReceiver<AudioThreadEventMessage<AudioThreadMessage>>,
    seek_rx: mpsc::UnboundedReceiver<f64>,
    evt_sender: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
    // Self-message channel: lets `run()` re-enter `process_message` for
    // auto-advance, matching the AMLL reference's NextSongGapless pattern.
    self_msg_tx: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadMessage>>,
    self_msg_rx: mpsc::UnboundedReceiver<AudioThreadEventMessage<AudioThreadMessage>>,

    // CPAL playback. Decoding runs on a worker thread and pushes PCM blocks
    // into the deck mixer; the mixer pushes mixed PCM blocks to this output queue.
    output: LowLatencyOutput,
    deck_mixer: DeckMixer,
    active_deck: DeckId,
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
    secondary_decoder_handle: Option<decoder::DecoderHandle>,
    secondary_temp_file: Option<tempfile::TempPath>,
    secondary_local_path: Option<PathBuf>,
    secondary_song: Option<SongData>,
    secondary_duration: f64,
    secondary_display_info: Option<DisplayAudioInfo>,
    secondary_quality: Option<AudioQuality>,
    secondary_playback_id: Option<u64>,
    decoder_event_tx: mpsc::UnboundedSender<decoder::DecoderEvent>,
    decoder_event_rx: mpsc::UnboundedReceiver<decoder::DecoderEvent>,
    automix_prepare_tx: mpsc::UnboundedSender<automix::AutoMixPrepareResult>,
    automix_prepare_rx: mpsc::UnboundedReceiver<automix::AutoMixPrepareResult>,
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
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum PlaybackIntent {
    Playing,
    Paused,
}

#[derive(Debug)]
struct PlayerClock {
    base_position: f64,
    base_rendered_samples: u64,
    is_playing: bool,
    duration: f64,
    render_clock: Option<OutputRenderClock>,
    sample_rate: u32,
    channels: u16,
}

impl PlayerClock {
    fn new() -> Self {
        Self {
            base_position: 0.0,
            base_rendered_samples: 0,
            is_playing: false,
            duration: 0.0,
            render_clock: None,
            sample_rate: 44_100,
            channels: 2,
        }
    }

    fn set_render_clock(
        &mut self,
        render_clock: OutputRenderClock,
        sample_rate: u32,
        channels: u16,
    ) {
        let position = self.position();
        self.render_clock = Some(render_clock);
        self.sample_rate = sample_rate.max(1);
        self.channels = channels.max(1);
        self.base_position = self.clamp_position(position);
        self.base_rendered_samples = self.rendered_samples();
    }

    fn set_duration(&mut self, duration: f64) {
        self.duration = duration.max(0.0);
        self.base_position = self.clamp_position(self.base_position);
    }

    fn set_anchor(&mut self, is_playing: bool, position: f64) -> f64 {
        let position = self.clamp_position(position);
        self.base_position = position;
        self.base_rendered_samples = self.rendered_samples();
        self.is_playing = is_playing;
        position
    }

    fn position(&self) -> f64 {
        let position = if self.is_playing {
            let rendered_delta = self
                .rendered_samples()
                .saturating_sub(self.base_rendered_samples);
            let samples_per_second = self.sample_rate.max(1) as f64 * self.channels.max(1) as f64;
            self.base_position + rendered_delta as f64 / samples_per_second
        } else {
            self.base_position
        };
        self.clamp_position(position)
    }

    fn is_playing(&self) -> bool {
        self.is_playing
    }

    fn clamp_position(&self, position: f64) -> f64 {
        let position = normalize_seek_position(position);
        if self.duration > 0.0 {
            position.min(self.duration)
        } else {
            position
        }
    }

    fn rendered_samples(&self) -> u64 {
        self.render_clock
            .as_ref()
            .map(OutputRenderClock::rendered_samples)
            .unwrap_or(self.base_rendered_samples)
    }
}

fn normalize_seek_position(position: f64) -> f64 {
    if position.is_finite() {
        position.max(0.0)
    } else {
        0.0
    }
}

impl AudioPlayer {
    async fn new(
        msg_receiver: mpsc::UnboundedReceiver<AudioThreadEventMessage<AudioThreadMessage>>,
        seek_rx: mpsc::UnboundedReceiver<f64>,
        evt_sender: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
    ) -> AudioResult<Self> {
        let mut output = output::open_preferred_output(None).map_err(AudioError::Output)?;
        output.writer().set_paused(true);
        spawn_output_low_freq_forwarder(&mut output, evt_sender.clone());
        let deck_mixer = DeckMixer::new(output.writer(), output.config().channels);
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

        Ok(Self {
            msg_receiver,
            seek_rx,
            evt_sender,
            self_msg_tx,
            self_msg_rx,
            output,
            deck_mixer,
            active_deck: DeckId::Primary,
            automix,
            volume: 1.0,
            playback_intent: PlaybackIntent::Paused,
            clock,
            current_file_path: None,
            current_local_path: None,
            current_temp_file: None,
            current_decoder_handle: None,
            secondary_decoder_handle: None,
            secondary_temp_file: None,
            secondary_local_path: None,
            secondary_song: None,
            secondary_duration: 0.0,
            secondary_display_info: None,
            secondary_quality: None,
            secondary_playback_id: None,
            decoder_event_tx,
            decoder_event_rx,
            automix_prepare_tx,
            automix_prepare_rx,
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
        })
    }

    fn emitter(&self) -> EventEmitter {
        EventEmitter::new(self.evt_sender.clone())
    }

    fn ensure_output_for_source(&mut self, audio_info: &AudioInfo) -> anyhow::Result<bool> {
        let target = output::OutputTarget::for_source(audio_info.channels, audio_info.sample_rate);
        if self.output.target() == Some(target) {
            return Ok(false);
        }

        let mut output = output::open_preferred_output(Some(target)).map_err(AudioError::Output)?;
        output.writer().set_volume(self.volume as f32);
        output
            .writer()
            .set_paused(self.playback_intent == PlaybackIntent::Paused);
        spawn_output_low_freq_forwarder(&mut output, self.evt_sender.clone());
        let changed =
            output.config() != self.output.config() || output.device() != self.output.device();
        self.output = output;
        {
            let writer = self.output.writer();
            let config = self.output.config();
            self.clock.lock().set_render_clock(
                writer.render_clock(),
                config.sample_rate,
                config.channels,
            );
        }
        self.deck_mixer = DeckMixer::new(self.output.writer(), self.output.config().channels);
        self.active_deck = DeckId::Primary;
        self.secondary_playback_id = None;
        self.native_crossfade_generation = self.native_crossfade_generation.wrapping_add(1);
        self.native_crossfade_active = false;
        self.native_crossfade_transition_id = None;
        Ok(changed)
    }

    fn clock_position(&self) -> f64 {
        self.clock.lock().position()
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
        loop {
            tokio::select! {
              biased;

              seek = self.seek_rx.recv() => {
                if let Some(first_seek) = seek {
                  let seek_pos = self.drain_latest_seek(first_seek);
                  if let Err(err) = self.process_seek_position(seek_pos).await {
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

    fn drain_latest_seek(&mut self, first_seek: f64) -> f64 {
        let mut seek_pos = first_seek;
        while let Ok(next_seek) = self.seek_rx.try_recv() {
            seek_pos = next_seek;
        }
        seek_pos
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

    async fn process_seek_position(&mut self, position: f64) -> anyhow::Result<()> {
        let seek_pos = normalize_seek_position(position);
        self.cancel_native_automix_runtime().await;
        self.automix_prepare_generation = self.automix_prepare_generation.wrapping_add(1);
        let events = self.automix.cancel(self.current_play_index);
        self.emit_many(events).await;
        if let Some(handle) = &self.current_decoder_handle {
            self.deck_mixer.clear_all();
            handle.seek(Duration::from_secs_f64(seek_pos))?;
            let _ = self.analysis_tx.send(AnalysisCommand::Clear);
            let is_playing = self.playback_intent == PlaybackIntent::Playing;
            self.publish_position_anchor(is_playing, seek_pos).await;
        } else {
            warn!("找不到解码器句柄, 无法执行跳转");
        }
        Ok(())
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
                    self.output.writer().set_paused(false);
                    if let Some(handle) = &self.current_decoder_handle {
                        let _ = handle.set_paused(false);
                    }
                    if let Some(handle) = &self.secondary_decoder_handle {
                        let _ = handle.set_paused(false);
                    }
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
                        self.output.writer().set_paused(false);
                        if let Some(handle) = &self.current_decoder_handle {
                            let _ = handle.set_paused(false);
                        }
                        if let Some(handle) = &self.secondary_decoder_handle {
                            let _ = handle.set_paused(false);
                        }
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
                AudioThreadMessage::SeekAudio { position } => {
                    self.process_seek_position(*position).await?;
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
                AudioThreadMessage::SetFFT { enabled } => {
                    if !enabled {
                        let _ = self.analysis_tx.send(AnalysisCommand::Clear);
                    }
                }
                AudioThreadMessage::SetFFTRange { from_freq, to_freq } => {
                    let _ = self.analysis_tx.send(AnalysisCommand::SetFreqRange {
                        from: *from_freq,
                        to: *to_freq,
                    });
                }
                AudioThreadMessage::SetAudioOutput { .. } => {
                    warn!("SetAudioOutput 尚未实现");
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
                    self.automix_prepare_generation =
                        self.automix_prepare_generation.wrapping_add(1);
                    let generation = self.automix_prepare_generation;
                    let current_song = self.current_song.clone();
                    let current_duration = Some(self.current_audio_info.read().await.duration)
                        .filter(|duration| *duration > 0.0);
                    let (events, request) = self.automix.begin_prepare_next(
                        generation,
                        *transition_id,
                        *current_index,
                        *next_index,
                        current_song,
                        current_duration,
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
            let result = match tokio::time::timeout(Duration::from_secs(10), task).await {
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
        if let Some(start_time) = self.automix.status(status_index).crossfade_start {
            self.schedule_native_automix_trigger(start_time);
        }
        self.emit_many(events).await;
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
        self.deck_mixer.set_deck_gain(self.active_deck, 1.0);
        self.deck_mixer.set_deck_gain(self.inactive_deck(), 0.0);
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
        incoming_writer.set_paused(self.playback_intent == PlaybackIntent::Paused);
        self.deck_mixer.set_deck_gain(incoming_deck, 0.0);

        if let Some(handle) = self.secondary_decoder_handle.take() {
            handle.stop();
        }

        let analysis_tx_for_open = self.analysis_tx.clone();
        let path_for_open = prepared.local_path.clone();
        let output_config = self.output.config();
        let playback_id = self.decoder_playback_id.wrapping_add(1);
        let decoder_event_tx = self.decoder_event_tx.clone();
        let start_paused = self.playback_intent == PlaybackIntent::Paused;

        let open_result = tokio::task::spawn_blocking(move || {
            decoder::spawn_playback_decoder(
                &path_for_open,
                Some(0.0),
                incoming_writer,
                output_config.channels,
                output_config.sample_rate,
                analysis_tx_for_open,
                decoder_event_tx,
                playback_id,
                start_paused,
            )
        })
        .await?;

        let incoming_handle = open_result.map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let duration = prepared.plan.as_ref().map(|p| p.duration).unwrap_or(2.0);

        self.secondary_decoder_handle = Some(incoming_handle);
        self.secondary_local_path = Some(prepared.local_path.clone());
        self.secondary_temp_file = prepared._temp_file;
        self.secondary_song = Some(prepared.song.clone());
        self.secondary_duration = prepared.analysis_duration;
        self.secondary_display_info = Some(prepared.display_info.clone());
        self.secondary_quality = Some(prepared.quality.clone());
        self.secondary_playback_id = Some(playback_id);
        self.native_crossfade_active = true;
        self.native_crossfade_transition_id = prepared.transition_id;

        let crossfade_params = prepared
            .plan
            .as_ref()
            .map(|plan| CrossfadeParams {
                curve: plan.curve,
                incoming_gain: plan.incoming_gain_adjustment as f32,
                outgoing_gain: 1.0,
                overlap_headroom_db: plan.overlap_headroom_db as f32,
            })
            .unwrap_or_default();

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
        self.output
            .writer()
            .set_paused(self.playback_intent == PlaybackIntent::Paused);

        // `initial_position` is applied inside the decoder worker before it
        // starts pushing PCM, avoiding a separate post-load seek round trip.
        let analysis_tx_for_open = self.analysis_tx.clone();
        let path_for_open = local_path.clone();
        let output_writer = self.deck_mixer.primary_writer();
        let output_config = self.output.config();
        self.decoder_playback_id = self.decoder_playback_id.wrapping_add(1);
        let playback_id = self.decoder_playback_id;
        let decoder_event_tx = self.decoder_event_tx.clone();
        let start_paused = self.playback_intent == PlaybackIntent::Paused;
        let seek_into_open = initial_position.filter(|p| *p > 0.0);

        let open_result = tokio::task::spawn_blocking(move || {
            decoder::spawn_playback_decoder(
                &path_for_open,
                seek_into_open,
                output_writer,
                output_config.channels,
                output_config.sample_rate,
                analysis_tx_for_open,
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
        // The analysis thread exits on its own once every `AnalysisCommand`
        // sender drops (we get `RecvTimeoutError::Disconnected`). Dropping the
        // JoinHandle here detaches the thread; it terminates cleanly within the
        // next `recv_timeout` interval (50 ms).
        let _ = self.analysis_thread.take();
    }
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
