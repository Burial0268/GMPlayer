//! Desktop (Windows / macOS / Linux) backend: multi-window management, tray, desktop lyrics.

pub mod window;

use crate::algorithms::audio_analysis;
use crate::desktop::window::config::WindowConfig;
use crate::desktop::window::manager as wm;
use crate::shared;
use log::warn;
use tauri::{Emitter, Manager, RunEvent, WindowEvent};
#[cfg(target_os = "macos")]
use tauri_plugin_decorum::WebviewWindowExt;
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

/// State flags for window-state plugin — excludes VISIBLE so the main window
/// always starts hidden and the frontend controls when to show it.
const WINDOW_STATE_FLAGS: StateFlags = StateFlags::SIZE
    .union(StateFlags::POSITION)
    .union(StateFlags::MAXIMIZED)
    .union(StateFlags::FULLSCREEN)
    .union(StateFlags::DECORATIONS);

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(tauri_plugin_log::log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_decorum::init())
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            shared::detect_desktop,
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
            // Tray commands
            window::tray::set_tray_tooltip,
            // Audio analysis (native Rust, bypasses JS Worker)
            audio_analysis::analyze_audio_native,
        ])
        .setup(|app| {
            #[allow(unused_variables)]
            let main_window = app.get_webview_window("main").unwrap();

            // macOS-specific helpers
            #[cfg(target_os = "macos")]
            {
                main_window.create_overlay_titlebar().unwrap();

                // Set a custom inset to the traffic lights
                main_window.set_traffic_lights_inset(12.0, 16.0).unwrap();

                // Make window transparent without privateApi
                main_window.make_transparent().unwrap();

                // Set window level
                // NSWindowLevel: https://developer.apple.com/documentation/appkit/nswindowlevel
                main_window.set_window_level(25).unwrap();
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

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let RunEvent::WindowEvent { label, event, .. } = &event {
            // Handle desktop lyrics window events (moved/resized/destroyed)
            window::desktop_lyrics::commands::handle_desktop_lyrics_event(app_handle, label, event);

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
