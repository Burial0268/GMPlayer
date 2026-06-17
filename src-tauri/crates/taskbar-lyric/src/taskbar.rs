use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use taskbar_lyric::TaskbarService;
use tauri::{Emitter, Manager, Runtime};
use tracing::warn;
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{SetWindowPos, HWND_TOP, SWP_NOZORDER},
};

use crate::{mouse_forward, webview_finder};

#[allow(dead_code)]
pub struct TaskbarLyricWatchers {
    pub uia: Option<taskbar_lyric::UiaWatcher>,
    pub tray: Option<taskbar_lyric::TrayWatcher>,
    pub reg: Option<taskbar_lyric::RegistryWatcher>,
}

#[derive(Default)]
pub struct TaskbarLyricState {
    pub service: Mutex<Option<taskbar_lyric::TaskbarService>>,
    pub watchers: Mutex<Option<TaskbarLyricWatchers>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskbarLayoutExtraPayload {
    pub is_centered: bool,
    pub system_type: String,
}

fn schedule_mouse_bounds_sync() {
    tauri::async_runtime::spawn(async move {
        for delay in [50, 150, 300] {
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            mouse_forward::sync_cursor_position();
        }
    });
}

fn find_webview_hwnd_ptr(top_hwnd_ptr: usize) -> Option<usize> {
    webview_finder::find_webview_hwnd(HWND(top_hwnd_ptr as _)).map(|hwnd| hwnd.0 as usize)
}

fn schedule_webview_hwnd_refresh(top_hwnd_ptr: usize) {
    tauri::async_runtime::spawn(async move {
        for delay in [50, 100, 250, 500, 1000, 2000, 4000] {
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            if let Some(webview_hwnd_ptr) = find_webview_hwnd_ptr(top_hwnd_ptr) {
                mouse_forward::init_mouse_forwarding_state(
                    HWND(top_hwnd_ptr as _),
                    HWND(webview_hwnd_ptr as _),
                );
                mouse_forward::start_mouse_hook_thread();
                mouse_forward::sync_cursor_position();
            }
        }
    });
}

#[tauri::command]
pub fn close_taskbar_lyric<R: Runtime>(app: tauri::AppHandle<R>) {
    if let Some(win) = app.get_webview_window("taskbar-lyric") {
        mouse_forward::stop_mouse_hook();
        mouse_forward::clear_pointer_event_emitter();
        if let Some(state) = app.try_state::<TaskbarLyricState>() {
            let _ = state.watchers.lock().unwrap().take();
            let _ = state.service.lock().unwrap().take();
        }
        let _ = win.destroy();
    }
}

#[tauri::command]
pub fn open_taskbar_lyric_devtools<R: Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    #[cfg(not(debug_assertions))]
    {
        let _ = app;
        return Err("taskbar lyric devtools are only available in dev builds".into());
    }

    #[cfg(debug_assertions)]
    {
        if let Some(win) = app.get_webview_window("taskbar-lyric") {
            win.open_devtools();
        }
        Ok(())
    }
}

#[tauri::command]
pub fn open_taskbar_lyric<R: Runtime>(app: tauri::AppHandle<R>) {
    if app.get_webview_window("taskbar-lyric").is_some() {
        return;
    }

    let app_clone = app.clone();
    let service = TaskbarService::new(move |layout| {
        if let Some(win) = app_clone.get_webview_window("taskbar-lyric") {
            let left = layout.space.left;
            let current_rect = if left.width > 0 {
                left
            } else {
                layout.space.right
            };

            let _ = app_clone.emit(
                "taskbar-layout-extra",
                TaskbarLayoutExtraPayload {
                    is_centered: layout.extra.is_centered,
                    system_type: format!("{:?}", layout.extra.system_type),
                },
            );

            if let Ok(hwnd) = win.hwnd() {
                let top_hwnd_ptr = hwnd.0 as usize;
                unsafe {
                    let _ = SetWindowPos(
                        HWND(hwnd.0),
                        Some(HWND_TOP),
                        current_rect.x,
                        current_rect.y,
                        current_rect.width,
                        current_rect.height,
                        SWP_NOZORDER,
                    );
                }
                if let Some(webview_hwnd_ptr) = find_webview_hwnd_ptr(top_hwnd_ptr) {
                    mouse_forward::init_mouse_forwarding_state(
                        HWND(top_hwnd_ptr as _),
                        HWND(webview_hwnd_ptr as _),
                    );
                    mouse_forward::start_mouse_hook_thread();
                }
                schedule_mouse_bounds_sync();
            }
        }
    });

    if let Some(state) = app.try_state::<TaskbarLyricState>() {
        *state.service.lock().unwrap() = Some(service);
    }

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        #[cfg(debug_assertions)]
        let url = tauri::WebviewUrl::External(
            app_clone
                .config()
                .build
                .dev_url
                .clone()
                .unwrap()
                .join("slave.html#/taskbar-lyric")
                .unwrap(),
        );
        #[cfg(not(debug_assertions))]
        let url = tauri::WebviewUrl::App("slave.html#/taskbar-lyric".into());

        let win_builder = tauri::WebviewWindowBuilder::new(&app_clone, "taskbar-lyric", url)
            .decorations(true)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(false)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .visible(true);

        if let Ok(win) = win_builder.build() {
            let top_hwnd_ptr = match win.hwnd() {
                Ok(hwnd) => hwnd.0 as usize,
                Err(_) => {
                    warn!("failed to get hwnd for taskbar-lyric window");
                    return;
                }
            };
            let hwnd_ptr = top_hwnd_ptr;

            let pointer_app = app_clone.clone();
            mouse_forward::set_pointer_event_emitter(move |payload| {
                let _ = pointer_app.emit_to("taskbar-lyric", "taskbar-lyric:pointer", payload);
            });

            if let Some(state) = app_clone.try_state::<TaskbarLyricState>() {
                if let Some(srv) = state.service.lock().unwrap().as_ref() {
                    srv.embed_window_by_ptr(hwnd_ptr);
                    srv.update(300);
                }
            }

            let mut webview_hwnd_ptr = find_webview_hwnd_ptr(top_hwnd_ptr);
            for delay in [50, 100, 200, 400, 800] {
                if webview_hwnd_ptr.is_some() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                webview_hwnd_ptr = find_webview_hwnd_ptr(top_hwnd_ptr);
            }

            if let Some(webview_hwnd_ptr) = webview_hwnd_ptr {
                mouse_forward::init_mouse_forwarding_state(
                    HWND(top_hwnd_ptr as _),
                    HWND(webview_hwnd_ptr as _),
                );
                mouse_forward::start_mouse_hook_thread();
                mouse_forward::sync_cursor_position();
                schedule_webview_hwnd_refresh(top_hwnd_ptr);
            } else {
                warn!("failed to find WebView hwnd");
                schedule_webview_hwnd_refresh(top_hwnd_ptr);
            }

            if let Some(state) = app_clone.try_state::<TaskbarLyricState>() {
                let mut watchers = state.watchers.lock().unwrap();

                let uia_counter = Arc::new(AtomicUsize::new(0));
                let win_clone = app_clone.clone();
                let uia_cb = Box::new(move || {
                    let current = uia_counter.fetch_add(1, Ordering::SeqCst) + 1;
                    let counter_clone = uia_counter.clone();
                    let win_clone_inner = win_clone.clone();

                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                        if counter_clone.load(Ordering::SeqCst) == current {
                            if let Some(s) = win_clone_inner.try_state::<TaskbarLyricState>() {
                                if let Some(srv) = s.service.lock().unwrap().as_ref() {
                                    srv.update(300);
                                }
                            }
                        }
                    });
                });

                let win_clone2 = app_clone.clone();
                let tray_cb = Box::new(move || {
                    if let Some(s) = win_clone2.try_state::<TaskbarLyricState>() {
                        if let Some(srv) = s.service.lock().unwrap().as_ref() {
                            srv.update(300);
                        }
                    }
                });

                let reg_counter = Arc::new(AtomicUsize::new(0));
                let win_clone3 = app_clone.clone();
                let reg_cb = Box::new(move || {
                    let _ = win_clone3.emit("taskbar-lyric:fade-out", ());

                    let current = reg_counter.fetch_add(1, Ordering::SeqCst) + 1;
                    let counter_clone = reg_counter.clone();
                    let win_clone_inner = win_clone3.clone();

                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        if counter_clone.load(Ordering::SeqCst) == current {
                            if let Some(s) = win_clone_inner.try_state::<TaskbarLyricState>() {
                                if let Some(srv) = s.service.lock().unwrap().as_ref() {
                                    srv.update(300);
                                    let _ = win_clone_inner.emit("taskbar-lyric:fade-in", ());
                                }
                            }
                        }
                    });
                });

                *watchers = Some(TaskbarLyricWatchers {
                    uia: taskbar_lyric::UiaWatcher::new(uia_cb).ok(),
                    tray: taskbar_lyric::TrayWatcher::new(tray_cb).ok(),
                    reg: taskbar_lyric::RegistryWatcher::new(reg_cb).ok(),
                });

                let _ = win.show();
            }
        } else {
            warn!("failed to build taskbar-lyric window");
        }
    });
}
