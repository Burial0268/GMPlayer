//! Mobile (iOS / Android) backend: HTTP, logging, and native media session.

use crate::shared;
use gmplayer_audio_backend::commands;
use tauri::Manager;

#[cfg(target_os = "android")]
static ANDROID_NDK_READY: std::sync::OnceLock<()> = std::sync::OnceLock::new();


pub fn run() {
    let mut context = tauri::generate_context!();
    if context.config().app.windows.is_empty() {
        context
            .config_mut()
            .app
            .windows
            .push(tauri::utils::config::WindowConfig {
                ..Default::default()
            });
    }

    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(tauri_plugin_log::log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_opener::init())
        // Pinia 持久化：settingData / siteData / userData 落盘到应用数据目录。
        // 移动端只有一个 webview，同步用不上，但落盘比 WebView 的 localStorage
        // 更稳（后者会被系统清理）。
        .plugin(
            tauri_plugin_pinia::Builder::new()
                .default_save_strategy(tauri_plugin_pinia::SaveStrategy::debounce_millis(1000))
                .autosave(std::time::Duration::from_secs(300))
                .build(),
        )
        // Register the Android MediaNotification / MediaPlaybackService bridge.
        // On non-Android targets this is compiled as a no-op plugin so the same
        // binary can be built for iOS and simulator targets without any changes.
        .plugin(tauri_plugin_media_session::init())
        .plugin(gmplayer_orientation::init())
        // Storage Access Framework bridge: persistable directory grants and
        // file descriptors for the native decoder. Nothing else can give the
        // decoder a `content://` document, and no upstream plugin covers it —
        // `tauri-plugin-dialog` sends `ACTION_GET_CONTENT` (no directory mode,
        // no persistable grant) and `tauri-plugin-persisted-scope` persists
        // Tauri's own ACL rather than the system's URI-permission table.
        .plugin(tauri_plugin_local_files::init())
        .plugin(tauri_plugin_sharekit::init())
        .invoke_handler(tauri::generate_handler![
            shared::detect_desktop,
            shared::desktop_environment,
            // AMLL-style native playback backend. Android uses cpal/rodio's
            // native AAudio path plus Symphonia decoding, matching desktop's
            // message/Channel transport surface.
            commands::audio_send_msg,
            commands::audio_subscribe_events,
            crate::last_session::last_session_get,
            commands::audio_list_output_devices,
            commands::audio_preheat,
            commands::audio_analyze_automix,
            commands::audio_analyze_automix_source,
            // In-process NCM protocol layer (QuickJS + Rust primitives)
            crate::ncm::ncm_request,
            crate::ncm::ncm_request_projected,
            crate::ncm::ncm_protocol_info,
            crate::ncm::ncm_prefetch,
            // Local music library. All of these are app-owned commands under
            // `core:default`; the SAF plugin's own commands are reached from
            // Rust and never appear in a capability file.
            crate::local::local_source_add_directory,
            crate::local::local_source_add_files,
            crate::local::local_source_list,
            crate::local::local_source_rescan,
            crate::local::local_source_remove,
            crate::local::local_scan_cancel,
            crate::local::local_library_list,
            crate::local::local_track_get,
            crate::local::local_tracks_by_song_ids,
            crate::local::local_groups,
            crate::local::local_favourite_set,
            crate::local::local_favourite_list,
            crate::local::local_playlist_list,
            crate::local::local_playlist_create,
            crate::local::local_playlist_update,
            crate::local::local_playlist_delete,
            crate::local::local_playlist_add_tracks,
            crate::local::local_playlist_remove_tracks,
            crate::local::local_playlist_reorder,
            crate::local::local_playlist_import_m3u,
            crate::local::local_playlist_export_m3u,
            crate::local::local_lyric_for,
            crate::local::local_track_detail,
            crate::local::local_track_override_set,
            crate::local::local_track_reprobe,
            crate::local::local_lyric_import_text,
            crate::local::local_lyric_import_file,
            crate::local::local_lyric_clear,
            crate::local::local_cover_import,
            crate::local::local_cover_clear,
            crate::local::local_library_reset,
            // Song downloads. The queue lives in Rust because on Android the
            // WebView that started a batch is routinely killed mid-batch.
            crate::download::download_settings_get,
            crate::download::download_config_set,
            crate::download::download_dir_pick,
            crate::share::share_text,
            crate::share::share_image,
            crate::download::download_subscribe,
            crate::download::download_list,
            crate::download::download_set_credentials,
            crate::download::download_enqueue,
            crate::download::download_pause,
            crate::download::download_resume,
            crate::download::download_retry,
            crate::download::download_cancel,
            crate::download::download_clear_finished,
            crate::storage::storage_usage,
            crate::storage::storage_clear,
            crate::storage::storage_set_audio_budget,
        ])
        .setup(|app| {
            let app_handle = app.handle().clone();
            #[cfg(target_os = "android")]
            {
                commands::set_android_context_ready_check(android_ndk_context_ready);
                init_android_ndk_context(app);
            }
            // Before the player exists: its first download already lands in the audio cache.
            crate::storage::install(&app_handle);
            app.manage(commands::PlayerState::new());
            // Before any page subscribes: a cold boot asks it what the last process played.
            crate::last_session::install(&app_handle);
            // The isolate itself is built lazily on first request, so this only
            // records where its session state lives.
            app.manage(crate::ncm::NcmState::new(&app_handle));
            // ...and this builds it in the background, so the home page's
            // opening fan-out does not queue behind a ~440 ms cold start.
            crate::ncm::warm(&app_handle);
            // Playback source resolution follows the same transport as the UI.
            crate::ncm::install_resolver_hook(&app_handle);
            // The local music library. `install` also teaches the audio backend
            // how to open a `content://` document, which every Android local
            // track needs before it can resolve at all.
            app.manage(crate::local::LocalLibraryState::new(&app_handle));
            crate::local::install(&app_handle);
            // The download queue. Managed here rather than lazily so the settings
            // page can read the destination before anything has been downloaded.
            app.manage(crate::download::DownloadState::new(&app_handle));
            // Drive the Android MediaSession straight from the audio backend:
            // the WebView is destroyed under memory pressure while playback
            // continues, so a JS-driven notification freezes on a stale track.
            app.state::<commands::PlayerState>()
                .subscribe_events(crate::media::MediaSessionBridge::new(app_handle.clone()));
            // …and take its buttons back the same way, for the same reason.
            #[cfg(target_os = "android")]
            crate::media::install_controls(&app_handle);
            Ok(())
        })
        .build(context)
        .expect("error while building tauri application")
        .run(|_app_handle, _event| {});
}

#[cfg(target_os = "android")]
fn android_ndk_context_ready() -> bool {
    ANDROID_NDK_READY.get().is_some()
}

/// tao (0.37+) initializes `ndk_context` with the application context in the
/// Activity's `onCreate`, and `ndk_context` aborts the process on a second
/// `initialize_android_context` — so this only reports readiness, from the
/// Activity thread once the WebView exists, which is after that `onCreate`.
#[cfg(target_os = "android")]
fn init_android_ndk_context(app: &mut tauri::App) {
    use log::{info, warn};

    if ANDROID_NDK_READY.get().is_some() {
        return;
    }

    let Some(webview) = app.get_webview_window("main") else {
        warn!("Main webview not found; Android NDK context initialization is delayed");
        return;
    };

    if let Err(err) = webview.with_webview(|webview| {
        webview.jni_handle().exec(|_env, _activity, _webview| {
            ANDROID_NDK_READY.get_or_init(|| ());
            info!("Android NDK context ready for native audio");
        });
    }) {
        warn!("Failed to schedule Android NDK context initialization: {err}");
    }
}
