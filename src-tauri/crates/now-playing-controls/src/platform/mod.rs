#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use self::windows::WindowsImpl as PlatformMediaControls;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use self::linux::LinuxImpl as PlatformMediaControls;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::MacosImpl as PlatformMediaControls;

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod noop;
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub use self::noop::NoOpImpl as PlatformMediaControls;
