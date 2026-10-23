mod commands;

pub use commands::{
    apply_metadata, apply_play_mode, apply_play_state, apply_timeline, fetch_cover_blocking,
    on_action, NowPlayingState,
};

/// Re-exported so callers can name a repeat mode without depending on the
/// system-media crate directly.
pub use now_playing_controls::model::RepeatMode;

/// Re-exported for [`on_action`] consumers: an in-process handler matches on
/// these rather than on the wire strings the frontend path uses, so adding a
/// system-media action is a compile error at the consumer instead of an
/// unrecognised string at runtime.
pub use now_playing_controls::model::{SystemMediaEvent, SystemMediaEventType};

use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("now-playing-controls")
        .invoke_handler(tauri::generate_handler![
            commands::clear,
            commands::initialize,
            commands::set_enabled,
            commands::update_play_mode,
            commands::update_state,
            commands::update_timeline,
        ])
        .setup(|app, _api| {
            app.manage(commands::NowPlayingState::default());
            Ok(())
        })
        .build()
}
