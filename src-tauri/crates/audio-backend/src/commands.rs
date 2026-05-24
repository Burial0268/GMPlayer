use serde::Serialize;
use tauri::{Runtime, State};

use crate::player::Player;
use crate::types::*;

// ── PlayerState (managed by Tauri) ────────────────────────────────

pub struct PlayerState {
  pub player: std::sync::Arc<Player>,
}

impl PlayerState {
  pub fn new<R: Runtime>(app_handle: tauri::AppHandle<R>) -> Self {
    let player =
      Player::new(app_handle).expect("Failed to create native audio player");
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

/// Send an AudioThreadMessage to the player.
/// This is the PRIMARY IPC method — all playback control flows through it.
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
/// if the bridge failed to bind (e.g. all ports busy). Frontend calls this
/// once on startup and connects; commands and events flow through that
/// socket from then on (Tauri events remain as fallback).
#[tauri::command]
pub fn audio_get_ws_url(state: State<PlayerState>) -> Result<Option<String>, String> {
  Ok(state.player.ws_url())
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
