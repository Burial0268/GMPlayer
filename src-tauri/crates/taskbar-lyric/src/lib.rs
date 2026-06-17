use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

#[cfg(windows)]
pub mod mouse_forward;
#[cfg(windows)]
mod taskbar;
#[cfg(windows)]
pub mod webview_finder;

#[cfg(windows)]
pub use taskbar::{
    close_taskbar_lyric, open_taskbar_lyric, open_taskbar_lyric_devtools, TaskbarLyricState,
};

#[cfg(windows)]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("taskbar-lyric")
        .invoke_handler(tauri::generate_handler![
            close_taskbar_lyric,
            open_taskbar_lyric,
            open_taskbar_lyric_devtools,
            mouse_forward::set_click_interception,
            mouse_forward::set_forwarding_enabled,
            mouse_forward::stop_mouse_hook,
        ])
        .setup(|app, _api| {
            app.manage(TaskbarLyricState::default());
            Ok(())
        })
        .build()
}

#[cfg(not(windows))]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("taskbar-lyric").build()
}
