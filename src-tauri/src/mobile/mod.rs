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
            commands::audio_preheat,
            commands::audio_analyze_automix,
            commands::audio_analyze_automix_source,
            commands::audio_set_session,
            commands::audio_poll_events,
        ])
        .setup(|app| {
            let app_handle = app.handle().clone();
            #[cfg(target_os = "android")]
            {
                commands::set_android_context_ready_check(android_ndk_context_ready);
                init_android_ndk_context(app);
            }
            app.manage(commands::PlayerState::new(app_handle));
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
