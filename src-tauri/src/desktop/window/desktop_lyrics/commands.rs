use tauri::{AppHandle, Emitter, Manager};

use super::mouse_through::HitRegion;

/// Set window position to specific physical coordinates.
#[tauri::command(rename_all = "snake_case")]
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

/// Start global mouse-through detection for the desktop-lyrics window.
///
/// The frontend provides a list of hit regions (logical coordinates within the
/// webview client area). A background thread listens to global mouse movement
/// via `rdev` and emits `mouse-through-state` events to the window whenever
/// the cursor enters or leaves any region.
#[tauri::command(rename_all = "snake_case")]
pub async fn start_mouse_through(
    app: AppHandle,
    label: String,
    regions: Vec<HitRegion>,
) -> Result<(), String> {
    super::mouse_through::start_mouse_through(&app, &label, regions)
}

/// Stop the global mouse-through listener.
#[tauri::command(rename_all = "snake_case")]
pub async fn stop_mouse_through(app: AppHandle, label: String) -> Result<(), String> {
    super::mouse_through::stop_mouse_through(&app, &label)
}

/// Update hit regions without restarting the listener.
#[tauri::command(rename_all = "snake_case")]
pub async fn update_mouse_through_regions(
    app: AppHandle,
    label: String,
    regions: Vec<HitRegion>,
) -> Result<(), String> {
    super::mouse_through::update_hit_regions(&app, &label, regions)
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
