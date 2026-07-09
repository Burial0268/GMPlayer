use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;

use tauri::ipc::Channel;
use tauri::{Emitter, Runtime};
use tokio::sync::mpsc;
use tracing::warn;

use super::clock::normalize_seek_position;
use super::AudioPlayer;
use crate::error::{AudioError, AudioResult};
use crate::types::*;

// ── EventBuffer for session-scoped polling (kept for compat) ──

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
    seek_tx: mpsc::UnboundedSender<SeekRequest>,
    shared: Arc<PlayerShared>,
    thread: Option<thread::JoinHandle<()>>,
}

pub struct PlayerShared {
    pub state: AtomicU8,
    pub position_ms: AtomicU64,
    pub duration_ms: AtomicU64,
    pub event_buf: parking_lot::Mutex<EventBuffer>,
    /// Event sink registered by the frontend via `audio_subscribe_events`.
    /// The forwarder streams every `AudioThreadEventMessage` here; when no
    /// channel is registered yet (startup / secondary windows) or a send
    /// fails (webview reload), it falls back to a Tauri global `emit`.
    pub event_channel:
        parking_lot::Mutex<Option<Channel<AudioThreadEventMessage<AudioThreadEvent>>>>,
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
            event_channel: parking_lot::Mutex::new(None),
        });

        // Forward events from the internal evt channel → the frontend's
        // `Channel` (registered via `audio_subscribe_events`), falling back to
        // a Tauri global `emit` when no channel is registered yet or a send
        // fails. We peek at certain events to update `shared` atomics so
        // `audio_get_state` can return up-to-date values without going through
        // the message loop.
        //
        // High-rate analysis events are coalesced before forwarding. Playback
        // controls/status events must not sit behind stale 2048-bin FFT JSON
        // frames in the channel queue; keeping only the latest FFT/lowFreq
        // sample preserves visual freshness while state/control events stay
        // realtime.
        let shared_clone = Arc::clone(&shared);
        let app = app_handle.clone();
        let seq_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        tauri::async_runtime::spawn(async move {
            let forward_msg = |mut evt_msg: AudioThreadEventMessage<AudioThreadEvent>| {
                evt_msg.seq = seq_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                if let Some(event) = &evt_msg.data {
                    update_shared_from_event(&shared_clone, event);
                    if should_buffer_poll_event(event) {
                        shared_clone.event_buf.lock().push(event.clone());
                    }
                }
                let channel = shared_clone.event_channel.lock().clone();
                if let Some(channel) = channel {
                    if channel.send(evt_msg.clone()).is_err() {
                        // The webview/channel went away (e.g. reload). Drop the
                        // stale sink so it self-heals on the next subscribe, and
                        // fall back to a global emit for this event.
                        *shared_clone.event_channel.lock() = None;
                        let _ = app.emit("audio-player://event", &evt_msg);
                    }
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
        let thread = thread::Builder::new()
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
            thread: Some(thread),
        })
    }

    pub fn handle(&self) -> PlayerHandle {
        PlayerHandle {
            msg_tx: self.msg_tx.clone(),
            seek_tx: self.seek_tx.clone(),
        }
    }

    pub fn send_msg(&self, msg: AudioThreadEventMessage<AudioThreadMessage>) -> AudioResult<()> {
        // Route seeks to the dedicated priority channel (coalesced via
        // `drain_latest_seek`) instead of the generic message queue, matching
        // the retired WebSocket control path. The invoke `callback_id` is not
        // used for seek correlation — the frontend correlates on `request_id`
        // via SeekCommitted/SeekFailed — so bypassing the queue is safe.
        if let Some(AudioThreadMessage::SeekAudio {
            position,
            request_id,
            expected_music_id,
        }) = msg.data.as_ref()
        {
            return self
                .seek_tx
                .send(SeekRequest::new(
                    *position,
                    *request_id,
                    expected_music_id.clone(),
                ))
                .map_err(|_| AudioError::ThreadError("player seek channel closed".into()));
        }
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

    /// Register the frontend event `Channel`. The event forwarder streams all
    /// `AudioThreadEventMessage`s here until it is replaced or the webview
    /// reloads (a failed send clears the slot and falls back to global emit).
    pub fn set_event_channel(&self, channel: Channel<AudioThreadEventMessage<AudioThreadEvent>>) {
        *self.shared.event_channel.lock() = Some(channel);
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.msg_tx.send(AudioThreadEventMessage::new(
            String::new(),
            Some(AudioThreadMessage::Close),
        ));
        if let Some(thread) = self.thread.take() {
            join_thread_async("audio-player-join", thread);
        }
    }
}

pub(super) fn join_thread_async(name: &'static str, handle: thread::JoinHandle<()>) {
    let _ = thread::Builder::new().name(name.into()).spawn(move || {
        let _ = handle.join();
    });
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

fn should_buffer_poll_event(event: &AudioThreadEvent) -> bool {
    !matches!(
        event,
        AudioThreadEvent::FFTData { .. } | AudioThreadEvent::LowFrequencyVolume { .. }
    )
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

// ── Cloneable handle for sending messages ────────────────────────

#[derive(Clone, Debug)]
pub struct PlayerHandle {
    msg_tx: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadMessage>>,
    seek_tx: mpsc::UnboundedSender<SeekRequest>,
}

#[derive(Clone, Debug)]
pub(super) struct SeekRequest {
    pub(super) position: f64,
    pub(super) request_id: Option<u64>,
    pub(super) expected_music_id: Option<String>,
}

impl SeekRequest {
    pub(super) fn new(
        position: f64,
        request_id: Option<u64>,
        expected_music_id: Option<String>,
    ) -> Self {
        Self {
            position,
            request_id,
            expected_music_id,
        }
    }

    pub(super) fn normalized(self) -> Self {
        Self {
            position: normalize_seek_position(self.position),
            request_id: self.request_id,
            expected_music_id: self.expected_music_id,
        }
    }
}

impl PlayerHandle {
    pub fn send(&self, msg: AudioThreadEventMessage<AudioThreadMessage>) -> AudioResult<()> {
        if msg.callback_id.is_empty() {
            if let Some(AudioThreadMessage::SeekAudio {
                position,
                request_id,
                expected_music_id,
            }) = msg.data.as_ref()
            {
                return self.send_seek(*position, *request_id, expected_music_id.clone());
            }
        }

        self.msg_tx
            .send(msg)
            .map_err(|_| AudioError::ThreadError("player channel closed".into()))
    }

    pub async fn send_anonymous(&self, msg: AudioThreadMessage) -> AudioResult<()> {
        self.send(AudioThreadEventMessage::new("".into(), Some(msg)))
    }

    pub fn send_seek(
        &self,
        position: f64,
        request_id: Option<u64>,
        expected_music_id: Option<String>,
    ) -> AudioResult<()> {
        self.seek_tx
            .send(SeekRequest::new(position, request_id, expected_music_id))
            .map_err(|_| AudioError::ThreadError("player seek channel closed".into()))
    }
}
