//! Idle-memory policy for managed WebView2 windows.
//!
//! Every window in the app shares one WebView2 environment, so a window that is
//! merely hidden still owns a live renderer process. Hiding a Tauri window only
//! hides the host HWND — the WebView2 controller keeps reporting itself as
//! visible, so the engine keeps compositing and the page keeps its `visible`
//! visibility state. Three engine-side knobs fix that, all driven from the
//! window show/hide paths in [`super::manager`]:
//!
//! * `MemoryUsageTargetLevel` — `Low` asks the engine to drop cached data and
//!   trim its working set, restored to `Normal` before the window returns.
//!   Applied to every managed window; it changes no scheduling, only footprint.
//! * `IsVisible` — tells the engine the control is off screen, which stops its
//!   compositor and moves the page to the throttled `hidden` state. Applied
//!   only to [`IDLE_WHEN_HIDDEN`], because that throttling stops rAF and timers.
//! * `TrySuspend` — freezes the renderer process outright so the OS can reclaim
//!   its memory. Applied only to [`SUSPEND_WHEN_HIDDEN`], and only after
//!   `IsVisible` is false, which WebView2 requires (`ERROR_INVALID_STATE`
//!   otherwise).
//!
//! Everything here is best effort: an older Evergreen runtime does not expose
//! `ICoreWebView2_19`, `TrySuspend` itself is documented as best effort, and a
//! failed COM call must never take a window show/hide down with it.

use tauri::WebviewWindow;

/// Managed windows whose page has nothing to do while hidden, so the engine may
/// be told they are off screen.
///
/// `main` is deliberately absent. It is the playback master: while the app sits
/// in the tray it still drives the AutoMix monitor and broadcasts time anchors
/// and lyric payloads to the lyric windows, and all of that hangs off rAF and
/// timers that the hidden visibility state would throttle. The mini player and
/// lyric windows never reach the hidden path — they are destroyed when closed.
#[cfg(target_os = "windows")]
const IDLE_WHEN_HIDDEN: &[&str] = &["tray-popup", "desktop-lyrics-controls"];

/// Windows that are additionally suspended while hidden. Always a subset of
/// [`IDLE_WHEN_HIDDEN`], since `TrySuspend` requires an invisible controller.
///
/// Only the tray popup qualifies: it is pre-created, spends nearly the whole
/// session hidden, is dismissed on focus loss, and re-syncs from the
/// `tray-popup-opened` event every time it opens. `desktop-lyrics-controls` is
/// excluded on purpose — it toggles with pointer hover, where a resume on every
/// show would be felt.
#[cfg(target_os = "windows")]
const SUSPEND_WHEN_HIDDEN: &[&str] = &["tray-popup"];

/// A window became visible: undo whatever [`on_hidden`] applied.
///
/// Call this *before* showing the window. `with_webview` and the show request
/// travel the same event loop queue, so ordering the calls this way guarantees
/// the engine is awake by the time the window is on screen.
pub fn on_shown(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    imp::wake(window);
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

/// A window was hidden but kept alive (close-to-tray, tray popup dismissal).
pub fn on_hidden(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    imp::sleep(window);
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

/// A window that was created hidden finished loading its page.
///
/// This is where pre-created windows go to sleep. Suspending earlier would
/// fight the initial navigation — WebView2 auto-resumes on navigate — and would
/// leave a half-loaded page to finish rendering on first open. A window that
/// was already shown in the meantime is left alone.
pub fn on_loaded_while_hidden(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    imp::sleep_if_untouched(window);
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

/// Drop a destroyed window from the tracked state.
pub fn forget(label: &str) {
    #[cfg(target_os = "windows")]
    imp::forget(label);
    #[cfg(not(target_os = "windows"))]
    let _ = label;
}

#[cfg(target_os = "windows")]
mod imp {
    use super::{WebviewWindow, IDLE_WHEN_HIDDEN, SUSPEND_WHEN_HIDDEN};
    use std::sync::Mutex;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2Controller, ICoreWebView2_19, ICoreWebView2_3,
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    };
    use webview2_com::TrySuspendCompletedHandler;
    use windows::core::{Interface, BOOL};

    /// Labels believed to be on screen.
    ///
    /// Only [`sleep_if_untouched`] reads it, to resolve the one race that
    /// matters: a tray click that lands while the pre-created popup is still
    /// loading would otherwise be followed by a page-load callback that puts a
    /// visible window to sleep. Bounded by the app's window labels, and
    /// destroyed windows are dropped through [`forget`].
    static AWAKE: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn mark_awake(label: &str) {
        let Ok(mut awake) = AWAKE.lock() else { return };
        if !awake.iter().any(|known| known == label) {
            awake.push(label.to_owned());
        }
    }

    fn mark_asleep(label: &str) {
        let Ok(mut awake) = AWAKE.lock() else { return };
        awake.retain(|known| known != label);
    }

    fn is_awake(label: &str) -> bool {
        AWAKE
            .lock()
            .map(|awake| awake.iter().any(|known| known == label))
            .unwrap_or(false)
    }

    pub fn forget(label: &str) {
        mark_asleep(label);
    }

    /// Run `f` against the window's WebView2 on the UI thread.
    fn dispatch<F>(window: &WebviewWindow, f: F)
    where
        F: FnOnce(&ICoreWebView2Controller, &ICoreWebView2, &str) + Send + 'static,
    {
        let label = window.label().to_owned();
        let result = window.with_webview(move |webview| {
            let controller = webview.controller();
            let core = match unsafe { controller.CoreWebView2() } {
                Ok(core) => core,
                Err(error) => {
                    log::warn!("Failed to reach '{label}' CoreWebView2: {error}");
                    return;
                }
            };
            f(&controller, &core, &label);
        });
        if let Err(error) = result {
            log::warn!(
                "Failed to dispatch webview memory policy for '{}': {}",
                window.label(),
                error
            );
        }
    }

    pub fn wake(window: &WebviewWindow) {
        mark_awake(window.label());
        let restore_visible = IDLE_WHEN_HIDDEN.contains(&window.label());
        dispatch(window, move |controller, core, label| unsafe {
            // Resume before the control goes back on screen. WebView2 also
            // auto-resumes on visible, but doing it first keeps the order
            // deterministic and matches Microsoft's own sample.
            resume(core, label);
            if restore_visible {
                if let Err(error) = controller.SetIsVisible(true) {
                    log::warn!("Failed to show '{label}' webview control: {error}");
                }
            }
            set_memory_level(core, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL, label);
        });
    }

    pub fn sleep(window: &WebviewWindow) {
        mark_asleep(window.label());
        let hide_control = IDLE_WHEN_HIDDEN.contains(&window.label());
        let suspend = SUSPEND_WHEN_HIDDEN.contains(&window.label());
        dispatch(window, move |controller, core, label| unsafe {
            // Order is load-bearing: `TrySuspend` fails unless the controller is
            // already invisible, and it freezes script, so the memory target has
            // to be written before it.
            if hide_control {
                if let Err(error) = controller.SetIsVisible(false) {
                    log::warn!("Failed to hide '{label}' webview control: {error}");
                }
            }
            set_memory_level(core, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, label);
            if suspend {
                try_suspend(core, label);
            }
        });
    }

    pub fn sleep_if_untouched(window: &WebviewWindow) {
        if is_awake(window.label()) {
            return;
        }
        sleep(window);
    }

    /// `ICoreWebView2_19`, runtime 114+. Older Evergreen runtimes simply do not
    /// get this optimization.
    unsafe fn set_memory_level(
        core: &ICoreWebView2,
        level: COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL,
        label: &str,
    ) {
        let Ok(memory) = core.cast::<ICoreWebView2_19>() else {
            return;
        };
        if let Err(error) = unsafe { memory.SetMemoryUsageTargetLevel(level) } {
            log::warn!("Failed to set '{label}' memory usage target: {error}");
        }
    }

    unsafe fn resume(core: &ICoreWebView2, label: &str) {
        let Ok(lifecycle) = core.cast::<ICoreWebView2_3>() else {
            return;
        };
        let mut suspended = BOOL::default();
        if unsafe { lifecycle.IsSuspended(&mut suspended) }.is_err() || !suspended.as_bool() {
            return;
        }
        if let Err(error) = unsafe { lifecycle.Resume() } {
            log::warn!("Failed to resume '{label}' webview: {error}");
        }
    }

    unsafe fn try_suspend(core: &ICoreWebView2, label: &str) {
        let Ok(lifecycle) = core.cast::<ICoreWebView2_3>() else {
            return;
        };
        // Best effort by contract: a page holding a lock, a running script or an
        // active media/download keeps the renderer alive and reports
        // `is_successful = false` with a success HRESULT.
        let owned = label.to_owned();
        let handler = TrySuspendCompletedHandler::create(Box::new(move |result, is_successful| {
            match result {
                Ok(()) if is_successful => log::debug!("Suspended '{owned}' webview"),
                Ok(()) => log::debug!("'{owned}' webview declined to suspend"),
                Err(error) => log::warn!("Failed to suspend '{owned}' webview: {error}"),
            }
            Ok(())
        }));
        if let Err(error) = unsafe { lifecycle.TrySuspend(&handler) } {
            log::warn!("Failed to request suspend for '{label}' webview: {error}");
        }
    }
}
