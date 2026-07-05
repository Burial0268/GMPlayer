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

/// State flags for window-state plugin — excludes VISIBLE so a previous
/// hide-to-tray state is not restored as a hidden main window on launch.
const WINDOW_STATE_FLAGS: StateFlags = StateFlags::SIZE
    .union(StateFlags::POSITION)
    .union(StateFlags::MAXIMIZED)
    .union(StateFlags::FULLSCREEN)
    .union(StateFlags::DECORATIONS);

pub fn run() {
    #[cfg(target_os = "linux")]
    linux_graphics::configure_webkit_gtk_backend();

    let builder = tauri::Builder::default()
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
        .plugin(tauri_plugin_decorum::init())
        .plugin(gmplayer_now_playing_controls::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
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
            window::commands::set_ignore_cursor_events,
            window::commands::resize_window,
            window::commands::quit_app,
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
            // Session-based event polling (backward compat)
            commands::audio_set_session,
            commands::audio_poll_events,
        ])
        .setup(|app| {
            let app_handle = app.handle().clone();
            app.manage(commands::PlayerState::new(app_handle.clone()));

            // Create the primary desktop window from the Rust-side preset.
            // `tauri.conf.json` intentionally has no static windows so desktop
            // and mobile entry points can own their platform-specific startup.
            let mut main_config = WindowConfig::main();
            // Create hidden, restore saved geometry, then show. Otherwise
            // users see the default window size for one frame before the
            // window-state plugin applies the saved size/position.
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
            if let Err(e) = main_window.show() {
                warn!("Failed to show main window after state restore: {}", e);
            } else {
                let _ = main_window.set_focus();
                let _ = app_handle.emit("main-window-visibility", true);
            }

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

            spawn_audio_preheat(app_handle);

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
                // Tray popup loses focus → hide it
                ("tray-popup", WindowEvent::Focused(false)) => {
                    if let Some(popup) = app_handle.get_webview_window("tray-popup") {
                        let _ = popup.hide();
                    }
                }
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
