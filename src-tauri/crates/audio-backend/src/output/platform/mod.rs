#[cfg(target_os = "android")]
mod android;
#[cfg(not(any(target_os = "android", target_os = "linux")))]
mod desktop;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod other;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "android")]
pub(super) use android::{stable_buffer_size, DEFAULT_QUEUE_BLOCKS};
#[cfg(not(any(target_os = "android", target_os = "linux")))]
pub(super) use desktop::{stable_buffer_size, DEFAULT_QUEUE_BLOCKS};
#[cfg(target_os = "linux")]
pub(super) use linux::{stable_buffer_size, DEFAULT_QUEUE_BLOCKS};

#[cfg(target_os = "macos")]
pub(super) use macos::default_output_id;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(super) use other::default_output_id;
#[cfg(target_os = "windows")]
pub(super) use windows::default_output_id;
