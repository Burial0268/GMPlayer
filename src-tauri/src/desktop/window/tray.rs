use log::{info, warn};
use std::sync::{Mutex, OnceLock};
use tauri::image::Image;
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Rect, WebviewWindow};

use crate::desktop::window::config::{WindowConfig, TRAY_POPUP_BASE_HEIGHT, TRAY_POPUP_WIDTH};
use crate::desktop::window::manager as wm;

const TRAY_ID: &str = "main";
const TRAY_POPUP_MIN_WIDTH: f64 = 220.0;
const TRAY_POPUP_MAX_WIDTH: f64 = 420.0;
const TRAY_POPUP_MIN_HEIGHT: f64 = 260.0;
const TRAY_POPUP_MAX_HEIGHT: f64 = 560.0;
const TRAY_POPUP_GAP: f64 = 8.0;

static TRAY_POPUP_SIZE: OnceLock<Mutex<PopupSize>> = OnceLock::new();
static TRAY_POPUP_ANCHOR: OnceLock<Mutex<Option<PhysicalRect>>> = OnceLock::new();

#[derive(Debug, Clone, Copy)]
struct PopupSize {
    width: f64,
    height: f64,
}

#[derive(Debug, Clone, Copy)]
struct PhysicalRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Debug, Clone, Copy)]
struct MonitorBounds {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
    scale_factor: f64,
}

#[derive(Debug, Clone, Copy)]
enum ScreenEdge {
    Top,
    Bottom,
    Left,
    Right,
}

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

/// Update the tray popup size from the rendered Web UI and keep it anchored to the tray icon.
#[tauri::command]
pub fn update_tray_popup_layout(app: AppHandle, width: f64, height: f64) -> Result<(), String> {
    let size = sanitize_popup_size(width, height);
    remember_popup_size(size);

    let Some(popup) = app.get_webview_window("tray-popup") else {
        return Ok(());
    };

    popup
        .set_size(LogicalSize::new(size.width, size.height))
        .map_err(|e| e.to_string())?;

    if let Some(anchor) = current_anchor() {
        position_popup_window(&popup, anchor, size)?;
    }

    Ok(())
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
        let size = current_popup_size();
        let _ = popup.set_size(LogicalSize::new(size.width, size.height));
    }

    // Scale factor: config dimensions are logical, but tray rect and
    // show_window_at_position use physical pixels. We must convert.
    let scale_factor = popup
        .as_ref()
        .and_then(|w| w.scale_factor().ok())
        .unwrap_or(1.0);

    let anchor = rect_to_physical(rect, scale_factor);
    remember_anchor(anchor);

    let (x, y) = if let Some(ref popup_win) = popup {
        calculate_popup_position(popup_win, anchor, current_popup_size())
    } else {
        (
            anchor.x + (anchor.width / 2.0) - (config.width * scale_factor / 2.0),
            anchor.y - (config.height * scale_factor) - (TRAY_POPUP_GAP * scale_factor),
        )
    };

    wm::show_window_at_position(app, "tray-popup", x, y)?;

    // Notify the popup to request fresh player state
    let _ = app.emit("tray-popup-opened", ());

    Ok(())
}

fn popup_size_state() -> &'static Mutex<PopupSize> {
    TRAY_POPUP_SIZE.get_or_init(|| {
        Mutex::new(PopupSize {
            width: TRAY_POPUP_WIDTH,
            height: TRAY_POPUP_BASE_HEIGHT,
        })
    })
}

fn anchor_state() -> &'static Mutex<Option<PhysicalRect>> {
    TRAY_POPUP_ANCHOR.get_or_init(|| Mutex::new(None))
}

fn sanitize_popup_size(width: f64, height: f64) -> PopupSize {
    let width = if width.is_finite() {
        width.clamp(TRAY_POPUP_MIN_WIDTH, TRAY_POPUP_MAX_WIDTH)
    } else {
        TRAY_POPUP_WIDTH
    };
    let height = if height.is_finite() {
        height.clamp(TRAY_POPUP_MIN_HEIGHT, TRAY_POPUP_MAX_HEIGHT)
    } else {
        TRAY_POPUP_BASE_HEIGHT
    };
    PopupSize { width, height }
}

fn remember_popup_size(size: PopupSize) {
    if let Ok(mut cached) = popup_size_state().lock() {
        *cached = size;
    }
}

fn current_popup_size() -> PopupSize {
    popup_size_state()
        .lock()
        .map(|size| *size)
        .unwrap_or(PopupSize {
            width: TRAY_POPUP_WIDTH,
            height: TRAY_POPUP_BASE_HEIGHT,
        })
}

fn remember_anchor(anchor: PhysicalRect) {
    if let Ok(mut cached) = anchor_state().lock() {
        *cached = Some(anchor);
    }
}

fn current_anchor() -> Option<PhysicalRect> {
    anchor_state().lock().ok().and_then(|anchor| *anchor)
}

fn rect_to_physical(rect: &Rect, scale_factor: f64) -> PhysicalRect {
    let (x, y) = match &rect.position {
        tauri::Position::Physical(pos) => (pos.x as f64, pos.y as f64),
        tauri::Position::Logical(pos) => (pos.x * scale_factor, pos.y * scale_factor),
    };
    let (width, height) = match &rect.size {
        tauri::Size::Physical(size) => (size.width as f64, size.height as f64),
        tauri::Size::Logical(size) => (size.width * scale_factor, size.height * scale_factor),
    };

    PhysicalRect {
        x,
        y,
        width: width.max(1.0),
        height: height.max(1.0),
    }
}

fn position_popup_window(
    popup: &WebviewWindow,
    anchor: PhysicalRect,
    size: PopupSize,
) -> Result<(), String> {
    let (x, y) = calculate_popup_position(popup, anchor, size);
    popup
        .set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
        .map_err(|e| e.to_string())
}

fn calculate_popup_position(
    popup: &WebviewWindow,
    anchor: PhysicalRect,
    size: PopupSize,
) -> (f64, f64) {
    let fallback_scale = popup.scale_factor().unwrap_or(1.0);
    let Some(monitor) = target_monitor_bounds(popup, anchor) else {
        let popup_width = size.width * fallback_scale;
        let popup_height = size.height * fallback_scale;
        return (
            anchor.x + (anchor.width / 2.0) - (popup_width / 2.0),
            anchor.y - popup_height - (TRAY_POPUP_GAP * fallback_scale),
        );
    };

    let popup_width = size.width * monitor.scale_factor;
    let popup_height = size.height * monitor.scale_factor;
    let gap = TRAY_POPUP_GAP * monitor.scale_factor;
    let edge = nearest_screen_edge(anchor, monitor);
    let anchor_center_x = anchor.x + anchor.width / 2.0;
    let anchor_center_y = anchor.y + anchor.height / 2.0;

    let (mut x, mut y) = match edge {
        ScreenEdge::Top => (
            anchor_center_x - popup_width / 2.0,
            anchor.y + anchor.height + gap,
        ),
        ScreenEdge::Bottom => (
            anchor_center_x - popup_width / 2.0,
            anchor.y - popup_height - gap,
        ),
        ScreenEdge::Left => (
            anchor.x + anchor.width + gap,
            anchor_center_y - popup_height / 2.0,
        ),
        ScreenEdge::Right => (
            anchor.x - popup_width - gap,
            anchor_center_y - popup_height / 2.0,
        ),
    };

    x = x.clamp(
        monitor.left,
        (monitor.right - popup_width).max(monitor.left),
    );
    y = y.clamp(
        monitor.top,
        (monitor.bottom - popup_height).max(monitor.top),
    );

    (x, y)
}

fn target_monitor_bounds(popup: &WebviewWindow, anchor: PhysicalRect) -> Option<MonitorBounds> {
    let monitors = popup.available_monitors().ok()?;
    let anchor_center_x = anchor.x + anchor.width / 2.0;
    let anchor_center_y = anchor.y + anchor.height / 2.0;

    let target = monitors
        .iter()
        .find(|monitor| {
            let bounds = monitor_bounds(monitor);
            anchor_center_x >= bounds.left
                && anchor_center_x < bounds.right
                && anchor_center_y >= bounds.top
                && anchor_center_y < bounds.bottom
        })
        .or_else(|| {
            monitors.iter().min_by(|a, b| {
                let da = distance_to_monitor(anchor_center_x, anchor_center_y, monitor_bounds(a));
                let db = distance_to_monitor(anchor_center_x, anchor_center_y, monitor_bounds(b));
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
        })?;

    Some(monitor_bounds(target))
}

fn monitor_bounds(monitor: &tauri::Monitor) -> MonitorBounds {
    let position = monitor.position();
    let size = monitor.size();
    let left = position.x as f64;
    let top = position.y as f64;
    MonitorBounds {
        left,
        top,
        right: left + size.width as f64,
        bottom: top + size.height as f64,
        scale_factor: monitor.scale_factor(),
    }
}

fn distance_to_monitor(x: f64, y: f64, monitor: MonitorBounds) -> f64 {
    let dx = if x < monitor.left {
        monitor.left - x
    } else if x > monitor.right {
        x - monitor.right
    } else {
        0.0
    };
    let dy = if y < monitor.top {
        monitor.top - y
    } else if y > monitor.bottom {
        y - monitor.bottom
    } else {
        0.0
    };
    dx * dx + dy * dy
}

fn nearest_screen_edge(anchor: PhysicalRect, monitor: MonitorBounds) -> ScreenEdge {
    let left = (anchor.x - monitor.left).abs();
    let right = (monitor.right - (anchor.x + anchor.width)).abs();
    let top = (anchor.y - monitor.top).abs();
    let bottom = (monitor.bottom - (anchor.y + anchor.height)).abs();

    let mut edge = ScreenEdge::Top;
    let mut distance = top;
    for (candidate, candidate_distance) in [
        (ScreenEdge::Bottom, bottom),
        (ScreenEdge::Left, left),
        (ScreenEdge::Right, right),
    ] {
        if candidate_distance < distance {
            edge = candidate;
            distance = candidate_distance;
        }
    }

    edge
}
