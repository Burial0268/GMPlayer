use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

/// Set the screen orientation.
/// On Android this is handled by the Kotlin side via the OrientationPlugin;
/// on desktop / iOS this is a no-op.
#[tauri::command]
fn set_screen_orientation(_orientation: String) -> Result<(), String> {
    Ok(())
}

/// Inline Tauri plugin that delegates screen-orientation control to the
/// Android Kotlin side (`OrientationPlugin`).
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("orientation")
        .invoke_handler(tauri::generate_handler![set_screen_orientation])
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            _api.register_android_plugin("com.gbclstudio.gmplayer", "OrientationPlugin")?;
            Ok(())
        })
        .build()
}
