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
pub mod source;
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
// The `content://` bridge is injected for the same reason: SAF lives in the
// host app's Android plugin, and this crate must stay buildable for wasm32.
#[cfg(not(target_arch = "wasm32"))]
pub use source::{
    has_content_source_provider, install_content_source_provider, set_stream_cache_dir, ContentFd,
    ContentSourceProvider,
};
// Library-scan tag extraction. Lives here rather than in the host app because it
// has to go through `source::open` to see an Android `content://` document at
// all, and symphonia belongs to exactly one crate.
#[cfg(not(target_arch = "wasm32"))]
pub use metadata::{extract_track_tags, CoverArt, TrackTags};
// Local favourites. A separate set from the Netease like list on purpose — see
// `LocalFavouriteStore`'s doc comment for the login that would otherwise erase
// them.
#[cfg(not(target_arch = "wasm32"))]
pub use player::session_controls::{
    has_local_favourite_store, install_local_favourite_store, LocalFavouriteStore,
};
pub use types::{
    AudioInfo, AudioQuality, AudioThreadEvent, AudioThreadEventMessage, AudioThreadMessage,
    DisplayAudioInfo, NativePlaybackMode, NativeSessionSnapshot, NowPlayingInfo, PlaybackState,
    SessionControls, SessionControlsPatch, SongData, SpectrumConfig, TrackIdentity,
};
#[cfg(target_arch = "wasm32")]
pub use wasm::WasmAudioBackend;
