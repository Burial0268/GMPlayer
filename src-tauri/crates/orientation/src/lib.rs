use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

/// Set the screen orientation.
///
/// This exists only for mobile targets that have no native implementation
/// (currently iOS). Android MUST NOT reach this: see [`init`].
#[cfg(not(target_os = "android"))]
#[tauri::command(rename = "setOrientation")]
fn set_orientation(_orientation: String) -> Result<(), String> {
    Ok(())
}

/// Inline Tauri plugin that delegates screen-orientation control to the
/// Android Kotlin side (`OrientationPlugin`).
///
/// The Android command is deliberately *not* registered in an
/// `invoke_handler`. Tauri only forwards an IPC message to a native mobile
/// plugin when the Rust side reports the command as unhandled
/// (`Webview::on_message` -> `extend_api` returns `false` -> `mobile::run_command`).
/// A Rust handler for `setOrientation` therefore shadows the Kotlin
/// implementation and silently resolves without ever touching the Activity,
/// which is exactly what made orientation changes no-ops on Android.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    let builder = Builder::new("orientation");

    #[cfg(not(target_os = "android"))]
    let builder = builder.invoke_handler(tauri::generate_handler![set_orientation]);

    builder
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            _api.register_android_plugin("com.gbclstudio.gmplayer", "OrientationPlugin")?;
            Ok(())
        })
        .build()
}
