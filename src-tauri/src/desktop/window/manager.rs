use crate::desktop::window::{config::WindowConfig, effects, webview_memory};
use log::info;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(target_os = "windows"))]
use tauri::Theme;
use tauri::{
    webview::PageLoadEvent, AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};
#[cfg(target_os = "macos")]
use tauri_plugin_decorum::WebviewWindowExt;

static MAIN_SHELL_EFFECT_INITIALIZED: AtomicBool = AtomicBool::new(false);
static MAIN_SHELL_DARK: AtomicBool = AtomicBool::new(false);

/// Broadcast a managed window's visibility change. The master window uses this
/// to gate its slave broadcasts (time anchors / lyric payloads / reconcile
/// polling) on windows that are actually visible — a hidden-but-alive window
/// otherwise keeps that machinery running for the rest of the session.
fn emit_window_visibility(app: &AppHandle, label: &str, visible: bool) {
    let _ = app.emit(
        "managed-window-visibility",
        serde_json::json!({ "label": label, "visible": visible }),
    );
}

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
            apply_runtime_size_constraints(&existing, config)?;
            webview_memory::on_shown(&existing);
            existing.show().map_err(|e| e.to_string())?;
            if existing.is_minimized().unwrap_or(false) {
                existing.unminimize().map_err(|e| e.to_string())?;
            }
            existing.set_focus().map_err(|e| e.to_string())?;
            // Re-shown without a page load — no slaveReady handshake fires, so
            // announce visibility for the master's broadcast gating.
            emit_window_visibility(app, label, true);
            return Ok(());
        }
    }

    info!("Creating window '{}'", label);

    if label == "main" {
        MAIN_SHELL_EFFECT_INITIALIZED.store(false, Ordering::Release);
        MAIN_SHELL_DARK.store(false, Ordering::Release);
    }

    let has_configured_effect = config.window_effect.is_some();
    let defer_main_shell_effect =
        label == "main" && config.window_effect.as_deref() == Some("system-shell");
    let window_effects = if defer_main_shell_effect {
        None
    } else {
        config.window_effect.as_deref().and_then(|effect_name| {
            let color = if label == "tray-popup" {
                tauri::window::Color(240, 240, 240, 200)
            } else {
                tauri::window::Color(30, 30, 30, 200)
            };
            effects::build_window_effects(effect_name, Some(color))
        })
    };

    // Tauri's Windows effects documentation points to the window-vibrancy
    // workaround: create an undecorated transparent window without its shadow,
    // apply the effect, then restore the shadow so the native frame is rebuilt.
    #[cfg(target_os = "windows")]
    let initial_shadow = if has_configured_effect {
        false
    } else {
        config.shadow
    };
    #[cfg(not(target_os = "windows"))]
    let initial_shadow = config.shadow;

    let url = WebviewUrl::App(config.url.clone().into());
    let mut builder = WebviewWindowBuilder::new(app, label, url)
        .title(&config.title)
        .inner_size(config.width, config.height)
        .resizable(config.resizable)
        .decorations(config.decorations);

    #[cfg(not(target_os = "macos"))]
    {
        builder = builder.transparent(config.transparent);
    }

    let mut builder = builder
        .always_on_top(config.always_on_top)
        .skip_taskbar(config.skip_taskbar)
        .visible(config.visible)
        .shadow(initial_shadow);

    if config.center {
        builder = builder.center();
    }

    #[cfg(target_os = "windows")]
    if let Some(args) = config.effective_additional_args() {
        let args = args.trim();
        if !args.is_empty() {
            builder = builder.additional_browser_args(args);
        }
    }

    // Windows that are pre-created hidden (tray popup, lyric controls) sit
    // there for the whole session, so put them to sleep as soon as their page
    // is loaded. `main` is excluded: it is only hidden for the moment it takes
    // the frontend to pick a theme and reveal it, and nothing on that path
    // would wake it back up.
    if !config.visible && label != "main" {
        builder = builder.on_page_load(|window, payload| {
            if matches!(payload.event(), PageLoadEvent::Finished) {
                webview_memory::on_loaded_while_hidden(&window);
            }
        });
    }

    // Apply min/max size constraints. Each dimension defaults to 0.0 if unset,
    // allowing min_width and min_height to be specified independently.
    if config.min_width.is_some() || config.min_height.is_some() {
        builder = builder.min_inner_size(
            config.min_width.unwrap_or(0.0),
            config.min_height.unwrap_or(0.0),
        );
    }

    if config.max_width.is_some() || config.max_height.is_some() {
        builder = builder.max_inner_size(
            config.max_width.unwrap_or(f64::MAX),
            config.max_height.unwrap_or(f64::MAX),
        );
    }

    // Handle parent window relationship for child windows
    if let Some(ref parent_label) = config.parent_label {
        if let Some(parent_window) = app.get_webview_window(parent_label) {
            builder = builder.parent(&parent_window).map_err(|e| e.to_string())?;
        } else {
            return Err(format!(
                "Parent window '{}' not found for '{}'",
                parent_label, label
            ));
        }
    }

    let _window = builder.build().map_err(|e| e.to_string())?;
    apply_runtime_size_constraints(&_window, config)?;

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

    if let Some(window_effects) = window_effects {
        _window
            .set_effects(window_effects)
            .map_err(|e| e.to_string())?;

        #[cfg(target_os = "windows")]
        if config.shadow {
            _window.set_shadow(true).map_err(|e| e.to_string())?;
        }
    }

    info!("Window '{}' created successfully", label);
    Ok(())
}

/// Apply the app-selected native shell theme, then reveal the hidden main window.
pub fn set_main_window_effect_theme(app: &AppHandle, dark: bool) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Window 'main' not found".to_string())?;
    let initialize = !MAIN_SHELL_EFFECT_INITIALIZED.load(Ordering::Acquire);
    MAIN_SHELL_DARK.store(dark, Ordering::Release);

    // On Windows the Host Mica Alt helper sets DWM dark mode and tint directly. Avoid `set_theme`:
    // Wry applies it through the shared WebView2 profile, which changes `prefers-color-scheme` in
    // every slave window.
    #[cfg(not(target_os = "windows"))]
    window
        .set_theme(Some(if dark { Theme::Dark } else { Theme::Light }))
        .map_err(|e| e.to_string())?;

    // Runtime theme changes do not need to block the command/frontend transition. Queue the
    // native update on Tauri's UI thread and return immediately. Initial setup remains
    // synchronous because the hidden window must not be revealed before its material exists.
    #[cfg(target_os = "windows")]
    if !initialize {
        let update_window = window.clone();
        window
            .run_on_main_thread(move || {
                if let Err(error) = effects::apply_system_shell_effect(&update_window, dark) {
                    log::warn!("Failed to apply asynchronous main window material: {error}");
                }
            })
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    if initialize {
        window.set_shadow(false).map_err(|e| e.to_string())?;
    }

    let effect_result = effects::apply_system_shell_effect(&window, dark);

    #[cfg(target_os = "windows")]
    let frame_result = if initialize {
        window.set_shadow(true).map_err(|e| e.to_string())
    } else {
        Ok(())
    };

    effect_result?;

    #[cfg(target_os = "windows")]
    frame_result?;

    // Restoring the undecorated native shadow rebuilds the HWND frame and can clear an
    // undocumented SWCA policy. Reapply the live material after the frame is final.
    #[cfg(target_os = "windows")]
    if initialize {
        effects::apply_system_shell_effect(&window, dark)?;
    }

    if initialize {
        MAIN_SHELL_EFFECT_INITIALIZED.store(true, Ordering::Release);

        // Reveal on the UI thread and immediately force the first composition.
        // The material (and on the SWCA fallback, the accent policy) was applied
        // while the window was hidden; without a post-show nudge DWM can leave
        // the transparent window uncomposed — invisible except for its taskbar
        // button until a thumbnail hover forces a present.
        #[cfg(target_os = "windows")]
        {
            let revealed = window.clone();
            window
                .run_on_main_thread(move || {
                    if let Err(error) = revealed.show() {
                        log::warn!("Failed to show main window: {error}");
                        return;
                    }
                    let _ = revealed.set_focus();
                    effects::force_first_present(&revealed, dark);
                })
                .map_err(|e| e.to_string())?;
        }

        #[cfg(not(target_os = "windows"))]
        {
            window.show().map_err(|e| e.to_string())?;
            window.set_focus().map_err(|e| e.to_string())?;
        }

        let _ = app.emit("main-window-visibility", true);
    }

    Ok(())
}

/// Safety net for the hidden main window. The window is created invisible and
/// only revealed when the frontend calls `set_main_window_effect_theme`; if the
/// frontend fails before that call (script error, rejected invoke), the app
/// would keep running with no window at all — on macOS this looks like the app
/// "cannot be opened". Apply the default material for the current system theme
/// and reveal instead of staying invisible.
pub fn reveal_main_window_if_stuck(app: &AppHandle) {
    if MAIN_SHELL_EFFECT_INITIALIZED.load(Ordering::Acquire) {
        return;
    }
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        return;
    }
    let dark = matches!(window.theme(), Ok(tauri::Theme::Dark));
    log::warn!("Main window was never revealed by the frontend; forcing fallback reveal");
    if let Err(e) = set_main_window_effect_theme(app, dark) {
        log::warn!("Fallback reveal failed to apply the shell material: {e}");
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn apply_runtime_size_constraints(
    window: &WebviewWindow,
    config: &WindowConfig,
) -> Result<(), String> {
    if config.min_width.is_some() || config.min_height.is_some() {
        window
            .set_min_size(Some(LogicalSize::new(
                config.min_width.unwrap_or(0.0),
                config.min_height.unwrap_or(0.0),
            )))
            .map_err(|e| e.to_string())?;
    }

    if config.max_width.is_some() || config.max_height.is_some() {
        window
            .set_max_size(Some(LogicalSize::new(
                config.max_width.unwrap_or(f64::MAX),
                config.max_height.unwrap_or(f64::MAX),
            )))
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Show a window by label.
pub fn show_window(app: &AppHandle, label: &str) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;
    webview_memory::on_shown(&window);
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    if label == "main" {
        let _ = app.emit("main-window-visibility", true);
    }
    emit_window_visibility(app, label, true);
    Ok(())
}

/// Hide a window by label.
pub fn hide_window(app: &AppHandle, label: &str) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;
    window.hide().map_err(|e| e.to_string())?;
    webview_memory::on_hidden(&window);
    if label == "main" {
        let _ = app.emit("main-window-visibility", false);
    }
    emit_window_visibility(app, label, false);
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

/// A managed window was destroyed.
///
/// Announcing the close as a visibility change lets the master drop the window
/// from its broadcast set immediately, instead of waiting for the reconcile
/// poll to notice it is gone.
pub fn on_window_destroyed(app: &AppHandle, label: &str) {
    webview_memory::forget(label);
    emit_window_visibility(app, label, false);
}

/// Toggle visibility of a window by label.
pub fn toggle_window(app: &AppHandle, label: &str) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;

    let is_visible = window.is_visible().map_err(|e| e.to_string())?;
    if is_visible {
        window.hide().map_err(|e| e.to_string())?;
        webview_memory::on_hidden(&window);
        if label == "main" {
            let _ = app.emit("main-window-visibility", false);
        }
        emit_window_visibility(app, label, false);
        Ok(())
    } else {
        webview_memory::on_shown(&window);
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
        emit_window_visibility(app, label, true);
        Ok(())
    }
}

/// Focus a window by label.
pub fn focus_window(app: &AppHandle, label: &str) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;
    webview_memory::on_shown(&window);
    window.show().map_err(|e| e.to_string())?;
    if window.is_minimized().unwrap_or(false) {
        window.unminimize().map_err(|e| e.to_string())?;
    }
    window.set_focus().map_err(|e| e.to_string())?;
    if label == "main" {
        let _ = app.emit("main-window-visibility", true);
    }
    emit_window_visibility(app, label, true);
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

/// Open DevTools for a managed window in development builds.
pub fn open_window_devtools(app: &AppHandle, label: &str) -> Result<(), String> {
    #[cfg(not(debug_assertions))]
    {
        let _ = app;
        let _ = label;
        Err("window devtools are only available in dev builds".into())
    }

    #[cfg(debug_assertions)]
    {
        let window = app
            .get_webview_window(label)
            .ok_or_else(|| format!("Window '{}' not found", label))?;
        window.open_devtools();
        Ok(())
    }
}

/// Show a window at a specific position (physical pixels).
pub fn show_window_at_position(app: &AppHandle, label: &str, x: f64, y: f64) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;
    window
        .set_position(PhysicalPosition::new(x as i32, y as i32))
        .map_err(|e| e.to_string())?;
    webview_memory::on_shown(&window);
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    emit_window_visibility(app, label, true);
    Ok(())
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

/// Update the tray popup's window effect tint color.
pub fn set_window_effect_color(
    app: &AppHandle,
    label: &str,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) -> Result<(), String> {
    effects::set_effect_color(app, label, r, g, b, a)
}
