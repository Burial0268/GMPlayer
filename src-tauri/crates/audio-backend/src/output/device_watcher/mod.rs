//! Native device notifications are hints; output_runtime owns probing and recovery.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

const DEFAULT_CHANGED: u8 = 1;
const DEVICES_CHANGED: u8 = 2;

#[derive(Default)]
struct PendingChanges(AtomicU8);

impl PendingChanges {
    fn record(&self, changes: u8) {
        // A burst must retain the default-change bit even if a list change arrived first.
        self.0.fetch_or(changes, Ordering::Release);
    }

    fn take(&self) -> DeviceChanges {
        DeviceChanges(self.0.swap(0, Ordering::AcqRel))
    }
}

pub(crate) struct DeviceChanges(u8);

impl DeviceChanges {
    pub(crate) fn requires_refresh(&self, follows_default: bool) -> bool {
        self.0 & DEVICES_CHANGED != 0 || (follows_default && self.0 & DEFAULT_CHANGED != 0)
    }
}

/// Owned by the player, independently of any output stream or WebView.
pub(crate) struct DeviceWatcher {
    pending: Arc<PendingChanges>,
    #[cfg(target_os = "linux")]
    _backend: Option<linux::Backend>,
    #[cfg(target_os = "macos")]
    _backend: Option<macos::Backend>,
}

impl DeviceWatcher {
    pub(crate) fn new() -> Self {
        let pending = Arc::new(PendingChanges::default());
        Self {
            #[cfg(target_os = "linux")]
            _backend: linux::Backend::start(Arc::clone(&pending)),
            #[cfg(target_os = "macos")]
            _backend: macos::Backend::start(Arc::clone(&pending)),
            pending,
        }
    }

    pub(crate) fn take_changes(&self) -> DeviceChanges {
        // Windows already owns a process-wide MMDevAPI registration and COM policy.
        if super::take_default_output_changed() {
            self.pending.record(DEFAULT_CHANGED);
        }
        self.pending.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
