use crate::window::config::WindowConfig;
use log::info;
use tauri::window::EffectsBuilder;
#[cfg(target_os = "windows")]
use tauri::window::{Color, Effect};
#[cfg(target_os = "macos")]
use tauri::window::{Effect, EffectState};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

#[cfg(target_os = "macos")]
use tauri_plugin_decorum::WebviewWindowExt;

/// Create or focus a window from a `WindowConfig`.
///
/// If `config.single_instance` is true and a window with the same label already
/// exists, it is shown and focused instead of creating a duplicate.
pub fn create_window(app: &AppHandle, config: &WindowConfig) -> Result<(), String> {
    let label = &config.label;

    // Single-instance check: focus existing window if it exists
    if config.single_instance {
        if let Some(existing) = app.get_webview_window(label) {
            info!("Window '{}' already exists, focusing", label);
            existing.show().map_err(|e| e.to_string())?;
            if existing.is_minimized().unwrap_or(false) {
                existing.unminimize().map_err(|e| e.to_string())?;
            }
            existing.set_focus().map_err(|e| e.to_string())?;
            return Ok(());
        }
    }

    info!("Creating window '{}'", label);

    let url = WebviewUrl::App(config.url.clone().into());
    let mut builder = WebviewWindowBuilder::new(app, label, url)
        .title(&config.title)
        .inner_size(config.width, config.height)
        .resizable(config.resizable)
        .decorations(config.decorations)
        .transparent(config.transparent)
        .always_on_top(config.always_on_top)
        .skip_taskbar(config.skip_taskbar)
        .visible(config.visible)
        .shadow(config.shadow);

    if let Some(min_w) = config.min_width {
        if let Some(min_h) = config.min_height {
            builder = builder.min_inner_size(min_w, min_h);
        }
    }

    if let Some(max_w) = config.max_width {
        if let Some(max_h) = config.max_height {
            builder = builder.max_inner_size(max_w, max_h);
        }
    }

    if config.center {
        builder = builder.center();
    }

    // Handle parent window relationship for child windows
    if let Some(ref parent_label) = config.parent_label {
        if let Some(parent_window) = app.get_webview_window(parent_label) {
            builder = builder.parent(&parent_window).map_err(|e| e.to_string())?;
        } else {
            return Err(format!("Parent window '{}' not found for '{}'", parent_label, label));
        }
    }

    let _window = builder.build().map_err(|e| e.to_string())?;

    // Apply native window effects (acrylic, mica, etc.) if configured.
    // Uses set_effects() on the built window because WebviewWindowBuilder
    // does not reliably pass effects to the underlying WindowBuilder.
    if let Some(ref effect_name) = config.window_effect {
        if let Some(effects) = build_window_effects(effect_name) {
            let _ = _window.set_effects(effects);
        }
    }

    // Apply decorum overlay titlebar (macOS only — Windows/Linux use DOM-based titlebar)
    #[cfg(target_os = "macos")]
    if config.use_overlay_titlebar {
        _window
            .create_overlay_titlebar()
            .map_err(|e| e.to_string())?;
    }

    // macOS-specific: traffic lights and transparency
    #[cfg(target_os = "macos")]
    {
        if let Some((x, y)) = config.traffic_lights_inset {
            _window
                .set_traffic_lights_inset(x, y)
                .map_err(|e| e.to_string())?;
        }
        if config.transparent {
            _window.make_transparent().map_err(|e| e.to_string())?;
        }
    }

    info!("Window '{}' created successfully", label);
    Ok(())
}

/// Show a window by label.
pub fn show_window(app: &AppHandle, label: &str) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    if label == "main" {
        let _ = app.emit("main-window-visibility", true);
    }
    Ok(())
}

/// Hide a window by label.
pub fn hide_window(app: &AppHandle, label: &str) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;
    window.hide().map_err(|e| e.to_string())?;
    if label == "main" {
        let _ = app.emit("main-window-visibility", false);
    }
    Ok(())
}

/// Close a window by label.
/// If the window's preset has `closeable_to_tray`, it is hidden instead of destroyed.
pub fn close_window(app: &AppHandle, label: &str) -> Result<(), String> {
    // Check if this window should hide-to-tray instead of closing
    if let Some(preset) = WindowConfig::from_label(label) {
        if preset.closeable_to_tray {
            info!("Window '{}' is closeable-to-tray, hiding instead", label);
            return hide_window(app, label);
        }
    }

    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;
    window.destroy().map_err(|e| e.to_string())
}

/// Toggle visibility of a window by label.
pub fn toggle_window(app: &AppHandle, label: &str) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;

    let is_visible = window.is_visible().map_err(|e| e.to_string())?;
    if is_visible {
        window.hide().map_err(|e| e.to_string())?;
        if label == "main" {
            let _ = app.emit("main-window-visibility", false);
        }
        Ok(())
    } else {
        window.show().map_err(|e| e.to_string())?;
        // Only unminimize if actually minimized — calling unminimize on a
        // hidden-but-not-minimized window can reset its size on Windows.
        if window.is_minimized().unwrap_or(false) {
            window.unminimize().map_err(|e| e.to_string())?;
        }
        window.set_focus().map_err(|e| e.to_string())?;
        if label == "main" {
            let _ = app.emit("main-window-visibility", true);
        }
        Ok(())
    }
}

/// Focus a window by label.
pub fn focus_window(app: &AppHandle, label: &str) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;
    window.show().map_err(|e| e.to_string())?;
    if window.is_minimized().unwrap_or(false) {
        window.unminimize().map_err(|e| e.to_string())?;
    }
    window.set_focus().map_err(|e| e.to_string())?;
    if label == "main" {
        let _ = app.emit("main-window-visibility", true);
    }
    Ok(())
}

/// Check if a window exists.
pub fn window_exists(app: &AppHandle, label: &str) -> bool {
    app.get_webview_window(label).is_some()
}

/// Check if a window is visible.
pub fn is_window_visible(app: &AppHandle, label: &str) -> Result<bool, String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;
    window.is_visible().map_err(|e| e.to_string())
}

/// List all open window labels.
pub fn list_windows(app: &AppHandle) -> Vec<String> {
    app.webview_windows().keys().cloned().collect()
}

/// Show a window at a specific position (physical pixels).
pub fn show_window_at_position(app: &AppHandle, label: &str, x: f64, y: f64) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;
    window
        .set_position(PhysicalPosition::new(x as i32, y as i32))
        .map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

/// Set whether a window ignores cursor events (click-through).
pub fn set_ignore_cursor_events(app: &AppHandle, label: &str, ignore: bool) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;
    window
        .set_ignore_cursor_events(ignore)
        .map_err(|e| e.to_string())
}

/// Resize a window to a logical size.
pub fn resize_window(app: &AppHandle, label: &str, width: f64, height: f64) -> Result<(), String> {
    use tauri::LogicalSize;
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;
    window
        .set_size(LogicalSize::new(width, height))
        .map_err(|e| e.to_string())
}

/// Set window position to specific physical coordinates.
pub fn set_window_position(app: &AppHandle, label: &str, x: i32, y: i32) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())
}

/// Build platform-specific window effects config from a named effect.
fn build_window_effects(effect: &str) -> Option<tauri::utils::config::WindowEffectsConfig> {
    build_window_effects_with_color(effect, 30, 30, 30, 200)
}

/// Build platform-specific window effects config with a custom tint color.
fn build_window_effects_with_color(
    effect: &str,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) -> Option<tauri::utils::config::WindowEffectsConfig> {
    match effect {
        "acrylic" => {
            let mut builder = EffectsBuilder::new();
            #[cfg(target_os = "windows")]
            {
                builder = builder.effect(Effect::Acrylic).color(Color(r, g, b, a));
            }
            #[cfg(target_os = "macos")]
            {
                builder = builder
                    .effect(Effect::HudWindow)
                    .state(EffectState::Active)
                    .radius(12.0);
            }
            Some(builder.build())
        }
        _ => None,
    }
}

/// Update the tray popup's window effect tint color.
pub fn set_window_effect_color(
    app: &AppHandle,
    label: &str,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;

    // Look up the preset to find the effect name
    if let Some(preset) = WindowConfig::from_label(label) {
        if let Some(ref effect_name) = preset.window_effect {
            if let Some(effects) = build_window_effects_with_color(effect_name, r, g, b, a) {
                window.set_effects(effects).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}
