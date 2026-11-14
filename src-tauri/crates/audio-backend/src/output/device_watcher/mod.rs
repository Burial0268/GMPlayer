//! Native device notifications are hints; output_runtime owns probing and recovery.

use std::sync::atomic::{AtomicU8, Ordering};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux::Backend as SelectedBackend;
#[cfg(target_os = "macos")]
use macos::Backend as SelectedBackend;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
use unsupported::Backend as SelectedBackend;
#[cfg(target_os = "windows")]
use windows::Backend as SelectedBackend;

const DEFAULT_CHANGED: u8 = 1;
const DEVICES_CHANGED: u8 = 2;

#[derive(Default)]
struct PendingChanges(AtomicU8);

impl PendingChanges {
    #[cfg_attr(
        not(any(target_os = "linux", target_os = "macos", target_os = "windows")),
        allow(dead_code)
    )]
    fn record(&self, changes: u8) {
        // A burst must retain the default-change bit even if a list change arrived first.
        self.0.fetch_or(changes, Ordering::Release);
    }

    fn take(&self) -> DeviceChanges {
        DeviceChanges(self.0.swap(0, Ordering::AcqRel))
    }
}

/// Statically selected platform contract; registration lifetime stays with the backend.
trait PlatformBackend: Sized {
    fn start() -> Option<Self>;
    fn events(&self) -> &PendingChanges;
}

pub(crate) struct DeviceChanges(u8);

impl DeviceChanges {
    pub(crate) fn requires_refresh(&self, follows_default: bool) -> bool {
        self.0 & DEVICES_CHANGED != 0 || (follows_default && self.0 & DEFAULT_CHANGED != 0)
    }
}

/// Independent of output streams and WebViews. Unsupported hosts only use the probe.
pub(crate) struct DeviceWatcher {
    _backend: Option<SelectedBackend>,
}

impl DeviceWatcher {
    pub(crate) fn new() -> Self {
        Self {
            _backend: SelectedBackend::start(),
        }
    }

    pub(crate) fn take_changes(&self) -> DeviceChanges {
        self._backend
            .as_ref()
            .map_or(DeviceChanges(0), |backend| backend.events().take())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn a_burst_preserves_both_changes_and_consumes_them_once() {
        let pending = Arc::new(PendingChanges::default());
        std::thread::scope(|scope| {
            for change in [DEVICES_CHANGED, DEFAULT_CHANGED] {
                let pending = Arc::clone(&pending);
                scope.spawn(move || {
                    for _ in 0..100 {
                        pending.record(change);
                    }
                });
            }
        });
        assert_eq!(pending.take().0, DEVICES_CHANGED | DEFAULT_CHANGED);
        assert_eq!(pending.take().0, 0);
    }

    #[test]
    fn a_default_change_does_not_redirect_a_selected_device() {
        let changes = DeviceChanges(DEFAULT_CHANGED);
        assert!(changes.requires_refresh(true));
        assert!(!changes.requires_refresh(false));
        assert!(DeviceChanges(DEVICES_CHANGED).requires_refresh(false));
    }
}
