use std::{
    io::Read,
    sync::{Arc, Mutex},
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

#[derive(Default)]
pub struct NowPlayingState {
    inner: Mutex<NowPlayingStateInner>,
}

#[derive(Default)]
struct NowPlayingStateInner {
    session: Option<NowPlayingSession>,
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
    fn ensure_session<R: Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
    ) -> Result<NowPlayingSession, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "now playing state lock poisoned".to_string())?;

        if let Some(session) = &inner.session {
            return Ok(session.clone());
        }

        let options = NowPlayingOptions {
            hwnd: main_window_hwnd(app),
            discord: None,
            app_name: Some("GMPlayer".to_string()),
        };

        let event_app = app.clone();
        let callback: EventCallback = Arc::new(move |event| {
            let payload = MediaActionPayload::from(event);
            let _ = event_app.emit(MEDIA_ACTION_EVENT, payload);
        });

        let session = NowPlayingSession::new(options, callback)
            .map_err(|err| format!("failed to initialize now playing controls: {err}"))?;
        session.enable_system_media();
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

    fn last_duration(&self) -> f64 {
        self.inner
            .lock()
            .map(|inner| inner.last_duration_secs)
            .unwrap_or_default()
    }

    fn clear_session(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(session) = inner.session.take() {
                session.disable_system_media();
                session.shutdown();
            }
            inner.last_duration_secs = 0.0;
        }
    }
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
