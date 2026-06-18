mod commands;

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
