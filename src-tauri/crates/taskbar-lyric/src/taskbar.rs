use std::sync::Mutex;

use serde::Serialize;
use taskbar_lyric::{Alignment, Embed, Taskbar, TaskbarEvent, TaskbarKind, TaskbarLayout};
use tauri::{Emitter, Manager, Runtime};
use tracing::warn;
use windows::Win32::Foundation::HWND;

use crate::{mouse_forward, webview_finder};

#[derive(Default)]
pub struct TaskbarLyricState {
    pub service: Mutex<Option<Taskbar>>,
    pub additional_browser_args: Option<String>,
}

impl TaskbarLyricState {
    pub fn new(additional_browser_args: Option<String>) -> Self {
        Self {
            service: Mutex::default(),
            additional_browser_args,
        }
    }
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

fn apply_taskbar_layout<R: Runtime>(
    app: &tauri::AppHandle<R>,
    embed: &Embed,
    layout: TaskbarLayout,
) {
    let Some(win) = app.get_webview_window("taskbar-lyric") else {
        return;
    };

    let _ = win.emit(
        "taskbar-layout-extra",
        TaskbarLayoutExtraPayload {
            is_centered: layout.alignment == Alignment::Center,
            system_type: match layout.kind {
                TaskbarKind::Modern => "Win11",
                TaskbarKind::Legacy => "Win10",
            }
            .into(),
        },
    );

    let Some(rect) = layout.before.or(layout.after) else {
        let _ = win.hide();
        return;
    };
    if let Err(err) = embed.place(rect) {
        warn!("failed to place taskbar-lyric window: {err}");
        return;
    }

    let _ = win.show();
    let top_hwnd_ptr = embed.hwnd().0 as usize;
    if let Some(webview_hwnd_ptr) = find_webview_hwnd_ptr(top_hwnd_ptr) {
        mouse_forward::init_mouse_forwarding_state(
            HWND(top_hwnd_ptr as _),
            HWND(webview_hwnd_ptr as _),
        );
        mouse_forward::start_mouse_hook_thread();
    }
    schedule_mouse_bounds_sync();
}

#[tauri::command]
pub fn close_taskbar_lyric<R: Runtime>(app: tauri::AppHandle<R>) {
    mouse_forward::stop_mouse_hook();
    mouse_forward::clear_pointer_event_emitter();
    if let Some(state) = app.try_state::<TaskbarLyricState>() {
        let _ = state.service.lock().unwrap().take();
    }
    if let Some(win) = app.get_webview_window("taskbar-lyric") {
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

        let build_result = {
            let mut win_builder =
                tauri::WebviewWindowBuilder::new(&app_clone, "taskbar-lyric", url)
                    .decorations(true)
                    .transparent(true)
                    .always_on_top(true)
                    .skip_taskbar(false)
                    .resizable(false)
                    .maximizable(false)
                    .minimizable(false)
                    .visible(false);

            if let Some(state) = app_clone.try_state::<TaskbarLyricState>() {
                if let Some(args) = state.additional_browser_args.as_deref() {
                    let args = args.trim();
                    if !args.is_empty() {
                        win_builder = win_builder.additional_browser_args(args);
                    }
                }
            }

            win_builder.build()
        };

        match build_result {
            Ok(win) => {
                let top_hwnd_ptr = match win.hwnd() {
                    Ok(hwnd) => hwnd.0 as usize,
                    Err(_) => {
                        warn!("failed to get hwnd for taskbar-lyric window");
                        let _ = win.destroy();
                        return;
                    }
                };

                let (service, events) =
                    match tauri::async_runtime::spawn_blocking(|| Taskbar::builder().build()).await
                    {
                        Ok(Ok(started)) => started,
                        Ok(Err(err)) => {
                            warn!("failed to start taskbar-lyric service: {err}");
                            let _ = win.destroy();
                            return;
                        }
                        Err(err) => {
                            warn!("taskbar-lyric service task failed: {err}");
                            let _ = win.destroy();
                            return;
                        }
                    };

                let pointer_app = app_clone.clone();
                mouse_forward::set_pointer_event_emitter(move |payload| {
                    let _ = pointer_app.emit_to("taskbar-lyric", "taskbar-lyric:pointer", payload);
                });

                let service_app = app_clone.clone();
                // Embed installs a window subclass and must run on the window's UI thread.
                if let Err(err) = win.run_on_main_thread(move || {
                    let embed = match service.embed(HWND(top_hwnd_ptr as _)).and_then(|embed| {
                        embed.attach_primary()?;
                        Ok(embed)
                    }) {
                        Ok(embed) => embed,
                        Err(err) => {
                            warn!("failed to embed taskbar-lyric window: {err}");
                            close_taskbar_lyric(service_app);
                            return;
                        }
                    };

                    let Some(state) = service_app.try_state::<TaskbarLyricState>() else {
                        return;
                    };
                    *state.service.lock().unwrap() = Some(service);
                    schedule_webview_hwnd_refresh(top_hwnd_ptr);

                    tauri::async_runtime::spawn_blocking(move || {
                        for event in events {
                            match event {
                                TaskbarEvent::LayoutChanged { layout, .. }
                                    if layout.monitor.is_primary =>
                                {
                                    apply_taskbar_layout(&service_app, &embed, layout);
                                }
                                TaskbarEvent::TaskbarRemoved { monitor } if monitor.is_primary => {
                                    // Explorer destroys its child windows with the taskbar.
                                    close_taskbar_lyric(service_app);
                                    break;
                                }
                                _ => {}
                            }
                        }
                    });
                }) {
                    warn!("failed to schedule taskbar-lyric embedding: {err}");
                    close_taskbar_lyric(app_clone);
                    return;
                }
            }
            Err(err) => {
                warn!("failed to build taskbar-lyric window: {err}");
            }
        }
    });
}
