pub mod commands;
pub mod config;
pub mod desktop_lyrics;
pub mod effects;
#[cfg(target_os = "windows")]
mod host_backdrop;
pub mod manager;
#[cfg(target_os = "windows")]
mod minimize_guard;
pub mod payload;
pub mod tray;
pub mod webview_memory;
