use serde::Serialize;
use tauri::ipc::Channel;
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
/// This is the native playback control path (frontend → Rust); events flow
/// back over the `Channel` registered by `audio_subscribe_events`.
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

/// Register the frontend event `Channel` (Rust → frontend event stream:
/// FFT / status / position). The frontend creates a `Channel`, wires its
/// `onmessage`, and passes it here; the player forwards every
/// `AudioThreadEventMessage` to it. Replaces the old local WebSocket bridge.
#[tauri::command]
pub fn audio_subscribe_events(
    state: State<PlayerState>,
    channel: Channel<AudioThreadEventMessage<AudioThreadEvent>>,
) -> Result<(), String> {
    state.player.set_event_channel(channel);
    Ok(())
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
