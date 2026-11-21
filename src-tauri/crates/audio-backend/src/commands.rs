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
use crate::player::{bus, EventBus, EventSinks, Player};
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
    init_lock: parking_lot::Mutex<()>,
    player: parking_lot::Mutex<Option<Arc<Player>>>,
    /// Outlive every player. The player is created lazily (it opens an audio
    /// device) and replaced when it stops; subscribers and the frontend channel
    /// attach here once, and a replacement is announced through the bus.
    bus: Arc<EventBus>,
    sinks: Arc<EventSinks>,
}

impl PlayerState {
    pub fn new() -> Self {
        let (bus, events) = EventBus::new();
        let sinks = Arc::new(EventSinks::default());
        bus::spawn_forwarder(events, Arc::clone(&sinks));
        Self {
            inner: Arc::new(PlayerStateInner {
                init_lock: parking_lot::Mutex::new(()),
                player: parking_lot::Mutex::new(None),
                bus,
                sinks,
            }),
        }
    }

    /// Attach an in-process event consumer (see
    /// [`crate::player::PlayerEventSubscriber`]). Opens no audio device.
    pub fn subscribe_events(&self, subscriber: Arc<dyn crate::player::PlayerEventSubscriber>) {
        self.inner.sinks.subscribe(subscriber);
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
            .send_msg(CommandEnvelope::internal(msg))
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
        let replacing = self.player.lock().take().is_some();
        if replacing {
            warn!("native audio player stopped; starting a new one");
            // Before the new player can emit: its events must follow the
            // `Hello` that retires everything the old one said.
            self.bus.restart();
        }

        #[cfg(target_os = "android")]
        wait_for_android_context_ready()?;

        let next = Player::new(Arc::clone(&self.bus)).map_err(|e| e.to_string())?;
        *self.player.lock() = Some(Arc::new(next));
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

#[tauri::command]
pub async fn audio_preheat(state: State<'_, PlayerState>) -> Result<(), String> {
    state.preheat_async().await
}

// ═══════════════════════════════════════════════════════════════════
//  AMLL-style: single message entry point
// ═══════════════════════════════════════════════════════════════════

/// Send a command to the player. The frontend numbers its commands (`cmd`),
/// and every event the player emits from then on carries the last one it took
/// up as `ack`.
#[tauri::command]
pub async fn audio_send_msg(
    state: State<'_, PlayerState>,
    msg: CommandEnvelope,
) -> Result<(), String> {
    state
        .player()
        .await?
        .send_msg(msg)
        .map_err(|e| e.to_string())
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

/// Make `channel` the frontend's event stream. Its first message is a `Hello`
/// carrying the session snapshot; every later message is newer than it. A page
/// subscribes again whenever it needs a fresh snapshot — after a reload, or to
/// attach to a track the backend is already playing. Opens no audio device: a
/// backend that has not started answers with an empty snapshot.
#[tauri::command]
pub async fn audio_subscribe_events(
    state: State<'_, PlayerState>,
    channel: Channel<EventEnvelope>,
) -> Result<(), String> {
    state.inner.sinks.attach_channel(&state.inner.bus, channel)
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
