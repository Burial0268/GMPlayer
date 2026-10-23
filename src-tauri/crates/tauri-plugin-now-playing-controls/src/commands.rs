use std::{
    io::Read,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use now_playing_controls::{
    model::{
        MetadataPayload, NowPlayingOptions, PlayModePayload, PlayStatePayload, PlaybackStatus,
        RepeatMode, SystemMediaEvent, SystemMediaEventType, TimelinePayload,
    },
    EventCallback, NowPlayingSession,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, Runtime};
use tracing::warn;

const MEDIA_ACTION_EVENT: &str = "now-playing-controls:media-action";
const MAX_COVER_BYTES: u64 = 4 * 1024 * 1024;

/// In-process consumer of system-media actions.
type ActionHandler = Arc<dyn Fn(SystemMediaEvent) + Send + Sync>;

/// Where an installed [`on_action`] handler lives.
///
/// Process-wide rather than managed state, for the same reason the session is:
/// there is one media session per process and one OS that talks to it, and a
/// `static` sidesteps any question about whether the plugin's `setup` has run
/// by the time the app's own `setup` installs the handler.
static ACTION_HANDLER: OnceLock<ActionHandler> = OnceLock::new();

/// Take system-media actions in Rust instead of forwarding them to the frontend.
///
/// Installing a handler makes it the **only** consumer: nothing is emitted to
/// the webview afterwards, so the transport keeps exactly one writer. That is
/// the point — a media-key or flyout press that has to cross into JS to reach
/// the player is a press that depends on a live, listening page, and it is
/// indistinguishable from a working one right up until it silently isn't.
///
/// Only the first call takes effect.
pub fn on_action(handler: impl Fn(SystemMediaEvent) + Send + Sync + 'static) {
    if ACTION_HANDLER.set(Arc::new(handler)).is_err() {
        warn!("now playing action handler was already installed; ignoring the second one");
    }
}

#[derive(Default)]
pub struct NowPlayingState {
    inner: Mutex<NowPlayingStateInner>,
}

#[derive(Default)]
struct NowPlayingStateInner {
    session: Option<NowPlayingSession>,
    /// Whether the live session is currently projecting to the OS.
    ///
    /// Tracked separately from `session` because a clear only *disables* the
    /// session now — see [`NowPlayingState::clear_session`] — so "we have one"
    /// and "it is showing" stopped being the same question.
    enabled: bool,
    last_duration_secs: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStateRequest {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork_url: Option<String>,
    pub track_id: Option<i64>,
    pub is_playing: Option<bool>,
    pub playback_state: Option<String>,
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub playback_rate: Option<f64>,
    pub volume: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineRequest {
    pub position: f64,
    pub duration: Option<f64>,
    pub seeked: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayModeRequest {
    pub is_shuffling: bool,
    pub repeat_mode: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaActionPayload {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<f64>,
}

impl NowPlayingState {
    /// Get (creating on first use) the live system-media session.
    ///
    /// Public so the in-process media bridge can drive the session directly:
    /// on Android the WebView dies while playback continues, and the desktop
    /// path must stay symmetrical with it rather than keeping a second,
    /// JS-only entry point alive.
    pub fn ensure_session<R: Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
    ) -> Result<NowPlayingSession, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "now playing state lock poisoned".to_string())?;

        if let Some(session) = &inner.session {
            let session = session.clone();
            // Re-arm a session a clear left disabled. Cheap and idempotent on
            // every backend (SMTC flips `IsEnabled`, MPRIS a flag, macOS the
            // command targets), and it is what lets a clear stop short of
            // destroying the object.
            if !inner.enabled {
                session.enable_system_media();
                inner.enabled = true;
            }
            return Ok(session);
        }

        let options = NowPlayingOptions {
            hwnd: main_window_hwnd(app),
            discord: None,
            app_name: Some("GMPlayer".to_string()),
        };

        let event_app = app.clone();
        let callback: EventCallback = Arc::new(move |event| {
            // In-process first. When the app has claimed the actions there is
            // no frontend hop at all, which is what makes the buttons work
            // without depending on a page being mounted and listening.
            if let Some(handler) = ACTION_HANDLER.get() {
                handler(event);
                return;
            }

            let payload = MediaActionPayload::from(event);
            let action = payload.action.clone();
            // Not swallowed. This is the only hop between a system-media button
            // and the frontend that handles it, and it is the half that has no
            // visible symptom of its own: the push direction keeps working, so
            // the flyout looks perfectly healthy while every button is inert.
            // Note `emit` reports success even when no webview has registered a
            // listener, so a clean return here is not proof of delivery.
            if let Err(err) = event_app.emit(MEDIA_ACTION_EVENT, payload) {
                warn!("failed to forward media action {action}: {err}");
            }
        });

        let session = NowPlayingSession::new(options, callback)
            .map_err(|err| format!("failed to initialize now playing controls: {err}"))?;
        session.enable_system_media();
        inner.enabled = true;
        inner.session = Some(session.clone());
        Ok(session)
    }

    fn set_last_duration(&self, duration_secs: f64) {
        if duration_secs <= 0.0 {
            return;
        }
        if let Ok(mut inner) = self.inner.lock() {
            inner.last_duration_secs = duration_secs;
        }
    }

    /// Last duration handed to the session, used when a timeline update omits
    /// one. Public for the same reason as [`Self::ensure_session`].
    pub fn last_duration_secs(&self) -> f64 {
        self.last_duration()
    }

    fn last_duration(&self) -> f64 {
        self.inner
            .lock()
            .map(|inner| inner.last_duration_secs)
            .unwrap_or_default()
    }

    /// Stop projecting to the OS, keeping the session object alive.
    ///
    /// It used to `shutdown()` the session and drop it, which is a race on
    /// Windows: `shutdown` only *sends* a message, and the coordinator thread
    /// runs `WindowsImpl::shutdown` whenever it gets round to it — but
    /// `GetForWindow` hands out one `SystemMediaTransportControls` **per
    /// window**, so a session created in the meantime is talking to the same
    /// object. The late teardown then calls `SetIsEnabled(false)` and
    /// `RemoveButtonPressed` on the *new* session's SMTC, leaving a
    /// `WindowsImpl` whose own `is_enabled` says `true`: every later push
    /// succeeds and changes nothing.
    ///
    /// Disabling instead is what a clear actually means — the shell drops the
    /// card, and the next track re-enables through
    /// [`Self::ensure_session`]. Nothing needs the object destroyed: the
    /// session is process-scoped by design (playback outlives every window),
    /// and the OS releases it when the process goes.
    pub fn clear_session(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(session) = &inner.session {
                session.disable_system_media();
            }
            inner.enabled = false;
            inner.last_duration_secs = 0.0;
        }
    }
}

// ── Rust-callable surface ────────────────────────────────────────
//
// The media bridge (`src-tauri/src/media`) drives the session straight from the
// audio backend's event stream, so these must not live only behind
// `#[tauri::command]`. The commands below are thin wrappers over them — one
// implementation, two entry points.

/// Push metadata. `cover_data` is fetched by the caller (it is I/O).
pub fn apply_metadata<R: Runtime>(
    app: &tauri::AppHandle<R>,
    state: &NowPlayingState,
    title: String,
    artist: String,
    album: String,
    artwork_url: Option<String>,
    cover_data: Option<Vec<u8>>,
    track_id: Option<i64>,
    duration_secs: Option<f64>,
) -> Result<(), String> {
    let session = state.ensure_session(app)?;
    if let Some(duration) = duration_secs {
        state.set_last_duration(duration);
    }
    session.update_metadata(MetadataPayload {
        song_name: title,
        author_name: artist,
        album_name: album,
        cover_data,
        original_cover_url: artwork_url,
        genre: Vec::new(),
        track_id,
        discord_buttons: None,
        duration: positive_duration(duration_secs),
    });
    Ok(())
}

pub fn apply_play_state<R: Runtime>(
    app: &tauri::AppHandle<R>,
    state: &NowPlayingState,
    is_playing: bool,
) -> Result<(), String> {
    let session = state.ensure_session(app)?;
    session.update_play_state(PlayStatePayload {
        status: if is_playing {
            PlaybackStatus::Playing
        } else {
            PlaybackStatus::Paused
        },
    });
    Ok(())
}

pub fn apply_timeline<R: Runtime>(
    app: &tauri::AppHandle<R>,
    state: &NowPlayingState,
    position_secs: f64,
    duration_secs: Option<f64>,
    seeked: Option<bool>,
) -> Result<(), String> {
    let session = state.ensure_session(app)?;
    if let Some(duration) = duration_secs {
        state.set_last_duration(duration);
    }
    session.update_timeline(TimelinePayload {
        current_time: duration_from_secs(position_secs),
        total_time: duration_from_secs(duration_secs.unwrap_or_else(|| state.last_duration())),
        seeked,
    });
    Ok(())
}

/// Download cover art for [`apply_metadata`]. Blocking — call from a worker.
pub fn fetch_cover_blocking(url: &str) -> Option<Vec<u8>> {
    fetch_cover_data_blocking(url)
}

/// Project the play mode onto the system session.
///
/// The Rust-side twin of the [`update_play_mode`] command. Both exist because
/// the value has one owner (the audio backend) and two ways in: the backend's
/// own projection, which is what keeps SMTC/MPRIS right while the WebView is
/// gone, and the command, which is the web/legacy path.
///
/// Windows exposes this as `AutoRepeatMode` + `ShuffleEnabled` and MPRIS as
/// `LoopStatus` + `Shuffle`, so unlike Android there is nothing custom to draw —
/// the OS renders its own controls once the properties are set.
pub fn apply_play_mode<R: Runtime>(
    app: &tauri::AppHandle<R>,
    state: &NowPlayingState,
    is_shuffling: bool,
    repeat: RepeatMode,
) -> Result<(), String> {
    let session = state.ensure_session(app)?;
    session.update_play_mode(PlayModePayload {
        is_shuffling,
        repeat_mode: repeat,
    });
    Ok(())
}

impl From<SystemMediaEvent> for MediaActionPayload {
    fn from(event: SystemMediaEvent) -> Self {
        let action = match event.type_ {
            SystemMediaEventType::Play => "play",
            SystemMediaEventType::Pause => "pause",
            SystemMediaEventType::Stop => "stop",
            SystemMediaEventType::NextSong => "next",
            SystemMediaEventType::PreviousSong => "previous",
            SystemMediaEventType::ToggleShuffle => "toggleShuffle",
            SystemMediaEventType::ToggleRepeat => "toggleRepeat",
            SystemMediaEventType::SetRate => "setRate",
            SystemMediaEventType::SetVolume => "setVolume",
            SystemMediaEventType::Seek => "seek",
        }
        .to_string();

        Self {
            action,
            position: event.position.map(|pos| pos.as_millis() as u64),
            rate: event.rate,
            volume: event.volume,
        }
    }
}

#[cfg(windows)]
fn main_window_hwnd<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<isize> {
    app.get_webview_window("main")
        .and_then(|win| win.hwnd().ok())
        .map(|hwnd| hwnd.0 as isize)
}

#[cfg(not(windows))]
fn main_window_hwnd<R: Runtime>(_app: &tauri::AppHandle<R>) -> Option<isize> {
    None
}

#[tauri::command]
pub fn initialize<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, NowPlayingState>,
) -> Result<(), String> {
    let _ = state.ensure_session(&app)?;
    Ok(())
}

#[tauri::command]
pub async fn update_state<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, NowPlayingState>,
    payload: UpdateStateRequest,
) -> Result<(), String> {
    let session = state.ensure_session(&app)?;
    let is_playing = playback_status(&payload);
    let has_metadata = payload.title.is_some()
        || payload.artist.is_some()
        || payload.album.is_some()
        || payload.artwork_url.is_some()
        || payload.duration.is_some();

    if let Some(duration) = payload.duration {
        state.set_last_duration(duration);
    }

    if has_metadata {
        let artwork_url = non_empty(payload.artwork_url.clone());
        let cover_data = fetch_cover_data(artwork_url.clone()).await;
        session.update_metadata(MetadataPayload {
            song_name: payload.title.clone().unwrap_or_default(),
            author_name: payload.artist.clone().unwrap_or_default(),
            album_name: payload.album.clone().unwrap_or_default(),
            cover_data,
            original_cover_url: artwork_url,
            genre: Vec::new(),
            track_id: payload.track_id,
            discord_buttons: None,
            duration: positive_duration(payload.duration),
        });
    }

    if let Some(is_playing) = is_playing {
        session.update_play_state(PlayStatePayload {
            status: if is_playing {
                PlaybackStatus::Playing
            } else {
                PlaybackStatus::Paused
            },
        });
    }

    if payload.position.is_some() || payload.duration.is_some() {
        let current_time = payload.position.unwrap_or_default();
        let total_time = payload.duration.unwrap_or_else(|| state.last_duration());
        session.update_timeline(TimelinePayload {
            current_time: duration_from_secs(current_time),
            total_time: duration_from_secs(total_time),
            seeked: None,
        });
    }

    if let Some(rate) = payload.playback_rate {
        session.update_playback_rate(rate);
    }

    if let Some(volume) = payload.volume {
        session.update_volume(volume);
    }

    Ok(())
}

#[tauri::command]
pub fn update_timeline<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, NowPlayingState>,
    payload: TimelineRequest,
) -> Result<(), String> {
    let session = state.ensure_session(&app)?;
    if let Some(duration) = payload.duration {
        state.set_last_duration(duration);
    }

    session.update_timeline(TimelinePayload {
        current_time: duration_from_secs(payload.position),
        total_time: duration_from_secs(payload.duration.unwrap_or_else(|| state.last_duration())),
        seeked: payload.seeked,
    });
    Ok(())
}

#[tauri::command]
pub fn update_play_mode<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, NowPlayingState>,
    payload: PlayModeRequest,
) -> Result<(), String> {
    let session = state.ensure_session(&app)?;
    session.update_play_mode(PlayModePayload {
        is_shuffling: payload.is_shuffling,
        repeat_mode: match payload.repeat_mode.as_str() {
            "track" | "single" | "one" => RepeatMode::Track,
            "list" | "normal" | "all" => RepeatMode::List,
            _ => RepeatMode::None,
        },
    });
    Ok(())
}

#[tauri::command]
pub fn set_enabled<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, NowPlayingState>,
    enabled: bool,
) -> Result<(), String> {
    let session = state.ensure_session(&app)?;
    if enabled {
        session.enable_system_media();
    } else {
        session.disable_system_media();
    }
    Ok(())
}

#[tauri::command]
pub fn clear(state: tauri::State<'_, NowPlayingState>) {
    state.clear_session();
}

fn playback_status(payload: &UpdateStateRequest) -> Option<bool> {
    if let Some(is_playing) = payload.is_playing {
        return Some(is_playing);
    }

    match payload.playback_state.as_deref() {
        Some("playing") => Some(true),
        Some("paused") | Some("buffering") | Some("stopped") => Some(false),
        _ => None,
    }
}

fn positive_duration(secs: Option<f64>) -> Option<Duration> {
    secs.filter(|value| *value > 0.0).map(duration_from_secs)
}

fn duration_from_secs(secs: f64) -> Duration {
    Duration::from_secs_f64(secs.max(0.0))
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

async fn fetch_cover_data(url: Option<String>) -> Option<Vec<u8>> {
    let url = url?;
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }

    tauri::async_runtime::spawn_blocking(move || fetch_cover_data_blocking(&url))
        .await
        .ok()
        .flatten()
}

fn fetch_cover_data_blocking(url: &str) -> Option<Vec<u8>> {
    // A local track's cover is a path the library scan wrote, not a CDN URL.
    // `ureq::get` on it fails with a scheme error, so the OS session would show
    // no artwork at all for every locally-imported track.
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return read_local_cover_blocking(url);
    }
    let response = ureq::get(url).timeout(Duration::from_secs(5)).call().ok()?;
    let mut reader = response.into_reader().take(MAX_COVER_BYTES + 1);
    let mut bytes = Vec::new();
    if reader.read_to_end(&mut bytes).is_err() {
        return None;
    }
    if bytes.len() as u64 > MAX_COVER_BYTES {
        warn!("now playing cover skipped because it exceeds {MAX_COVER_BYTES} bytes");
        return None;
    }
    Some(bytes)
}

/// Read a cover off disk, accepting both a bare path and a `file://` URL.
///
/// Desktop only handles those two: a `content://` document is Android's, and
/// this crate is not built there.
fn read_local_cover_blocking(url: &str) -> Option<Vec<u8>> {
    let path = match url.strip_prefix("file://") {
        // `file:///C:/x` on Windows and `file:///home/x` elsewhere — the leading
        // slash belongs to the POSIX path but not to the drive-letter one.
        Some(rest) => {
            let trimmed = rest.trim_start_matches('/');
            if trimmed.chars().nth(1) == Some(':') {
                trimmed.to_string()
            } else {
                format!("/{trimmed}")
            }
        }
        None => url.to_string(),
    };

    let metadata = std::fs::metadata(&path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    if metadata.len() > MAX_COVER_BYTES {
        warn!("now playing cover skipped because it exceeds {MAX_COVER_BYTES} bytes");
        return None;
    }
    std::fs::read(&path).ok()
}
