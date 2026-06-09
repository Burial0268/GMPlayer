use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

/// Set the screen orientation. Android handles this in Kotlin; other mobile
/// targets use this no-op fallback so the frontend can call one command name.
#[tauri::command(rename = "setOrientation")]
fn set_orientation(_orientation: String) -> Result<(), String> {
    Ok(())
}

/// Inline Tauri plugin that delegates screen-orientation control to the
/// Android Kotlin side (`OrientationPlugin`).
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("orientation")
        .invoke_handler(tauri::generate_handler![set_orientation])
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            _api.register_android_plugin("com.gbclstudio.gmplayer", "OrientationPlugin")?;
            Ok(())
        })
        .build()
}
