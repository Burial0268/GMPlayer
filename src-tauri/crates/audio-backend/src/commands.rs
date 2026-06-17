use serde::Serialize;
use tauri::{Runtime, State};

use crate::automix::{self, AutomixAnalyzeRequest, AutomixAnalyzeSourceRequest, TrackAnalysis};
use crate::player::Player;
use crate::types::*;

// ── PlayerState (managed by Tauri) ────────────────────────────────

pub struct PlayerState {
    pub player: std::sync::Arc<Player>,
}

impl PlayerState {
    pub fn new<R: Runtime>(app_handle: tauri::AppHandle<R>) -> Self {
        let player = Player::new(app_handle).expect("Failed to create native audio player");
        PlayerState {
            player: std::sync::Arc::new(player),
        }
    }
}

impl Clone for PlayerState {
    fn clone(&self) -> Self {
        PlayerState {
            player: std::sync::Arc::clone(&self.player),
        }
    }
}

// ── Response types ────────────────────────────────────────────────

#[derive(Serialize, Clone, Debug)]
pub struct AudioStateResponse {
    pub state: String,
    pub is_playing: bool,
    pub position: f64,
    pub duration: f64,
}

#[derive(Serialize, Clone, Debug)]
pub struct AudioWsUrlsResponse {
    pub events: String,
    pub control: String,
}

fn state_name(s: PlaybackState) -> &'static str {
    match s {
        PlaybackState::Stopped => "stopped",
        PlaybackState::Playing => "playing",
        PlaybackState::Paused => "paused",
        PlaybackState::Ended => "ended",
    }
}

// ═══════════════════════════════════════════════════════════════════
//  AMLL-style: single message entry point
// ═══════════════════════════════════════════════════════════════════

/// Send an AudioThreadMessage to the player via Tauri invoke.
/// Native playback controls use the local WebSocket; this remains for
/// compatibility and diagnostics.
#[tauri::command]
pub fn audio_send_msg(
    state: State<PlayerState>,
    msg: AudioThreadEventMessage<AudioThreadMessage>,
) -> Result<(), String> {
    state.player.send_msg(msg).map_err(|e| e.to_string())
}

// ═══════════════════════════════════════════════════════════════════
//  Sync query commands (fast reads, no round-trip through msg loop)
// ═══════════════════════════════════════════════════════════════════

#[tauri::command]
pub fn audio_get_state(state: State<PlayerState>) -> Result<AudioStateResponse, String> {
    let p = state.player.as_ref();
    Ok(AudioStateResponse {
        state: state_name(p.state()).into(),
        is_playing: p.is_playing(),
        position: p.position(),
        duration: p.duration(),
    })
}

/// Return `ws://127.0.0.1:PORT` for the local WebSocket bridge, or `null`
/// if the bridge failed to bind (e.g. all ports busy). Kept for older
/// frontend code; returns the event socket.
#[tauri::command]
pub fn audio_get_ws_url(state: State<PlayerState>) -> Result<Option<String>, String> {
    Ok(state.player.ws_url())
}

/// Return split WebSocket URLs:
/// - `events`: Rust → frontend event stream (FFT/status/position)
/// - `control`: frontend → Rust command stream (play/pause/seek/volume)
#[tauri::command]
pub fn audio_get_ws_urls(state: State<PlayerState>) -> Result<Option<AudioWsUrlsResponse>, String> {
    Ok(state
        .player
        .ws_urls()
        .map(|(events, control)| AudioWsUrlsResponse { events, control }))
}

// ═══════════════════════════════════════════════════════════════════
//  Session-based event polling (kept for backward compat during migration)
// ═══════════════════════════════════════════════════════════════════

#[tauri::command]
pub fn audio_set_session(state: State<PlayerState>, session_id: u64) -> Result<(), String> {
    state.player.set_session(session_id);
    Ok(())
}

#[tauri::command]
pub fn audio_poll_events(
    state: State<PlayerState>,
    session_id: u64,
) -> Result<Vec<AudioThreadEvent>, String> {
    Ok(state.player.poll_events(session_id))
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
