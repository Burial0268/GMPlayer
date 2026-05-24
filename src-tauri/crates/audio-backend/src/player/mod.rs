/// AMLL-style message-driven audio player.
///
/// Architecture (mirrors `amll-player-core`):
/// - `Player` — public API: sends `AudioThreadMessage` to the internal player thread.
/// - `PlayerHandle` — cloneable handle for sending messages from anywhere.
/// - `AudioPlayer` — internal event loop that processes messages + emits events.
///
/// Message flow:  frontend → invoke → Player::send_msg() → AudioPlayer → rodio
/// Event flow:   AudioPlayer → callback → app.emit("audio-player://event", ...) → frontend
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use rodio::{OutputStream, OutputStreamHandle, Sink};
use tauri::{Emitter, Runtime};
use tokio::sync::RwLock as TokioRwLock;
use tokio::sync::mpsc;
use tracing::{info, warn};

use cpal::SampleRate;
use cpal::traits::{DeviceTrait, HostTrait};

use crate::analysis::{self, AnalysisCommand};
use crate::decoder;
use crate::error::{AudioError, AudioResult};
use crate::types::*;
use crate::ws_server::WsServer;

pub mod queue;

// ── Output stream helper ────────────────────────────────────────
//
// Windows commonly reports the default output device as 5.1 / 7.1 at
// 48 kHz (Realtek HD Audio, HDMI passthrough, Spatial Audio). If we let
// rodio fall back to `OutputStream::try_default()` we inherit that config,
// and rodio's `UniformSourceIterator` upmixes our stereo source inside
// the audio callback — roughly 4× the per-callback CPU cost. Fine when
// the app has foreground priority; when Windows EcoQoS deprioritises the
// thread in the background, the callback misses its WASAPI deadline and
// audio stutters.
//
// We pick a stereo config (preferring 44.1 / 48 kHz, F32 format) so the
// adapter does no channel work and minimal rate conversion. If the device
// exposes no stereo config at all, fall back to its default — caller
// still gets audio, just without the CPU savings.
fn open_preferred_output() -> Result<(OutputStream, OutputStreamHandle), String> {
  const PREFERRED_RATES: [u32; 2] = [44_100, 48_000];
  const PREFERRED_FORMATS: [cpal::SampleFormat; 3] = [
    cpal::SampleFormat::F32,
    cpal::SampleFormat::I16,
    cpal::SampleFormat::U16,
  ];

  let host = cpal::default_host();
  let device = host
    .default_output_device()
    .ok_or_else(|| "no default output device".to_string())?;
  let device_name = device.name().unwrap_or_else(|_| "<unknown>".into());

  let chosen = match device.supported_output_configs() {
    Ok(configs) => {
      let stereo: Vec<_> = configs.filter(|c| c.channels() == 2).collect();

      // First pass: stereo + preferred rate + preferred format.
      let mut result: Option<cpal::SupportedStreamConfig> = None;
      'outer: for fmt in PREFERRED_FORMATS {
        for range in &stereo {
          if range.sample_format() != fmt {
            continue;
          }
          for rate in PREFERRED_RATES {
            let sr = SampleRate(rate);
            if range.min_sample_rate() <= sr && sr <= range.max_sample_rate() {
              result = Some(range.clone().with_sample_rate(sr));
              break 'outer;
            }
          }
        }
      }

      // Second pass: stereo at range max if no preferred rate matched.
      if result.is_none() {
        'fallback: for fmt in PREFERRED_FORMATS {
          for range in &stereo {
            if range.sample_format() == fmt {
              result = Some(range.clone().with_max_sample_rate());
              break 'fallback;
            }
          }
        }
      }

      result
    }
    Err(e) => {
      warn!("supported_output_configs 失败 (device={device_name}): {e:?} — 退回默认");
      None
    }
  };

  let config = match chosen {
    Some(c) => c,
    None => {
      warn!("设备 {device_name} 无 stereo 配置 — 退回 default_output_config");
      device
        .default_output_config()
        .map_err(|e| format!("default_output_config: {e:?}"))?
    }
  };

  info!(
    "音频输出：device={device_name} channels={} rate={} format={:?}",
    config.channels(),
    config.sample_rate().0,
    config.sample_format()
  );

  OutputStream::try_from_device_config(&device, config)
    .map_err(|e| format!("try_from_device_config: {e:?}"))
}

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
  shared: Arc<PlayerShared>,
}

pub struct PlayerShared {
  pub state: AtomicU8,
  pub position_ms: AtomicU64,
  pub duration_ms: AtomicU64,
  pub event_buf: parking_lot::Mutex<EventBuffer>,
  /// Local-loopback WebSocket bridge for low-latency event delivery. The
  /// audio thread broadcasts events through `WsServer::broadcast_event`
  /// (sub-millisecond, decoupled from Tauri IPC). Tauri emit still fires
  /// in parallel as a fallback while the WS client connects or for older
  /// listeners that don't speak the WS protocol.
  pub ws_server: parking_lot::Mutex<Option<Arc<WsServer>>>,
}

impl Player {
  pub fn new<R: Runtime>(app_handle: tauri::AppHandle<R>) -> AudioResult<Self> {
    let (msg_tx, msg_rx) = mpsc::unbounded_channel();
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
    };
    let ws_server = match tauri::async_runtime::block_on(WsServer::start(player_handle_for_ws)) {
      Ok(server) => {
        let arc = Arc::new(server);
        info!("音频 WebSocket 服务器就绪: {}", arc.ws_url());
        Some(arc)
      }
      Err(e) => {
        warn!("音频 WebSocket 服务器启动失败 (将仅使用 Tauri 事件): {e:?}");
        None
      }
    };
    *shared.ws_server.lock() = ws_server.clone();

    // Forward events from the internal evt channel → Tauri emit + WS
    // broadcast + EventBuffer. We peek at certain events to update
    // `shared` atomics so the sync `audio_get_state` command can return
    // up-to-date values without going through the message loop.
    //
    // Every outbound event gets a monotonic `seq` stamp. The frontend
    // subscribes to BOTH transports (WebSocket as primary, Tauri channel
    // as safety net during WS reconnect), and without seq it processes
    // each event twice — which breaks state-flip dedup on bursts like
    // Pause → Seek → Resume (the late transport replays the intermediate
    // `PlayStatus(false)` after state has settled on playing, flipping it
    // back and triggering a spurious `play` toast on the recovery to
    // `true`). With seq, the frontend drops events it has already seen.
    let shared_clone = Arc::clone(&shared);
    let app = app_handle.clone();
    let ws_for_forwarder = ws_server;
    let seq_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    tauri::async_runtime::spawn(async move {
      while let Some(mut evt_msg) = evt_rx.recv().await {
        let seq = seq_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        evt_msg.seq = seq;
        if let Some(event) = &evt_msg.data {
          update_shared_from_event(&shared_clone, event);
          shared_clone.event_buf.lock().push(event.clone());
        }
        if let Some(ws) = ws_for_forwarder.as_ref() {
          ws.broadcast_event(&evt_msg);
        }
        let _ = app.emit("audio-player://event", &evt_msg);
      }
    });

    // Spawn the audio player on a dedicated thread with its own tokio runtime.
    // The `OutputStream` is kept on this thread so it lives for the
    // duration of playback (rodio drops the stream when its handle is
    // dropped, which would silence the audio device).
    std::thread::Builder::new()
      .name("audio-player".into())
      .spawn(move || {
        let (stream, stream_handle) = match open_preferred_output() {
          Ok(s) => s,
          Err(e) => {
            warn!("无法打开默认音频输出设备：{e:?}");
            return;
          }
        };
        // Keep `stream` alive for the lifetime of this thread.
        let _stream = stream;

        let rt = tokio::runtime::Builder::new_current_thread()
          .enable_all()
          .build()
          .expect("Build tokio runtime");
        rt.block_on(async move {
          let player = match AudioPlayer::new(msg_rx, evt_tx, stream_handle).await {
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

    Ok(Player { msg_tx, shared })
  }

  pub fn handle(&self) -> PlayerHandle {
    PlayerHandle {
      msg_tx: self.msg_tx.clone(),
    }
  }

  pub fn send_msg(&self, msg: AudioThreadEventMessage<AudioThreadMessage>) -> AudioResult<()> {
    self
      .msg_tx
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
  /// `None` if the bind failed (frontend will fall back to Tauri events).
  pub fn ws_url(&self) -> Option<String> {
    self.shared.ws_server.lock().as_ref().map(|s| s.ws_url())
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

// ── Cloneable handle for sending messages ────────────────────────

#[derive(Clone, Debug)]
pub struct PlayerHandle {
  msg_tx: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadMessage>>,
}

impl PlayerHandle {
  pub fn send(&self, msg: AudioThreadEventMessage<AudioThreadMessage>) -> AudioResult<()> {
    self
      .msg_tx
      .send(msg)
      .map_err(|_| AudioError::ThreadError("player channel closed".into()))
  }

  pub async fn send_anonymous(&self, msg: AudioThreadMessage) -> AudioResult<()> {
    self
      .msg_tx
      .send(AudioThreadEventMessage::new("".into(), Some(msg)))
      .map_err(|_| AudioError::ThreadError("player channel closed".into()))
  }
}

// ═══════════════════════════════════════════════════════════════════
//  Internal AudioPlayer
// ═══════════════════════════════════════════════════════════════════

struct AudioPlayer {
  // Channels
  msg_receiver: mpsc::UnboundedReceiver<AudioThreadEventMessage<AudioThreadMessage>>,
  evt_sender: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
  // Self-message channel: lets `run()` re-enter `process_message` for
  // auto-advance, matching the AMLL reference's NextSongGapless pattern.
  self_msg_tx: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadMessage>>,
  self_msg_rx: mpsc::UnboundedReceiver<AudioThreadEventMessage<AudioThreadMessage>>,

  // Rodio playback
  sink: Arc<Sink>,
  stream_handle: OutputStreamHandle,
  volume: f64,
  /// What the user wants the sink to be doing. Used so auto-advance and
  /// seek transitions preserve the playing/paused state even though the
  /// underlying sink momentarily becomes empty/stopped during the swap.
  playback_intent: PlaybackIntent,

  // Current track
  current_file_path: Option<String>,
  /// Resolved local path: same as `current_file_path` for local files, or
  /// the temp-file path created when streaming a remote URL.
  current_local_path: Option<PathBuf>,
  /// RAII guard for the downloaded temp file — dropped when a new track
  /// loads, which deletes the temp file from disk.
  current_temp_file: Option<tempfile::TempPath>,
  current_decoder_handle: Option<decoder::DecoderHandle>,

  /// Sample counter from the current track's `FFTFeedSource`, paired with
  /// `samples_per_sec` (source rate × channels). Shared with the position
  /// task so it can read the latest counter on each heartbeat. Tuple is
  /// replaced atomically on each `start_playing_song`; cleared on stop.
  current_samples_counter: Arc<TokioRwLock<Option<(Arc<AtomicU64>, f64)>>>,

  // Playlist
  playlist: Vec<SongData>,
  playlist_inited: bool,
  current_play_index: usize,
  current_song: Option<SongData>,

  // Shared state snapshots
  current_audio_info: Arc<TokioRwLock<DisplayAudioInfo>>,
  current_position: Arc<TokioRwLock<f64>>,
  current_audio_quality: Arc<TokioRwLock<AudioQuality>>,

  // Position tracking channel — payload is (is_playing, base_time_secs)
  play_pos_sx: mpsc::UnboundedSender<(bool, f64)>,

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

impl AudioPlayer {
  async fn new(
    msg_receiver: mpsc::UnboundedReceiver<AudioThreadEventMessage<AudioThreadMessage>>,
    evt_sender: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
    stream_handle: OutputStreamHandle,
  ) -> AudioResult<Self> {
    let sink = Arc::new(
      Sink::try_new(&stream_handle).map_err(|e| AudioError::Output(format!("Create sink: {e}")))?,
    );
    sink.pause();

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
    // The frontend extrapolates intermediate position locally (last
    // event + elapsed wall time). The Rust side only needs to emit on
    // (a) state transitions / seeks — pushed through `play_pos_sx` — and
    // (b) a 1 Hz heartbeat using the sample counter. This drops the
    // event rate from 62.5 Hz to ~1 Hz and eliminates IPC backpressure
    // that caused stutter when the window wasn't focused.
    let position_writer = current_position.clone();
    let audio_info_reader = current_audio_info.clone();
    let samples_counter_reader: Arc<TokioRwLock<Option<(Arc<AtomicU64>, f64)>>> =
      Arc::new(TokioRwLock::new(None));
    let samples_counter_for_player = samples_counter_reader.clone();
    let emitter_pos = EventEmitter::new(evt_sender.clone());
    let (play_pos_sx, mut play_pos_rx) = mpsc::unbounded_channel::<(bool, f64)>();

    tasks.push(tokio::task::spawn(async move {
      let mut time_it = tokio::time::interval(Duration::from_secs(1));
      time_it.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

      let mut is_playing = false;
      let mut base_time = 0.0;

      loop {
        tokio::select! {
          msg = play_pos_rx.recv() => {
            match msg {
              Some((new_is_playing, new_base_time)) => {
                is_playing = new_is_playing;
                base_time = new_base_time;
                // CRITICAL: reset the sample counter on every anchor change.
                // The counter is an absolute count of samples yielded by
                // FFTFeedSource since track open — it's never reset on
                // pause/play. If we didn't reset here, after pause→play we'd
                // compute `position = base_time + abs_counter/sps` which
                // double-counts: e.g. paused at 60s with counter=60s worth
                // of samples, then play → base_time=60, counter still=60s,
                // heartbeat → position=120. Same applies to seeks where
                // FFTFeedSource::try_seek may not have run yet (paused sink
                // doesn't pull, so the source's try_seek is deferred to the
                // first pull). Resetting here makes the counter mean
                // "samples since anchor", which is what the heartbeat
                // formula expects.
                if let Some((counter, _)) = samples_counter_reader.read().await.as_ref() {
                  counter.store(0, Ordering::SeqCst);
                }
                *position_writer.write().await = base_time;
                // Immediate emit so the frontend snaps on play/pause/seek
                // without waiting for the 1-second tick.
                let _ = emitter_pos
                  .emit(AudioThreadEvent::PlayPosition { position: base_time })
                  .await;
              }
              None => break,
            }
          }
          _ = time_it.tick() => {
            if is_playing {
              let duration = audio_info_reader.read().await.duration;
              if duration > 0.0 {
                // `samples_count` was reset above on every anchor change, so
                // it represents samples-since-anchor. position = base_time
                // + samples / (rate × channels).
                let played = if let Some((counter, sps)) =
                  samples_counter_reader.read().await.as_ref()
                {
                  let samples = counter.load(Ordering::Relaxed) as f64;
                  if *sps > 0.0 { samples / *sps } else { 0.0 }
                } else {
                  0.0
                };
                let current_pos = (base_time + played).min(duration);
                *position_writer.write().await = current_pos;
                let _ = emitter_pos
                  .emit(AudioThreadEvent::PlayPosition { position: current_pos })
                  .await;
              }
            }
          }
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

    Ok(Self {
      msg_receiver,
      evt_sender,
      self_msg_tx,
      self_msg_rx,
      sink,
      stream_handle,
      volume: 1.0,
      playback_intent: PlaybackIntent::Paused,
      current_file_path: None,
      current_local_path: None,
      current_temp_file: None,
      current_decoder_handle: None,
      current_samples_counter: samples_counter_for_player,
      playlist: Vec::new(),
      playlist_inited: false,
      current_play_index: 0,
      current_song: None,
      current_audio_info,
      current_position,
      current_audio_quality,
      play_pos_sx,
      tasks,
      analysis_tx,
      analysis_thread,
    })
  }

  fn emitter(&self) -> EventEmitter {
    EventEmitter::new(self.evt_sender.clone())
  }

  async fn sync_ui(&self) {
    let audio_info = self.current_audio_info.read().await.clone();
    let position = *self.current_position.read().await;
    let is_playing = !self.sink.is_paused() && !self.sink.empty();
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
    let mut check_end_interval = tokio::time::interval(Duration::from_millis(50));
    check_end_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
      tokio::select! {
        biased;

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

        _ = check_end_interval.tick() => {
          if self.sink.empty() && !self.sink.is_paused() && self.current_song.is_some() {
            // Emit AudioPlayFinished BEFORE clearing current_song so the
            // music_id is preserved.
            let finished_id = self.current_song.as_ref().map(|s| s.get_id()).unwrap_or_default();
            let _ = self.play_pos_sx.send((false, 0.0));
            self.current_song = None;
            let _ = self.emitter().emit(AudioThreadEvent::AudioPlayFinished {
              music_id: finished_id,
            }).await;

            // Auto-advance via a NextSongGapless self-message — the
            // reference architecture does the same so all start-playing
            // bookkeeping lives in one path (process_message).
            if !self.playlist.is_empty() {
              let _ = self.self_msg_tx.send(AudioThreadEventMessage::new(
                "".into(),
                Some(AudioThreadMessage::NextSongGapless),
              ));
            }
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

  async fn process_message(
    &mut self,
    msg: AudioThreadEventMessage<AudioThreadMessage>,
  ) -> anyhow::Result<()> {
    let emitter = self.emitter();
    if let Some(ref data) = msg.data {
      match data {
        AudioThreadMessage::ResumeAudio => {
          self.playback_intent = PlaybackIntent::Playing;
          self.sink.play();
          let current_pos = *self.current_position.read().await;
          let _ = self.play_pos_sx.send((true, current_pos));
          let _ = emitter
            .emit(AudioThreadEvent::PlayStatus { is_playing: true })
            .await;
        }
        AudioThreadMessage::PauseAudio => {
          self.playback_intent = PlaybackIntent::Paused;
          self.sink.pause();
          let current_pos = *self.current_position.read().await;
          let _ = self.play_pos_sx.send((false, current_pos));
          let _ = emitter
            .emit(AudioThreadEvent::PlayStatus { is_playing: false })
            .await;
        }
        AudioThreadMessage::ResumeOrPauseAudio => {
          let was_paused = self.sink.is_paused();
          if was_paused {
            self.sink.play();
            self.playback_intent = PlaybackIntent::Playing;
          } else {
            self.sink.pause();
            self.playback_intent = PlaybackIntent::Paused;
          }
          let current_pos = *self.current_position.read().await;
          let _ = self.play_pos_sx.send((was_paused, current_pos));
          let _ = emitter
            .emit(AudioThreadEvent::PlayStatus {
              is_playing: was_paused,
            })
            .await;
        }
        AudioThreadMessage::SeekAudio { position } => {
          if let Some(local_path) = self.current_local_path.clone() {
            let seek_pos = position.max(0.0);
            let seek_duration = Duration::from_secs_f64(seek_pos);

            // Always clear the analyzer's PCM queue first so visualization
            // doesn't show pre-seek samples after the seek completes.
            let _ = self.analysis_tx.send(AnalysisCommand::Clear);

            // Fast path: in-place seek through the existing Symphonia-backed
            // source. Avoids re-downloading / re-decoding.
            let in_place_result = self.sink.try_seek(seek_duration);

            match in_place_result {
              Ok(()) => {
                let is_playing = self.playback_intent == PlaybackIntent::Playing;
                // Synchronously update current_position BEFORE the play_pos
                // message is sent — otherwise `finish_message → sync_ui`
                // could read stale 0 here (the position task hasn't yet
                // polled play_pos_rx in this runtime tick), and the
                // resulting SyncStatus overwrites the frontend's optimistic
                // position to 0.
                *self.current_position.write().await = seek_pos;
                let _ = self.play_pos_sx.send((is_playing, seek_pos));
                let _ = emitter
                  .emit(AudioThreadEvent::PlayStatus { is_playing })
                  .await;
              }
              Err(e) => {
                // Fallback: recreate the source at the new position.
                // Slower (decodes-and-discards via skip_duration) but works
                // for formats whose decoder doesn't implement try_seek.
                warn!("In-place seek failed ({e:?})，回退到 re-decode 路径");

                let analysis_tx_for_open = self.analysis_tx.clone();
                let path_for_open = local_path.clone();
                let open_result = tokio::task::spawn_blocking(move || {
                  decoder::open_source_with_fft_at(&path_for_open, seek_pos, analysis_tx_for_open)
                })
                .await?;

                match open_result {
                  Ok((source, _info, handle, samples_count)) => {
                    let samples_per_sec = self
                      .current_samples_counter
                      .read()
                      .await
                      .as_ref()
                      .map(|(_, sps)| *sps)
                      .unwrap_or(0.0);
                    self.sink.stop();
                    let new_sink = Arc::new(
                      Sink::try_new(&self.stream_handle)
                        .map_err(|e| anyhow::anyhow!("Create sink in seek: {e}"))?,
                    );
                    new_sink.set_volume(self.volume as f32);
                    // Pause BEFORE append so we don't leak a burst of audio
                    // when the intent is to stay paused.
                    if self.playback_intent == PlaybackIntent::Paused {
                      new_sink.pause();
                    }
                    new_sink.append(source);
                    self.sink = new_sink;
                    self.current_decoder_handle = Some(handle);
                    // New source ⇒ new counter; preserve samples_per_sec
                    // (same source rate/channels). If unknown (shouldn't
                    // happen here because we already had a track), fall
                    // back to 44100·2 to avoid div-by-zero in the task.
                    let sps = if samples_per_sec > 0.0 { samples_per_sec } else { 88_200.0 };
                    *self.current_samples_counter.write().await = Some((samples_count, sps));

                    let is_playing = self.playback_intent == PlaybackIntent::Playing;
                    // Same sync-write as the Ok(()) path above.
                    *self.current_position.write().await = seek_pos;
                    let _ = self.play_pos_sx.send((is_playing, seek_pos));
                    let _ = emitter
                      .emit(AudioThreadEvent::PlayStatus { is_playing })
                      .await;
                  }
                  Err(e) => {
                    warn!("Seek 时重新创建解码器失败: {e:?}");
                    let _ = emitter
                      .emit(AudioThreadEvent::PlayError {
                        error: format!("Seek failed: {e}"),
                      })
                      .await;
                  }
                }
              }
            }
          } else {
            warn!("找不到本地文件路径, 无法执行跳转");
          }
        }
        AudioThreadMessage::SetVolume { volume } => {
          self.volume = volume.clamp(0.0, 1.0);
          self.sink.set_volume(self.volume as f32);
          let _ = emitter
            .emit(AudioThreadEvent::VolumeChanged {
              volume: self.volume,
            })
            .await;
        }
        AudioThreadMessage::SetVolumeRelative { volume } => {
          self.volume = (self.volume + volume).clamp(0.0, 1.0);
          self.sink.set_volume(self.volume as f32);
          let _ = emitter
            .emit(AudioThreadEvent::VolumeChanged {
              volume: self.volume,
            })
            .await;
        }
        AudioThreadMessage::NextSong => {
          if self.playlist.is_empty() {
            return self.finish_message(msg).await;
          }
          self.current_play_index = (self.current_play_index + 1) % self.playlist.len();
          self.current_song = self.playlist.get(self.current_play_index).cloned();
          self.start_playing_song(true, None).await?;
        }
        AudioThreadMessage::NextSongGapless => {
          if self.playlist.is_empty() {
            return self.finish_message(msg).await;
          }
          self.current_play_index = (self.current_play_index + 1) % self.playlist.len();
          self.current_song = self.playlist.get(self.current_play_index).cloned();
          self.start_playing_song(true, None).await?;
        }
        AudioThreadMessage::PrevSong => {
          if self.playlist.is_empty() {
            return self.finish_message(msg).await;
          }
          self.current_play_index = self
            .current_play_index
            .checked_sub(1)
            .unwrap_or(self.playlist.len() - 1);
          self.current_song = self.playlist.get(self.current_play_index).cloned();
          self.start_playing_song(true, None).await?;
        }
        AudioThreadMessage::JumpToSong { song_index } => {
          if let Some(song) = self.playlist.get(*song_index).cloned() {
            self.current_play_index = *song_index;
            self.current_song = Some(song);
            self.start_playing_song(true, None).await?;
          }
        }
        AudioThreadMessage::JumpToSongAt {
          song_index,
          position,
        } => {
          if let Some(song) = self.playlist.get(*song_index).cloned() {
            self.current_play_index = *song_index;
            self.current_song = Some(song);
            self.start_playing_song(true, Some(*position)).await?;
          }
        }
        AudioThreadMessage::SetPlaylist { songs } => {
          self.playlist = songs.clone();
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
      }
    }
    self.finish_message(msg).await
  }

  /// Ack the request (so the frontend's callback_id pairing resolves) and
  /// return. Specific state-change events (PlayStatus, PlayPosition,
  /// VolumeChanged, PlayListChanged) are emitted at the point of change.
  /// SyncStatus snapshots are only emitted on explicit `SyncStatus`
  /// requests or after `start_playing_song` — emitting one per command
  /// caused a race: e.g. SetVolume → finish_message → sync_ui's read of
  /// `sink.is_paused()` returned true (because the follow-on ResumeAudio
  /// hadn't been processed yet), so the SyncStatus carried isPlaying=false
  /// and clobbered the frontend's optimistic "playing" state — triggering
  /// a duplicate `"play"` event when the eventual PlayStatus(true) arrived.
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
      self.sink.stop();

      let _ = self.analysis_tx.send(AnalysisCommand::Clear);

      let new_sink = Arc::new(
        Sink::try_new(&self.stream_handle)
          .map_err(|e| anyhow::anyhow!("Create sink: {e}"))?,
      );
      // Honour playback intent BEFORE the source is appended so a paused
      // intent never leaks any audio.
      if self.playback_intent == PlaybackIntent::Paused {
        new_sink.pause();
      }
      new_sink.set_volume(self.volume as f32);
      self.sink = new_sink;
      self.current_decoder_handle = None;
      // Drop the previous temp file (if any) by clearing the guard. This
      // must happen BEFORE we assign the new temp path so disk usage stays
      // bounded.
      self.current_local_path = None;
      self.current_temp_file = None;
    }

    self.current_file_path = Some(file_path.clone());

    // Resolve URL → local path (download to temp) so rodio's File-based
    // decoder can read it.
    let resolve_path = file_path.clone();
    let resolve_result =
      tokio::task::spawn_blocking(move || -> AudioResult<(PathBuf, Option<tempfile::TempPath>)> {
        if decoder::is_http_url(&resolve_path) {
          let temp = decoder::download_to_temp_path(&resolve_path)?;
          let path = temp.to_path_buf();
          Ok((path, Some(temp)))
        } else {
          Ok((PathBuf::from(&resolve_path), None))
        }
      })
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

    // `initial_position` (Some) triggers `open_source_with_fft_at`, which
    // bundles the seek into the load. This avoids the prior race where a
    // follow-up `SeekAudio` message would race the position task: stale
    // current_position was read by `sync_ui`'s SyncStatus, which overwrote
    // the frontend's optimistic position on startup-resume.
    let analysis_tx_for_open = self.analysis_tx.clone();
    let path_for_open = local_path.clone();
    let seek_into_open = initial_position.filter(|p| *p > 0.0);
    let open_result = if let Some(seek_pos) = seek_into_open {
      tokio::task::spawn_blocking(move || {
        decoder::open_source_with_fft_at(&path_for_open, seek_pos, analysis_tx_for_open)
      })
      .await?
    } else {
      tokio::task::spawn_blocking(move || {
        decoder::open_source_with_fft(&path_for_open, analysis_tx_for_open)
      })
      .await?
    };

    let (source, audio_info, handle, samples_count) = match open_result {
      Ok(t) => t,
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
    // Pre-publish the samples counter so the position task can read it
    // as soon as the sink starts pulling samples. samples_per_sec uses
    // the SOURCE's reported rate/channels — rodio's resampler sits AFTER
    // our FFTFeedSource, so the counter ticks at source rate.
    let samples_per_sec =
      (audio_info.sample_rate as f64).max(1.0) * (audio_info.channels.max(1) as f64);
    *self.current_samples_counter.write().await = Some((samples_count, samples_per_sec));

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
      bitrate: audio_info
        .bitrate_bps
        .map(|b| b as u32)
        .unwrap_or_default(),
    };

    *self.current_audio_info.write().await = display_info.clone();
    *self.current_audio_quality.write().await = quality.clone();
    // Synchronously write current_position so `sync_ui` (called below)
    // reads the correct anchor, even if the position task hasn't yet
    // polled the play_pos message we send next.
    *self.current_position.write().await = anchor_pos;

    self.sink.append(source);

    let is_now_playing = self.playback_intent == PlaybackIntent::Playing;
    let _ = self.play_pos_sx.send((is_now_playing, anchor_pos));

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
    // sender drops (we get `RecvTimeoutError::Disconnected`). Rust's
    // field-drop order handles that: the `sink` (carrying a
    // `FFTFeedSource` with an `analysis_tx` clone) and `analysis_tx`
    // both drop right after this `Drop` impl returns. Dropping the
    // JoinHandle here detaches the thread; it terminates cleanly within
    // the next `recv_timeout` interval (50 ms).
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
    self
      .evt_sender
      .send(AudioThreadEventMessage::new("".into(), Some(event)))
      .map_err(|_| anyhow::anyhow!("event channel closed"))?;
    Ok(())
  }

  async fn ret_none(
    &self,
    req: AudioThreadEventMessage<AudioThreadMessage>,
  ) -> anyhow::Result<()> {
    self
      .evt_sender
      .send(req.to_none::<AudioThreadEvent>())
      .map_err(|_| anyhow::anyhow!("event channel closed"))?;
    Ok(())
  }
}
