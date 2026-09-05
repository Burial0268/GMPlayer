//! Desktop (Windows / macOS / Linux) backend: multi-window management, tray, desktop lyrics.

#[cfg(target_os = "linux")]
mod linux_graphics;
pub mod window;

use crate::desktop::window::config::WindowConfig;
#[cfg(windows)]
use crate::desktop::window::config::DEFAULT_ADDITIONAL_WINDOW_ARGS;
use crate::desktop::window::desktop_lyrics::mouse_through::{HitRegionRegistry, MouseThroughState};
use crate::desktop::window::manager as wm;
use crate::shared;
use gmplayer_audio_backend::commands;
use log::warn;
use tauri::{Emitter, Manager, RunEvent, WindowEvent};
#[cfg(target_os = "macos")]
use tauri_plugin_decorum::WebviewWindowExt;
use tauri_plugin_window_state::{AppHandleExt, StateFlags, WindowExt};

/// Persist geometry only. Visibility is owned by close-to-tray behavior and
/// decorations are owned by the Rust window preset; restoring either can
/// overwrite the initial native backdrop/frame configuration on Windows.
pub(crate) const WINDOW_STATE_FLAGS: StateFlags = StateFlags::SIZE
    .union(StateFlags::POSITION)
    .union(StateFlags::MAXIMIZED)
    .union(StateFlags::FULLSCREEN);

pub fn run() {
    #[cfg(target_os = "linux")]
    linux_graphics::configure_webkit_gtk_backend();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            let _ = app
                .get_webview_window("main")
                .expect("no main window")
                .set_focus();
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(tauri_plugin_log::log::LevelFilter::Info)
                .level_for(
                    "symphonia_bundle_mp3",
                    tauri_plugin_log::log::LevelFilter::Info,
                )
                .level_for("symphonia_core", tauri_plugin_log::log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_decorum::init())
        .plugin(gmplayer_now_playing_controls::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Pinia 持久化：settingData / siteData / userData 落盘并在主窗口与从
        // 窗口之间同步。前端已经按 300ms debounce 推状态，这里再按 1s debounce
        // 合并写文件，连续调节设置时不会反复落盘。
        .plugin(
            tauri_plugin_pinia::Builder::new()
                .default_save_strategy(tauri_plugin_pinia::SaveStrategy::debounce_millis(1000))
                .autosave(std::time::Duration::from_secs(300))
                .build(),
        )
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(WINDOW_STATE_FLAGS)
                .skip_initial_state("main")
                .build(),
        );

    #[cfg(windows)]
    let builder = builder.plugin(gmplayer_taskbar_lyric::init_with_browser_args(Some(
        DEFAULT_ADDITIONAL_WINDOW_ARGS.to_owned(),
    )));

    let app = builder
        .manage(MouseThroughState::default())
        .manage(HitRegionRegistry::default())
        .invoke_handler(tauri::generate_handler![
            shared::detect_desktop,
            shared::desktop_environment,
            // Window management commands
            window::commands::create_window,
            window::commands::create_custom_window,
            window::commands::create_window_with_payload,
            window::commands::show_window,
            window::commands::hide_window,
            window::commands::close_managed_window,
            window::commands::toggle_window,
            window::commands::focus_window,
            window::commands::get_window_state,
            window::commands::list_windows,
            window::commands::open_window_devtools,
            window::commands::set_window_payload,
            window::commands::take_window_payload,
            window::commands::peek_window_payload,
            window::commands::show_window_at_position,
            window::commands::set_window_effect_color,
            window::commands::set_main_window_effect_theme,
            window::commands::set_ignore_cursor_events,
            window::commands::resize_window,
            window::commands::quit_app,
            window::commands::flush_persisted_state,
            window::commands::get_cursor_position,
            window::commands::get_window_bounds,
            // Desktop lyrics commands
            window::desktop_lyrics::commands::set_window_position,
            window::desktop_lyrics::commands::start_mouse_through,
            window::desktop_lyrics::commands::stop_mouse_through,
            window::desktop_lyrics::commands::update_mouse_through_regions,
            // Tray commands
            window::tray::set_tray_tooltip,
            window::tray::update_tray_popup_layout,
            // AutoMix analysis (native Rust, shared by desktop/mobile)
            commands::audio_analyze_automix,
            commands::audio_analyze_automix_source,
            commands::audio_preheat,
            // AMLL-style: single message command for all playback control
            commands::audio_send_msg,
            // Event stream subscription (Rust → frontend Tauri Channel)
            commands::audio_subscribe_events,
            // Sync query commands
            commands::audio_get_state,
            commands::audio_get_session,
            // Session-based event polling (backward compat)
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
            app.manage(commands::PlayerState::new(app_handle.clone()));
            // The isolate itself is built lazily on first request, so this only
            // records where its session state lives.
            app.manage(crate::ncm::NcmState::new(&app_handle));
            // ...and this builds it in the background, so the home page's
            // opening fan-out does not queue behind a ~440 ms cold start.
            crate::ncm::warm(&app_handle);
            // Playback source resolution follows the same transport as the UI.
            crate::ncm::install_resolver_hook(&app_handle);
            // Drive the OS media session straight from the audio backend, so it
            // stays correct through backend-initiated track advances and does
            // not depend on a live WebView. Queued until the player is created.
            app.state::<commands::PlayerState>()
                .subscribe_events(crate::media::MediaSessionBridge::new(app_handle.clone()));
            // ...and the other direction: SMTC / MPRIS / MPRemoteCommandCenter
            // buttons drive the backend from Rust, so a press does not depend on
            // a page being mounted and listening, and the transport keeps one
            // writer. The frontend follows through `PlayStatus`.
            crate::media::install_controls(&app_handle);

            // Create the primary desktop window from the Rust-side preset.
            // `tauri.conf.json` intentionally has no static windows so desktop
            // and mobile entry points can own their platform-specific startup.
            let mut main_config = WindowConfig::main();
            // Create hidden and restore saved geometry before the frontend
            // initializes the native shell and reveals the window.
            main_config.visible = false;
            if let Err(e) = wm::create_window(&app_handle, &main_config) {
                warn!("Failed to create main window: {}", e);
            }

            #[allow(unused_variables)]
            let Some(main_window) = app.get_webview_window("main") else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "main window was not created",
                )
                .into());
            };

            // macOS-specific helpers that are not part of the generic window preset.
            #[cfg(target_os = "macos")]
            {
                // NSWindowLevel: https://developer.apple.com/documentation/appkit/nswindowlevel
                if let Err(e) = main_window.set_window_level(25) {
                    warn!("Failed to set main window level: {}", e);
                }
            }

            if let Err(e) = main_window.restore_state(WINDOW_STATE_FLAGS) {
                warn!("Failed to restore main window state before show: {}", e);
            }
            // The frontend applies its persisted theme, initializes the matching
            // native material, then reveals the window through
            // `set_main_window_effect_theme`. Keeping it hidden here prevents a
            // light system backdrop from flashing before a dark app theme loads.

            // Set up system tray
            let handle = app.handle().clone();
            if let Err(e) = window::tray::setup_tray(&handle) {
                warn!("Failed to setup system tray: {}", e);
            }

            // Pre-create tray popup (hidden) so it's loaded and ready on first right-click
            let popup_config = WindowConfig::tray_popup();
            if let Err(e) = wm::create_window(&handle, &popup_config) {
                warn!("Failed to pre-create tray popup: {}", e);
            }

            spawn_audio_preheat(app_handle.clone());

            // Never leave the app running invisibly: if the frontend fails
            // before revealing the hidden main window, force-show it.
            let _ = std::thread::Builder::new()
                .name("main-window-reveal-fallback".into())
                .spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(10));
                    wm::reveal_main_window_if_stuck(&app_handle);
                });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let RunEvent::WindowEvent { label, event, .. } = &event {
            // Handle desktop lyrics window events (moved/resized/destroyed)
            window::desktop_lyrics::commands::handle_desktop_lyrics_event(app_handle, label, event);

            #[cfg(windows)]
            if label == "main" && matches!(event, WindowEvent::Destroyed) {
                gmplayer_taskbar_lyric::close_taskbar_lyric(app_handle.clone());
            }

            match (label.as_str(), event) {
                // Main window close → save state, emit to frontend for close-behavior decision
                ("main", WindowEvent::CloseRequested { api, .. }) => {
                    api.prevent_close();
                    let _ = app_handle.save_window_state(WINDOW_STATE_FLAGS);
                    let _ = app_handle.emit("main-close-requested", ());
                }
                // Tray popup loses focus → hide it. Routed through the window
                // manager so the dismissed popup also drops to the low WebView2
                // memory target instead of idling at full footprint.
                ("tray-popup", WindowEvent::Focused(false)) => {
                    if let Err(e) = wm::hide_window(app_handle, "tray-popup") {
                        warn!("Failed to hide tray popup on focus loss: {}", e);
                    }
                }
                (_, WindowEvent::Destroyed) => wm::on_window_destroyed(app_handle, label),
                _ => {}
            }
        }
    });
}

fn spawn_audio_preheat(app_handle: tauri::AppHandle) {
    let _ = std::thread::Builder::new()
        .name("audio-preheat".into())
        .spawn(move || {
            let Some(player_state) = app_handle.try_state::<commands::PlayerState>() else {
                return;
            };
            if let Err(e) = player_state.preheat() {
                warn!("Failed to preheat native audio backend: {}", e);
            }
        });
}
