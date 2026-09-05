//! Mobile (iOS / Android) backend: HTTP, logging, and native media session.

use crate::shared;
use gmplayer_audio_backend::commands;
use tauri::Manager;

#[cfg(target_os = "android")]
static ANDROID_NDK_READY: std::sync::OnceLock<()> = std::sync::OnceLock::new();

#[cfg(target_os = "android")]
static ANDROID_CONTEXT_REF: std::sync::OnceLock<jni::objects::GlobalRef> =
    std::sync::OnceLock::new();

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
        .invoke_handler(tauri::generate_handler![
            shared::detect_desktop,
            shared::desktop_environment,
            // AMLL-style native playback backend. Android uses cpal/rodio's
            // native AAudio path plus Symphonia decoding, matching desktop's
            // message/Channel transport surface.
            commands::audio_send_msg,
            commands::audio_subscribe_events,
            commands::audio_get_state,
            commands::audio_get_session,
            commands::audio_preheat,
            commands::audio_analyze_automix,
            commands::audio_analyze_automix_source,
            commands::audio_set_session,
            commands::audio_poll_events,
            // In-process NCM protocol layer (QuickJS + Rust primitives)
            crate::ncm::ncm_request,
            crate::ncm::ncm_request_projected,
            crate::ncm::ncm_protocol_info,
            crate::ncm::ncm_prefetch,
        ])
        .setup(|app| {
            let app_handle = app.handle().clone();
            #[cfg(target_os = "android")]
            {
                commands::set_android_context_ready_check(android_ndk_context_ready);
                init_android_ndk_context(app);
            }
            app.manage(commands::PlayerState::new(app_handle.clone()));
            // The isolate itself is built lazily on first request, so this only
            // records where its session state lives.
            app.manage(crate::ncm::NcmState::new(&app_handle));
            // ...and this builds it in the background, so the home page's
            // opening fan-out does not queue behind a ~440 ms cold start.
            crate::ncm::warm(&app_handle);
            // Playback source resolution follows the same transport as the UI.
            crate::ncm::install_resolver_hook(&app_handle);
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
        webview.jni_handle().exec(|env, activity, _webview| {
            if ANDROID_NDK_READY.get().is_some() {
                return;
            }

            let vm = match env.get_java_vm() {
                Ok(vm) => vm,
                Err(err) => {
                    warn!("Failed to get Android JavaVM for native audio: {err}");
                    return;
                }
            };

            let app_context = match env.call_method(
                activity,
                "getApplicationContext",
                "()Landroid/content/Context;",
                &[],
            ) {
                Ok(value) => match value.l() {
                    Ok(context) => Some(context),
                    Err(err) => {
                        warn!("Failed to read Android application context: {err}");
                        None
                    }
                },
                Err(err) => {
                    warn!("Failed to get Android application context: {err}");
                    None
                }
            };

            let global_context = match app_context.as_ref() {
                Some(context) => env.new_global_ref(context),
                None => env.new_global_ref(activity),
            };
            let global_context = match global_context {
                Ok(context) => context,
                Err(err) => {
                    warn!("Failed to create Android context global ref: {err}");
                    return;
                }
            };

            let context_ptr = global_context.as_obj().as_raw() as *mut _;
            if ANDROID_CONTEXT_REF.set(global_context).is_err() {
                warn!("Android context global ref was already initialized");
                return;
            }

            unsafe {
                ndk_context::initialize_android_context(
                    vm.get_java_vm_pointer() as *mut _,
                    context_ptr,
                );
            }

            ANDROID_NDK_READY.get_or_init(|| ());
            info!("Android NDK context initialized for native audio");
        });
    }) {
        warn!("Failed to schedule Android NDK context initialization: {err}");
    }
}
