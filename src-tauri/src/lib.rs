pub mod algorithms;
pub mod shared;

#[cfg(not(mobile))]
pub mod desktop;

#[cfg(mobile)]
pub mod mobile;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(not(mobile))]
    desktop::run();

    #[cfg(mobile)]
    mobile::run();
}
