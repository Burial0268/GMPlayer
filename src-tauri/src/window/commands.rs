use serde::Serialize;
use serde_json::Value;
use tauri::{command, AppHandle, Manager};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

use crate::window::config::WindowConfig;
use crate::window::manager;
use crate::window::payload::PayloadCache;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub exists: bool,
    pub visible: bool,
}

/// Create a window from a preset label (e.g. "settings", "mini-player").
#[command]
pub async fn create_window(app: AppHandle, label: String) -> Result<(), String> {
    let config = WindowConfig::from_label(&label)
        .ok_or_else(|| format!("No preset found for label '{}'", label))?;
    manager::create_window(&app, &config)
}

/// Create a window with a fully custom configuration.
#[command]
pub async fn create_custom_window(app: AppHandle, config: WindowConfig) -> Result<(), String> {
    manager::create_window(&app, &config)
}

/// Create a window from a preset label, with an attached payload.
#[command]
pub async fn create_window_with_payload(
    app: AppHandle,
    label: String,
    payload: Value,
) -> Result<(), String> {
    PayloadCache::set(&label, payload);
    let config = WindowConfig::from_label(&label)
        .ok_or_else(|| format!("No preset found for label '{}'", label))?;
    manager::create_window(&app, &config)
}

/// Show a window by label.
#[command]
pub async fn show_window(app: AppHandle, label: String) -> Result<(), String> {
    manager::show_window(&app, &label)
}

/// Hide a window by label.
#[command]
pub async fn hide_window(app: AppHandle, label: String) -> Result<(), String> {
    manager::hide_window(&app, &label)
}

/// Close a window by label (respects closeable_to_tray).
#[command]
pub async fn close_managed_window(app: AppHandle, label: String) -> Result<(), String> {
    manager::close_window(&app, &label)
}

/// Toggle window visibility.
#[command]
pub async fn toggle_window(app: AppHandle, label: String) -> Result<(), String> {
    manager::toggle_window(&app, &label)
}

/// Focus a window by label.
#[command]
pub async fn focus_window(app: AppHandle, label: String) -> Result<(), String> {
    manager::focus_window(&app, &label)
}

/// Get the state (exists, visible) of a window.
#[command]
pub async fn get_window_state(app: AppHandle, label: String) -> Result<WindowState, String> {
    let exists = manager::window_exists(&app, &label);
    let visible = if exists {
        manager::is_window_visible(&app, &label)?
    } else {
        false
    };
    Ok(WindowState { exists, visible })
}

/// List all open window labels.
#[command]
pub async fn list_windows(app: AppHandle) -> Vec<String> {
    manager::list_windows(&app)
}

/// Store a payload in the cache for a window label.
#[command]
pub async fn set_window_payload(label: String, payload: Value) -> Result<(), String> {
    PayloadCache::set(&label, payload);
    Ok(())
}

/// Take (consume) a payload from the cache.
#[command]
pub async fn take_window_payload(label: String) -> Option<Value> {
    PayloadCache::take(&label)
}

/// Peek at a payload without consuming it.
#[command]
pub async fn peek_window_payload(label: String) -> Option<Value> {
    PayloadCache::peek(&label)
}

/// Show a window at a specific screen position (physical pixels).
#[command]
pub async fn show_window_at_position(
    app: AppHandle,
    label: String,
    x: f64,
    y: f64,
) -> Result<(), String> {
    manager::show_window_at_position(&app, &label, x, y)
}

/// Update the native window effect tint color (e.g. Acrylic on Windows).
#[command]
pub async fn set_window_effect_color(
    app: AppHandle,
    label: String,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) -> Result<(), String> {
    manager::set_window_effect_color(&app, &label, r, g, b, a)
}

/// Set whether a window ignores cursor events (click-through).
#[command]
pub async fn set_ignore_cursor_events(
    app: AppHandle,
    label: String,
    ignore: bool,
) -> Result<(), String> {
    manager::set_ignore_cursor_events(&app, &label, ignore)
}

/// Resize a window to a logical size.
#[command]
pub async fn resize_window(
    app: AppHandle,
    label: String,
    width: f64,
    height: f64,
) -> Result<(), String> {
    manager::resize_window(&app, &label, width, height)
}

/// Quit the application after saving window state.
/// Excludes VISIBLE flag so the main window always starts hidden on next launch
/// (the frontend controls when to show it, preventing a blank-window flash).
#[command]
pub async fn quit_app(app: AppHandle) -> Result<(), String> {
    let flags = StateFlags::SIZE
        | StateFlags::POSITION
        | StateFlags::MAXIMIZED
        | StateFlags::FULLSCREEN
        | StateFlags::DECORATIONS;
    let _ = app.save_window_state(flags);
    app.exit(0);
    Ok(())
}

/// Get the current screen cursor position (physical pixels).
#[command]
pub fn get_cursor_position() -> Result<(i32, i32), String> {
    #[cfg(target_os = "windows")]
    {
        use std::mem::MaybeUninit;
        #[repr(C)]
        struct POINT {
            x: i32,
            y: i32,
        }
        extern "system" {
            fn GetCursorPos(lp_point: *mut POINT) -> i32;
        }
        unsafe {
            let mut pt = MaybeUninit::<POINT>::uninit();
            if GetCursorPos(pt.as_mut_ptr()) != 0 {
                let pt = pt.assume_init();
                Ok((pt.x, pt.y))
            } else {
                Err("GetCursorPos failed".into())
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        use core_graphics::event::CGEvent;
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .map_err(|_| "Failed to create CGEventSource".to_string())?;
        let event = CGEvent::new(source)
            .map_err(|_| "Failed to create CGEvent".to_string())?;
        let loc = event.location();
        Ok((loc.x as i32, loc.y as i32))
    }

    #[cfg(target_os = "linux")]
    {
        Err("get_cursor_position is not yet supported on Linux".into())
    }
}

/// Get a window's outer position and size (physical pixels).
#[command]
pub async fn get_window_bounds(
    app: AppHandle,
    label: String,
) -> Result<(i32, i32, u32, u32), String> {
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    Ok((pos.x, pos.y, size.width, size.height))
}
