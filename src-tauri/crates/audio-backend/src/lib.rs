//! Native audio backend for GMPlayer (Symphonia + rodio, AMLL-style
//! message/event architecture).
//!
//! Commands and the `PlayerState` resource are registered by the host
//! Tauri app in `src-tauri/src/desktop/mod.rs` (and the mobile entry
//! point when added). Consumers should `use gmplayer_audio_backend::commands;`
//! and wire `commands::PlayerState::new(app_handle)` into `app.manage(...)`
//! alongside the four invoke handlers.

#[cfg(not(target_arch = "wasm32"))]
pub mod analysis;
#[cfg(not(target_arch = "wasm32"))]
pub mod automix;
#[cfg(not(target_arch = "wasm32"))]
pub mod commands;
#[cfg(not(target_arch = "wasm32"))]
mod decoder;
#[cfg(not(target_arch = "wasm32"))]
mod effects;
mod error;
#[cfg(not(target_arch = "wasm32"))]
mod metadata;
#[cfg(not(target_arch = "wasm32"))]
mod output;
#[cfg(not(target_arch = "wasm32"))]
mod player;
#[cfg(not(target_arch = "wasm32"))]
mod rt_priority;
#[cfg(not(target_arch = "wasm32"))]
mod spectrum;
mod types;
#[cfg(target_arch = "wasm32")]
mod wasm;

pub use error::{AudioError, AudioResult};
#[cfg(not(target_arch = "wasm32"))]
pub use player::{Player, PlayerEventSubscriber, SubscriberId};
// The in-process NCM protocol layer lives in the host app (this crate also
// builds for wasm32, where a QuickJS isolate has no place), so it is injected
// rather than depended on. See `player::source_resolver`.
#[cfg(not(target_arch = "wasm32"))]
pub use player::source_resolver::{has_ncm_call_hook, install_ncm_call_hook, NcmCallHook};
pub use types::{
    AudioInfo, AudioQuality, AudioThreadEvent, AudioThreadEventMessage, AudioThreadMessage,
    DisplayAudioInfo, NativePlaybackMode, NativeSessionSnapshot, NowPlayingInfo, PlaybackState,
    SessionControls, SessionControlsPatch, SongData, SpectrumConfig, TrackIdentity,
};
#[cfg(target_arch = "wasm32")]
pub use wasm::WasmAudioBackend;
