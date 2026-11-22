use serde::Serialize;
use std::sync::Arc;
#[cfg(target_os = "android")]
use std::sync::OnceLock;
#[cfg(target_os = "android")]
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tauri::State;
#[cfg(target_os = "android")]
use tracing::info;
use tracing::warn;

use crate::automix::{self, AutomixAnalyzeRequest, AutomixAnalyzeSourceRequest, TrackAnalysis};
use crate::player::Player;
use crate::types::*;

// ── PlayerState (managed by Tauri) ────────────────────────────────

#[cfg(target_os = "android")]
type AndroidContextReadyCheck = fn() -> bool;

#[cfg(target_os = "android")]
static ANDROID_CONTEXT_READY_CHECK: OnceLock<AndroidContextReadyCheck> = OnceLock::new();

#[cfg(target_os = "android")]
pub fn set_android_context_ready_check(check: AndroidContextReadyCheck) {
    let _ = ANDROID_CONTEXT_READY_CHECK.set(check);
}

pub struct PlayerState {
    inner: Arc<PlayerStateInner>,
}

type EventChannel = Channel<AudioThreadEventMessage<AudioThreadEvent>>;

struct PlayerStateInner {
    app_handle: tauri::AppHandle,
    init_lock: parking_lot::Mutex<()>,
    player: parking_lot::Mutex<Option<Arc<Player>>>,
    /// Every in-process subscriber, attached to each player that is created.
    ///
    /// The player is created lazily (it opens an audio device), and forcing
    /// that at startup just to attach the media bridge would move device-open
    /// failures into app launch. They are kept rather than drained because a
    /// player that stopped is replaced, and its replacement needs them too.
    subscribers: parking_lot::Mutex<Vec<Arc<dyn crate::player::PlayerEventSubscriber>>>,
    /// The frontend's event channel, re-attached to a replacement player.
    event_channel: parking_lot::Mutex<Option<EventChannel>>,
}

impl PlayerState {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self {
            inner: Arc::new(PlayerStateInner {
                app_handle,
                init_lock: parking_lot::Mutex::new(()),
                player: parking_lot::Mutex::new(None),
                subscribers: parking_lot::Mutex::new(Vec::new()),
                event_channel: parking_lot::Mutex::new(None),
            }),
        }
    }

    /// Attach an in-process event consumer (see
    /// [`crate::player::PlayerEventSubscriber`]).
    ///
    /// Safe to call before playback has ever started: the subscriber is queued
    /// and attached when the player is first created, so registering the media
    /// bridge at startup does not itself open an audio device.
    pub fn subscribe_events(&self, subscriber: Arc<dyn crate::player::PlayerEventSubscriber>) {
        // Hold the list across the check so a player created concurrently
        // cannot attach the list without this one and leave it unattached.
        let mut subscribers = self.inner.subscribers.lock();
        if let Some(player) = self.inner.live_player() {
            player.subscribe(Arc::clone(&subscriber));
        }
        subscribers.push(subscriber);
    }

    pub fn preheat(&self) -> Result<(), String> {
        self.inner.preheat()
    }

    /// Send a control message from in-process glue (the OS media session),
    /// without creating a player that does not exist yet.
    ///
    /// Synchronous on purpose: the caller is a platform callback — a
    /// notification button, a media key, an audio-focus change — that must not
    /// await anything. Creating the player here is deliberately *not* done: a
    /// media button can only be pressed while a session is live, so "no player"
    /// means the press is stale, and opening an audio device in response would
    /// be worse than dropping it.
    pub fn try_send_msg(&self, msg: AudioThreadMessage) -> Result<(), String> {
        let player = self
            .inner
            .live_player()
            .ok_or_else(|| "native audio player is not running".to_string())?;
        player
            .send_msg(AudioThreadEventMessage::new(String::new(), Some(msg)))
            .map_err(|e| e.to_string())
    }

    async fn preheat_async(&self) -> Result<(), String> {
        let inner = Arc::clone(&self.inner);
        tauri::async_runtime::spawn_blocking(move || inner.preheat())
            .await
            .map_err(|e| e.to_string())?
    }

    async fn player(&self) -> Result<Arc<Player>, String> {
        if let Some(player) = self.inner.live_player() {
            return Ok(player);
        }

        let inner = Arc::clone(&self.inner);
        tauri::async_runtime::spawn_blocking(move || inner.player())
            .await
            .map_err(|e| e.to_string())?
    }
}

impl PlayerStateInner {
    /// The player, unless it has stopped. A stopped one is replaced by the next caller that may
    /// create a player, instead of failing every command until the app restarts.
    fn live_player(&self) -> Option<Arc<Player>> {
        self.player
            .lock()
            .as_ref()
            .filter(|player| player.is_alive())
            .cloned()
    }

    fn preheat(&self) -> Result<(), String> {
        if self.live_player().is_some() {
            return Ok(());
        }

        let _init = self.init_lock.lock();
        if self.live_player().is_some() {
            return Ok(());
        }
        if self.player.lock().take().is_some() {
            warn!("native audio player stopped; starting a new one");
        }

        #[cfg(target_os = "android")]
        wait_for_android_context_ready()?;

        let next = Player::new(self.app_handle.clone()).map_err(|e| e.to_string())?;
        let next = Arc::new(next);
        // Attach every subscriber while holding the list — a `subscribe_events`
        // racing this either sees the player (and attaches directly) or lands in
        // the list before we walk it.
        {
            let subscribers = self.subscribers.lock();
            for subscriber in subscribers.iter() {
                next.subscribe(Arc::clone(subscriber));
            }
            if let Some(channel) = self.event_channel.lock().clone() {
                next.set_event_channel(channel);
            }
            *self.player.lock() = Some(next);
        }
        Ok(())
    }

    fn player(&self) -> Result<Arc<Player>, String> {
        if let Some(player) = self.live_player() {
            return Ok(player);
        }

        self.preheat()?;
        self.player
            .lock()
            .as_ref()
            .cloned()
            .ok_or_else(|| "native audio player was not initialized".into())
    }
}

#[cfg(target_os = "android")]
fn wait_for_android_context_ready() -> Result<(), String> {
    const ANDROID_CONTEXT_TIMEOUT: Duration = Duration::from_secs(15);
    const ANDROID_CONTEXT_POLL: Duration = Duration::from_millis(10);

    let Some(check) = ANDROID_CONTEXT_READY_CHECK.get().copied() else {
        return Err("android native audio context readiness check was not registered".into());
    };

    if check() {
        return Ok(());
    }

    info!("Waiting for Android NDK context before opening native audio");
    let deadline = Instant::now() + ANDROID_CONTEXT_TIMEOUT;
    while !check() {
        if Instant::now() >= deadline {
            warn!("Timed out waiting for Android NDK context before native audio init");
            return Err("timed out waiting for Android NDK context".into());
        }
        std::thread::sleep(ANDROID_CONTEXT_POLL);
    }

    Ok(())
}

// ── Response types ────────────────────────────────────────────────

#[derive(Serialize, Clone, Debug)]
pub struct AudioStateResponse {
    pub state: String,
    pub is_playing: bool,
    pub position: f64,
    pub duration: f64,
}

fn state_name(s: PlaybackState) -> &'static str {
    match s {
        PlaybackState::Stopped => "stopped",
        PlaybackState::Playing => "playing",
        PlaybackState::Paused => "paused",
        PlaybackState::Ended => "ended",
    }
}

#[tauri::command]
pub async fn audio_preheat(state: State<'_, PlayerState>) -> Result<(), String> {
    state.preheat_async().await
}

// ═══════════════════════════════════════════════════════════════════
//  AMLL-style: single message entry point
// ═══════════════════════════════════════════════════════════════════

/// Send an AudioThreadMessage to the player via Tauri invoke.
/// This is the native playback control path (frontend → Rust); events flow
/// back over the `Channel` registered by `audio_subscribe_events`.
#[tauri::command]
pub async fn audio_send_msg(
    state: State<'_, PlayerState>,
    msg: AudioThreadEventMessage<AudioThreadMessage>,
) -> Result<(), String> {
    state
        .player()
        .await?
        .send_msg(msg)
        .map_err(|e| e.to_string())
}

// ═══════════════════════════════════════════════════════════════════
//  Sync query commands (fast reads, no round-trip through msg loop)
// ═══════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn audio_get_state(state: State<'_, PlayerState>) -> Result<AudioStateResponse, String> {
    let p = state.player().await?;
    Ok(AudioStateResponse {
        state: state_name(p.state()).into(),
        is_playing: p.is_playing(),
        position: p.position(),
        duration: p.duration(),
    })
}

/// Authoritative read of what the backend is playing right now.
///
/// The Rust process (and playback) outlives the WebView on Android: the page is
/// destroyed and reloaded while audio keeps going. A reloaded frontend must ask
/// here *before* it resolves a URL for its persisted track, otherwise it
/// replaces live playback with a stale snapshot from when the app was
/// backgrounded. Synchronous — reads a mutex, never touches the message loop.
#[tauri::command]
pub async fn audio_get_session(
    state: State<'_, PlayerState>,
) -> Result<NativeSessionSnapshot, String> {
    Ok(state.player().await?.session())
}

/// Output devices the user can pick, for the player's output menu. Selection itself is the
/// `SetAudioOutput` message; an empty name there means the system default.
#[tauri::command]
pub async fn audio_list_output_devices() -> Result<Vec<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    crate::output::spawn_device_task(move || {
        let _ = tx.send(crate::output::output_device_names());
    });
    rx.await
        .map_err(|_| "audio device thread dropped the request".to_string())?
}

/// Register the frontend event `Channel` (Rust → frontend event stream:
/// FFT / status / position). The frontend creates a `Channel`, wires its
/// `onmessage`, and passes it here; the player forwards every
/// `AudioThreadEventMessage` to it. Replaces the old local WebSocket bridge.
#[tauri::command]
pub async fn audio_subscribe_events(
    state: State<'_, PlayerState>,
    channel: Channel<AudioThreadEventMessage<AudioThreadEvent>>,
) -> Result<(), String> {
    *state.inner.event_channel.lock() = Some(channel.clone());
    state.player().await?.set_event_channel(channel);
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
//  Session-based event polling (kept for backward compat during migration)
// ═══════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn audio_set_session(
    state: State<'_, PlayerState>,
    session_id: u64,
) -> Result<(), String> {
    state.player().await?.set_session(session_id);
    Ok(())
}

#[tauri::command]
pub async fn audio_poll_events(
    state: State<'_, PlayerState>,
    session_id: u64,
) -> Result<Vec<AudioThreadEvent>, String> {
    Ok(state.player().await?.poll_events(session_id))
}

// ═══════════════════════════════════════════════════════════════════
//  AutoMix analysis
// ═══════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn audio_analyze_automix(req: AutomixAnalyzeRequest) -> Result<TrackAnalysis, String> {
    tauri::async_runtime::spawn_blocking(move || automix::analyze_audio_bytes(req))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn audio_analyze_automix_source(
    req: AutomixAnalyzeSourceRequest,
) -> Result<TrackAnalysis, String> {
    tauri::async_runtime::spawn_blocking(move || automix::analyze_audio_source(req))
        .await
        .map_err(|e| e.to_string())?
}
