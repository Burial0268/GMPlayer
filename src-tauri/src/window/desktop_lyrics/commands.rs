use tauri::{AppHandle, Emitter, Manager};

/// Set window position to specific physical coordinates.
#[tauri::command]
pub async fn set_window_position(
    app: AppHandle,
    label: String,
    x: i32,
    y: i32,
) -> Result<(), String> {
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;
    window
        .set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }))
        .map_err(|e| e.to_string())
}

/// Emit events when desktop lyrics window moves or resizes.
/// Call this from the main event loop (app.run() closure).
pub fn handle_desktop_lyrics_event(app: &AppHandle, label: &str, event: &tauri::WindowEvent) {
    if label != "desktop-lyrics" {
        return;
    }

    match event {
        tauri::WindowEvent::Moved(position) => {
            let _ = app.emit("desktop-lyrics-moved", (position.x, position.y));
        }
        tauri::WindowEvent::Resized(size) => {
            let _ = app.emit("desktop-lyrics-resized", (size.width, size.height));
        }
        _ => {}
    }
}
