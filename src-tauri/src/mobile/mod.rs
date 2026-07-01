//! Mobile (iOS / Android) backend: HTTP, logging, and native media session.

use crate::shared;
use gmplayer_audio_backend::commands;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(tauri_plugin_log::log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_http::init())
        // Register the Android MediaNotification / MediaPlaybackService bridge.
        // On non-Android targets this is compiled as a no-op plugin so the same
        // binary can be built for iOS and simulator targets without any changes.
        .plugin(tauri_plugin_media_session::init())
        .plugin(gmplayer_orientation::init())
        .invoke_handler(tauri::generate_handler![
            shared::detect_desktop,
            shared::desktop_environment,
            // AMLL-style native playback backend. Android uses cpal/rodio's
            // native AAudio path plus Symphonia decoding, matching desktop's
            // message/Channel transport surface.
            commands::audio_send_msg,
            commands::audio_subscribe_events,
            commands::audio_get_state,
            commands::audio_analyze_automix,
            commands::audio_analyze_automix_source,
            commands::audio_set_session,
            commands::audio_poll_events,
        ])
        .setup(|app| {
            let app_handle = app.handle().clone();
            app.manage(commands::PlayerState::new(app_handle));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, _event| {});
}
