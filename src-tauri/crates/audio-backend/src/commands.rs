use serde::Serialize;
use std::sync::Arc;
#[cfg(target_os = "android")]
use std::sync::OnceLock;
#[cfg(target_os = "android")]
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tauri::State;
#[cfg(target_os = "android")]
use tracing::{info, warn};

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

struct PlayerStateInner {
    app_handle: tauri::AppHandle,
    init_lock: parking_lot::Mutex<()>,
    player: parking_lot::Mutex<Option<Arc<Player>>>,
}

impl PlayerState {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self {
            inner: Arc::new(PlayerStateInner {
                app_handle,
                init_lock: parking_lot::Mutex::new(()),
                player: parking_lot::Mutex::new(None),
            }),
        }
    }

    pub fn preheat(&self) -> Result<(), String> {
        self.inner.preheat()
    }

    async fn preheat_async(&self) -> Result<(), String> {
        let inner = Arc::clone(&self.inner);
        tauri::async_runtime::spawn_blocking(move || inner.preheat())
            .await
            .map_err(|e| e.to_string())?
    }

    async fn player(&self) -> Result<Arc<Player>, String> {
        if let Some(player) = self.inner.player.lock().as_ref().cloned() {
            return Ok(player);
        }

        let inner = Arc::clone(&self.inner);
        tauri::async_runtime::spawn_blocking(move || inner.player())
            .await
            .map_err(|e| e.to_string())?
    }
}

impl PlayerStateInner {
    fn preheat(&self) -> Result<(), String> {
        if self.player.lock().is_some() {
            return Ok(());
        }

        let _init = self.init_lock.lock();
        if self.player.lock().is_some() {
            return Ok(());
        }

        #[cfg(target_os = "android")]
        wait_for_android_context_ready()?;

        let next = Player::new(self.app_handle.clone()).map_err(|e| e.to_string())?;
        *self.player.lock() = Some(Arc::new(next));
        Ok(())
    }

    fn player(&self) -> Result<Arc<Player>, String> {
        if let Some(player) = self.player.lock().as_ref().cloned() {
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

/// Register the frontend event `Channel` (Rust → frontend event stream:
/// FFT / status / position). The frontend creates a `Channel`, wires its
/// `onmessage`, and passes it here; the player forwards every
/// `AudioThreadEventMessage` to it. Replaces the old local WebSocket bridge.
#[tauri::command]
pub async fn audio_subscribe_events(
    state: State<'_, PlayerState>,
    channel: Channel<AudioThreadEventMessage<AudioThreadEvent>>,
) -> Result<(), String> {
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
