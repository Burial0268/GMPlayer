//! Native audio backend for GMPlayer (Symphonia + rodio, AMLL-style
//! message/event architecture).
//!
//! Commands and the `PlayerState` resource are registered by the host
//! Tauri app in `src-tauri/src/desktop/mod.rs` (and the mobile entry
//! point when added). Consumers should `use gmplayer_audio_backend::commands;`
//! and wire `commands::PlayerState::new(app_handle)` into `app.manage(...)`
//! alongside the four invoke handlers.

pub mod analysis;
mod decoder;
mod effects;
mod error;
mod metadata;
mod output;
mod player;
mod spectrum;
mod types;
mod ws_server;
pub mod commands;

pub use error::{AudioError, AudioResult};
pub use player::Player;
pub use types::{
  AudioInfo, AudioQuality, AudioThreadEvent, AudioThreadEventMessage, AudioThreadMessage,
  DisplayAudioInfo, PlaybackState, SongData, SpectrumConfig, TrackSource,
};
