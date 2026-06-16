//! Desktop (Windows / macOS / Linux) backend: multi-window management, tray, desktop lyrics.

pub mod window;

use crate::desktop::window::config::WindowConfig;
use crate::desktop::window::desktop_lyrics::mouse_through::{HitRegionRegistry, MouseThroughState};
use crate::desktop::window::manager as wm;
use crate::shared;
use gmplayer_audio_backend::commands;
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
                // Silence symphonia's MP3 bit-reservoir underflow WARN
                // spam. These warnings fire on every track open and after
                // every seek (the decoder's frame buffer needs a few
                // frames to fill up) but are masked entirely by our
                // pre-roll silence in `FFTFeedSource` — they never reach
                // the speakers. The log lines were being mistaken for
                // real glitches; silencing them at Error keeps real
                // decode errors visible while dropping the noise.
                .level_for(
                    "symphonia_bundle_mp3",
                    tauri_plugin_log::log::LevelFilter::Error,
                )
                .level_for("symphonia_core", tauri_plugin_log::log::LevelFilter::Error)
                .build(),
        )
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_decorum::init())
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .manage(MouseThroughState::default())
        .manage(HitRegionRegistry::default())
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
            window::desktop_lyrics::commands::start_mouse_through,
            window::desktop_lyrics::commands::stop_mouse_through,
            window::desktop_lyrics::commands::update_mouse_through_regions,
            // Tray commands
            window::tray::set_tray_tooltip,
            // AutoMix analysis (native Rust, shared by desktop/mobile)
            commands::audio_analyze_automix,
            // AMLL-style: single message command for all playback control
            commands::audio_send_msg,
            // Sync query commands
            commands::audio_get_state,
            commands::audio_get_ws_url,
            commands::audio_get_ws_urls,
            // Session-based event polling (backward compat)
            commands::audio_set_session,
            commands::audio_poll_events,
        ])
        .setup(|app| {
            let app_handle = app.handle().clone();
            app.manage(commands::PlayerState::new(app_handle));

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
