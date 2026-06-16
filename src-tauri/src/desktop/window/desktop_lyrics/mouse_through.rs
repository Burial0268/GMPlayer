//! Desktop lyrics transparent-through mouse tracking.
//!
//! Uses a global mouse listener (via rdev) running in a background thread to
//! detect when the cursor is over interactive DOM elements. When the window
//! is in locked mode with `setIgnoreCursorEvents(true)`, mouse events are not
//! delivered to the webview. This module bridges that gap by tracking the
//! global cursor position and toggling click-through dynamically based on
//! whether the cursor is inside user-defined hit regions.
//!
//! The approach is adapted from:
//! https://github.com/codecnmc/tauri2-transparent-through

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// State shared between the mouse worker thread and Tauri commands.
pub struct MouseThroughState {
    /// Whether the global listener is currently running.
    pub listening: AtomicBool,
    /// Handle to the background thread so we can signal it to stop.
    pub stop_sender: Mutex<Option<mpsc::Sender<()>>>,
}

impl Default for MouseThroughState {
    fn default() -> Self {
        Self {
            listening: AtomicBool::new(false),
            stop_sender: Mutex::new(None),
        }
    }
}

/// A hit region defined by the frontend (logical coordinates relative to the
/// webview client area). The backend uses these to decide whether the cursor
/// is over an interactive element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitRegion {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Current window geometry used for coordinate translation.
#[derive(Debug, Clone, Copy)]
struct WindowGeometry {
    inner_x: i32,
    inner_y: i32,
    client_width: u32,
    client_height: u32,
}

/// Start the global mouse listener for a given window label.
///
/// The listener runs in a dedicated thread, receiving global mouse coordinates
/// from `rdev::listen`. Every ~60ms it checks whether the cursor is inside any
/// of the registered hit regions and emits `mouse-through-state` to the
/// frontend. The frontend (or this backend) then calls
/// `setIgnoreCursorEvents(!is_inside)` so the window only blocks the cursor
/// when hovering interactive elements.
///
/// # Platform notes
/// - **Windows / macOS / Linux**: `rdev::listen` captures global mouse moves.
/// - On Windows, `rdev` may require running with UI access for some apps.
pub fn start_mouse_through<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    regions: Vec<HitRegion>,
) -> Result<(), String> {
    let _window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("Window '{}' not found", label))?;

    // If already listening for this window, stop first to avoid duplicates.
    stop_mouse_through(app, label)?;

    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    // Store initial regions in the global registry so the worker thread
    // can read them on its first iteration.
    if let Some(registry) = app.try_state::<HitRegionRegistry>() {
        let mut guard = registry.0.lock().unwrap();
        guard.clear();
        guard.extend(regions);
    }

    let app_handle = app.clone();
    let label_owned = label.to_owned();

    // Spawn the rdev listener thread.
    let rdev_thread = thread::spawn(move || {
        let (coord_tx, coord_rx) = mpsc::channel::<(f64, f64)>();

        // Inner thread: run rdev listener.
        let rdev_stop = stop_rx;
        thread::spawn(move || {
            let tx = coord_tx;
            let callback = move |event: rdev::Event| {
                if let rdev::EventType::MouseMove { x, y } = event.event_type {
                    let _ = tx.send((x, y));
                }
            };

            // rdev::listen blocks; we can't easily interrupt it, so we rely on
            // the outer loop checking the stop channel and the frontend calling
            // stop. rdev does not expose a graceful shutdown, so we accept that
            // the thread will linger until the process exits.
            let _ = rdev::listen(callback);
        });

        let mut last_emit = Instant::now();
        let mut last_state: Option<bool> = None;

        loop {
            // Check stop signal (non-blocking).
            if rdev_stop.try_recv().is_ok() {
                break;
            }

            // Drain all pending coordinates and keep the latest.
            let mut latest: Option<(f64, f64)> = None;
            while let Ok(pos) = coord_rx.try_recv() {
                latest = Some(pos);
            }

            // Throttle to ~60 FPS (16ms).
            if last_emit.elapsed() >= Duration::from_millis(16) {
                if let Some((gx, gy)) = latest {
                    let inside = is_inside_regions(&app_handle, &label_owned, gx, gy);

                    // Only emit when state changes to reduce IPC traffic.
                    if last_state != Some(inside) {
                        last_state = Some(inside);
                        let _ = app_handle.emit_to(&label_owned, "mouse-through-state", inside);
                    }
                }
                last_emit = Instant::now();
            }

            thread::sleep(Duration::from_millis(1));
        }
    });

    // Store the stop sender so `stop_mouse_through` can signal termination.
    if let Some(state) = app.try_state::<MouseThroughState>() {
        let mut guard = state.stop_sender.lock().unwrap();
        *guard = Some(stop_tx);
        state.listening.store(true, Ordering::SeqCst);
    }

    // Detach the thread; it will clean itself up when stopped.
    let _ = rdev_thread;

    Ok(())
}

/// Stop the global mouse listener.
pub fn stop_mouse_through<R: Runtime>(app: &AppHandle<R>, _label: &str) -> Result<(), String> {
    if let Some(state) = app.try_state::<MouseThroughState>() {
        let mut guard = state.stop_sender.lock().unwrap();
        if let Some(tx) = guard.take() {
            let _ = tx.send(());
        }
        state.listening.store(false, Ordering::SeqCst);
    }
    Ok(())
}

/// Update the hit regions without restarting the listener.
pub fn update_hit_regions<R: Runtime>(
    app: &AppHandle<R>,
    _label: &str,
    regions: Vec<HitRegion>,
) -> Result<(), String> {
    if let Some(state) = app.try_state::<MouseThroughState>() {
        if !state.listening.load(Ordering::SeqCst) {
            return Err("Mouse through listener is not running".into());
        }
    }

    // The regions are stored in an Arc<Mutex<_>> shared with the worker thread.
    // We need to locate that Arc. Since we don't have a global registry here,
    // we emit an event to the frontend and let it restart with new regions,
    // or we can store regions in a separate managed state.
    // For simplicity, we store regions in a separate managed state map.
    if let Some(registry) = app.try_state::<HitRegionRegistry>() {
        let mut guard = registry.0.lock().unwrap();
        guard.clear();
        guard.extend(regions);
    }

    Ok(())
}

/// Registry for hit regions, keyed by nothing (single-window assumption for
/// desktop lyrics). If multiple transparent windows need this, extend to a
/// HashMap<String, Vec<HitRegion>>.
pub struct HitRegionRegistry(pub Mutex<Vec<HitRegion>>);

impl Default for HitRegionRegistry {
    fn default() -> Self {
        Self(Mutex::new(Vec::new()))
    }
}

/// Check whether the global coordinate (gx, gy) falls inside any hit region.
///
/// The global coordinate is translated into the webview's client coordinate
/// space using the window's inner position.
fn is_inside_regions<R: Runtime>(app: &AppHandle<R>, label: &str, gx: f64, gy: f64) -> bool {
    let Some(window) = app.get_webview_window(label) else {
        return false;
    };

    let Ok(inner_pos) = window.inner_position() else {
        return false;
    };
    let Ok(size) = window.inner_size() else {
        return false;
    };

    let geo = WindowGeometry {
        inner_x: inner_pos.x,
        inner_y: inner_pos.y,
        client_width: size.width,
        client_height: size.height,
    };

    // Global → client (logical) coordinates.
    // rdev gives physical screen coordinates; inner_position is also physical.
    let cx = gx - geo.inner_x as f64;
    let cy = gy - geo.inner_y as f64;

    // If outside the window client area entirely, treat as outside.
    if cx < 0.0 || cy < 0.0 || cx > geo.client_width as f64 || cy > geo.client_height as f64 {
        return false;
    }

    // Read regions from the registry.
    let registry = match app.try_state::<HitRegionRegistry>() {
        Some(r) => r,
        None => return false,
    };
    let regions = registry.0.lock().unwrap();

    regions
        .iter()
        .any(|r| cx >= r.x && cx <= r.x + r.width && cy >= r.y && cy <= r.y + r.height)
}
