use log::{info, warn};
use tauri::image::Image;
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, Rect};

use crate::window::config::WindowConfig;
use crate::window::manager as wm;

const TRAY_ID: &str = "main";

/// Set up the system tray icon (no native menu — right-click shows a WebviewWindow popup).
pub fn setup_tray(app: &AppHandle) -> Result<(), String> {
    // Load tray icon from bundled icons
    let icon = app.default_window_icon().cloned().unwrap_or_else(|| {
        warn!("No default window icon found, using empty icon");
        Image::new(&[], 0, 0)
    });

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("GMPlayer")
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button, rect, .. } = event {
                let app = tray.app_handle();
                match button {
                    MouseButton::Left => {
                        // Always show main window (never toggle)
                        if let Err(e) = wm::show_window(app, "main") {
                            warn!("Failed to show main window: {}", e);
                        }
                    }
                    MouseButton::Right => {
                        if let Err(e) = show_tray_popup(app, &rect) {
                            warn!("Failed to show tray popup: {}", e);
                        }
                    }
                    _ => {}
                }
            }
        })
        .build(app)
        .map_err(|e| e.to_string())?;

    info!("System tray initialized (popup mode)");
    Ok(())
}

/// Update the tray icon tooltip (e.g., "Song Name - Artist").
/// Call this from JS when the playing song changes.
#[tauri::command]
pub fn set_tray_tooltip(app: AppHandle, text: String) -> Result<(), String> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_tooltip(Some(&text)).map_err(|e| e.to_string())
    } else {
        Err("Tray icon not found".into())
    }
}

/// Show the tray popup window near the tray icon.
/// The popup is pre-created (hidden) during app setup. If it somehow doesn't
/// exist yet, it is created lazily here as a fallback.
fn show_tray_popup(app: &AppHandle, rect: &Rect) -> Result<(), String> {
    let config = WindowConfig::tray_popup();

    // Fallback: create the popup if it doesn't exist yet
    if app.get_webview_window("tray-popup").is_none() {
        wm::create_window(app, &config)?;
    }

    // Force correct size — window-state plugin may have restored old dimensions
    let popup = app.get_webview_window("tray-popup");
    if let Some(ref popup) = popup {
        let _ = popup.set_size(LogicalSize::new(config.width, config.height));
    }

    // Scale factor: config dimensions are logical, but tray rect and
    // show_window_at_position use physical pixels. We must convert.
    let scale_factor = popup
        .as_ref()
        .and_then(|w| w.scale_factor().ok())
        .unwrap_or(1.0);

    // Extract physical position and size from the tray icon rect
    let (icon_x, icon_y) = match &rect.position {
        tauri::Position::Physical(pos) => (pos.x as f64, pos.y as f64),
        tauri::Position::Logical(pos) => (pos.x * scale_factor, pos.y * scale_factor),
    };
    let (icon_w, icon_h) = match &rect.size {
        tauri::Size::Physical(size) => (size.width as f64, size.height as f64),
        tauri::Size::Logical(size) => (size.width * scale_factor, size.height * scale_factor),
    };

    // Popup dimensions in physical pixels
    let popup_width = config.width * scale_factor;
    let popup_height = config.height * scale_factor;
    let gap = 8.0 * scale_factor;

    // Position: center horizontally above the tray icon (Windows), with gap
    let mut x = icon_x + (icon_w / 2.0) - (popup_width / 2.0);

    // On Windows the tray is at the bottom — position popup above the icon
    // On macOS the tray is at the top — position popup below the icon
    let mut y = if cfg!(target_os = "macos") {
        icon_y + icon_h + gap
    } else {
        icon_y - popup_height - gap
    };

    // Clamp to screen bounds so the popup (especially the quit button) stays visible
    if let Some(ref popup_win) = popup {
        if let Ok(monitors) = popup_win.available_monitors() {
            // Find the monitor containing the tray icon
            let target = monitors.iter().find(|m| {
                let p = m.position();
                let s = m.size();
                let (ml, mt) = (p.x as f64, p.y as f64);
                let (mr, mb) = (ml + s.width as f64, mt + s.height as f64);
                icon_x >= ml && icon_x < mr && icon_y >= mt && icon_y < mb
            });

            if let Some(monitor) = target {
                let mp = monitor.position();
                let ms = monitor.size();
                let mon_left = mp.x as f64;
                let mon_top = mp.y as f64;
                let mon_right = mon_left + ms.width as f64;
                let mon_bottom = mon_top + ms.height as f64;

                x = x.clamp(mon_left, (mon_right - popup_width).max(mon_left));
                y = y.clamp(mon_top, (mon_bottom - popup_height).max(mon_top));
            }
        }
    }

    wm::show_window_at_position(app, "tray-popup", x, y)?;

    // Notify the popup to request fresh player state
    let _ = app.emit("tray-popup-opened", ());

    Ok(())
}
