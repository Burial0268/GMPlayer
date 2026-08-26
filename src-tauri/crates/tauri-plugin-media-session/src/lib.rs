use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

#[cfg(mobile)]
use std::sync::{Arc, Mutex};

#[cfg(mobile)]
use serde::{Deserialize, Serialize};

#[cfg(mobile)]
use tauri::ipc::{Channel, InvokeResponseBody};

#[cfg(mobile)]
use tauri::{Emitter, Manager};

/// Kotlin package holding `MediaSessionPlugin`.
///
/// Tauri resolves the plugin class as `<identifier>.<class>`, so this must stay
/// in lockstep with the `namespace` in `android/build.gradle.kts` and the
/// package declaration in the Kotlin sources. Renamed off the upstream
/// `app.tauri.mediasession` so the foreground service shows up under our own
/// name in the system's running-services and battery screens.
#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "com.gbclstudio.gmplayer.media";

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_media_session);

/// Media action triggered from notification, lockscreen, or hardware buttons.
#[cfg(mobile)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaAction {
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    Seek,
    /// Advance the play mode one step. Deliberately an intent rather than a
    /// value: the notification button knows only that it was pressed, and the
    /// mode it would compute from its own copy could already be stale.
    CyclePlayMode,
    /// Toggle the current track's like state.
    ToggleFavourite,
}

/// Event payload received when the user interacts with media controls.
#[cfg(mobile)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaActionEvent {
    /// The action triggered by the user.
    pub action: MediaAction,
    /// Target position in seconds (only present for `Seek` actions).
    pub seek_position: Option<f64>,
}

/// Media playback state.
///
/// All fields are optional — omitted fields preserve their previous values
/// on the native side (merge semantics).
#[cfg(mobile)]
#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaState {
    /// Track title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Artist name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    /// Album name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// URL to an image (JPEG/PNG). Downloaded natively (no CORS).
    /// Use this when the image is on a CDN or external server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork_url: Option<String>,
    /// Track duration in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    /// Current playback position in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<f64>,
    /// Playback speed multiplier (default: 1.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback_speed: Option<f64>,
    /// Whether media is currently playing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_playing: Option<bool>,
    /// Whether the session is preparing the track (resolving, downloading,
    /// buffering) rather than paused.
    ///
    /// Renders as `STATE_BUFFERING`, which is what makes the system controls
    /// show a spinner instead of a play button that appears to do nothing.
    /// Cleared by the next `is_playing` update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_loading: Option<bool>,
    /// Whether the "previous track" action is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_prev: Option<bool>,
    /// Whether the "next track" action is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_next: Option<bool>,
    /// Whether seeking is available (default: true).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_seek: Option<bool>,
    /// Traversal mode: `"normal"`, `"random"` or `"single"`.
    ///
    /// A string, not an int, so a mode one side does not know about renders as
    /// the default glyph instead of silently picking the wrong one. Also drives
    /// the session's native `setShuffleMode`/`setRepeatMode`, which is what the
    /// lock screen and Android Auto read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub play_mode: Option<String>,
    /// Whether the current track is in the user's 我喜欢的音乐.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favourite: Option<bool>,
}

/// Lightweight timeline update (position, duration, speed only).
///
/// Skips notification rebuild — ideal for frequent position syncs.
#[cfg(mobile)]
#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineUpdate {
    /// Current playback position in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<f64>,
    /// Track duration in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    /// Playback speed multiplier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback_speed: Option<f64>,
}

/// Handle to the native media session.
#[cfg(mobile)]
pub struct MediaSession<R: Runtime> {
    handle: tauri::plugin::PluginHandle<R>,
    callback: Arc<Mutex<Option<Box<dyn Fn(MediaActionEvent) + Send + Sync>>>>,
}

#[cfg(mobile)]
impl<R: Runtime> MediaSession<R> {
    /// Update the media session state and notification.
    ///
    /// Auto-initializes the session on first call.
    /// Only include the fields that changed — previous values are preserved.
    ///
    /// Async on purpose. The blocking `run_mobile_plugin` parks the calling
    /// thread on a `recv()` with no timeout until the Android main thread
    /// services the call — and a stopped Activity under a background-restricted
    /// ROM may not service it for a long time, or at all. Doing that from the
    /// media bridge's drain task parked a runtime worker and, worse, wedged
    /// every later push behind the stuck one: the session then froze on
    /// whatever it was last showing while playback carried on.
    pub async fn update_state(&self, state: MediaState) -> Result<(), String> {
        self.handle
            .run_mobile_plugin_async("updateState", state)
            .await
            .map_err(|e| format!("{e}"))
    }

    /// Lightweight timeline update — only touches `PlaybackState`, skips notification rebuild.
    ///
    /// Use this for frequent position syncs during playback.
    /// The session must already be initialized via [`update_state`](Self::update_state).
    pub async fn update_timeline(&self, timeline: TimelineUpdate) -> Result<(), String> {
        self.handle
            .run_mobile_plugin_async("updateTimeline", timeline)
            .await
            .map_err(|e| format!("{e}"))
    }

    /// Clear the media session, dismiss the notification, and release resources.
    pub async fn clear(&self) -> Result<(), String> {
        self.handle
            .run_mobile_plugin_async::<()>("clear", ())
            .await
            .map_err(|e| format!("{e}"))
    }

    /// Pre-initialize the session and request notification permissions.
    ///
    /// Optional — [`update_state`](Self::update_state) auto-initializes when needed.
    pub async fn initialize(&self) -> Result<(), String> {
        self.handle
            .run_mobile_plugin_async::<()>("initialize", ())
            .await
            .map_err(|e| format!("{e}"))
    }

    /// Register a callback for media action events (play, pause, seek, etc.).
    ///
    /// Calling this again replaces the previous listener.
    /// Use [`remove_action_listener`](Self::remove_action_listener) to clear it.
    pub fn on_action<F: Fn(MediaActionEvent) + Send + Sync + 'static>(&self, callback: F) {
        *self.callback.lock().unwrap() = Some(Box::new(callback));
    }

    /// Remove the currently registered action listener.
    pub fn remove_action_listener(&self) {
        *self.callback.lock().unwrap() = None;
    }
}

/// Extension trait for accessing the media session from any Tauri manager.
#[cfg(mobile)]
pub trait MediaSessionExt<R: Runtime> {
    fn media_session(&self) -> &MediaSession<R>;
}

#[cfg(mobile)]
impl<R: Runtime, T: Manager<R>> MediaSessionExt<R> for T {
    fn media_session(&self) -> &MediaSession<R> {
        self.state::<MediaSession<R>>().inner()
    }
}

#[cfg(mobile)]
#[derive(Serialize)]
struct SetEventHandlerArgs {
    handler: Channel,
}

/// Initialize the media-session plugin.
///
/// On non-mobile platforms this registers a no-op plugin so that
/// cross-platform apps compile without `cfg` gates around the plugin call.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("media-session")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            let handle = _api.register_android_plugin(PLUGIN_IDENTIFIER, "MediaSessionPlugin")?;

            #[cfg(target_os = "ios")]
            let handle = _api.register_ios_plugin(init_plugin_media_session)?;

            #[cfg(mobile)]
            {
                let callback: Arc<Mutex<Option<Box<dyn Fn(MediaActionEvent) + Send + Sync>>>> =
                    Arc::new(Mutex::new(None));
                let cb = callback.clone();

                let app_handle = _app.clone();
                handle.run_mobile_plugin::<()>(
                    "setEventHandler",
                    SetEventHandlerArgs {
                        handler: Channel::new(move |event| {
                            if let InvokeResponseBody::Json(payload) = event {
                                match serde_json::from_str::<MediaActionEvent>(&payload) {
                                    Ok(event) => {
                                        // In-process consumer first. This arrives
                                        // on the Android UI thread, and the
                                        // registered callback is the one that
                                        // actually moves playback; `emit` behind
                                        // it is a webview round-trip that must not
                                        // delay a button press — and does nothing
                                        // at all when the page is gone, which is
                                        // when it matters.
                                        if let Some(ref f) = *cb.lock().unwrap() {
                                            f(event.clone());
                                        }
                                        let _ = app_handle.emit("media_action", &event);
                                    }
                                    // Silence here used to mean a button that did
                                    // nothing with no way to tell why: an action
                                    // name Kotlin sends but this enum does not
                                    // know deserializes to nothing and is dropped.
                                    Err(err) => log::warn!(
                                        "unrecognised media action {payload}: {err}"
                                    ),
                                }
                            }
                            Ok(())
                        }),
                    },
                )?;

                _app.manage(MediaSession { handle, callback });
            }

            Ok(())
        })
        .build()
}
