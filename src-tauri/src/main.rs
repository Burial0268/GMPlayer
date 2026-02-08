// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use app::algorithms;
use app::algorithms::chunk::{chunk, ChunkedArray};
use tauri::command;
use tauri::Manager;
use tauri_plugin_decorum::WebviewWindowExt; // adds helper methods to WebviewWindow

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_decorum::init())
        .invoke_handler(tauri::generate_handler![
            get_chunk,
            format_number,
            detect_desktop
        ])
        .setup(|app| {
            let main_window = app.get_webview_window("main").unwrap();
            main_window.create_overlay_titlebar().unwrap();

            // Some macOS-specific helpers
            #[cfg(target_os = "macos")]
            {
                // Set a custom inset to the traffic lights
                main_window.set_traffic_lights_inset(12.0, 16.0).unwrap();

                // Make window transparent without privateApi
                main_window.make_transparent().unwrap();

                // Set window level
                // NSWindowLevel: https://developer.apple.com/documentation/appkit/nswindowlevel
                main_window.set_window_level(25).unwrap();
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[command]
fn detect_desktop() -> bool {
    #[cfg(target_os = "android")]
    {
        return false;
    }
    return true;
}

#[command]
fn get_chunk<T>(input: &[T], size: usize) -> ChunkedArray<T>
where
    T: Clone,
{
    chunk(input, size)
}

#[command]
fn format_number(num: f64, language_data: &str) {
    algorithms::format_number::format_number(num, language_data);
}
