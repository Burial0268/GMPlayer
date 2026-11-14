use super::*;

#[test]
fn a_retired_callback_context_cannot_signal_a_new_watcher() {
    let state = Arc::new(WatchState {
        pending: Arc::new(PendingChanges::default()),
        refresh_devices: AtomicBool::new(false),
        stopped: AtomicBool::new(false),
        worker: OnceLock::new(),
    });
    let context = CallbackContext::new(Arc::clone(&state));
    let old_id = context.0;
    let addresses = [
        AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMaster,
        },
        AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMaster,
        },
    ];
    unsafe {
        property_changed(
            0,
            addresses.len() as u32,
            addresses.as_ptr(),
            old_id as *mut c_void,
        );
    }
    assert_eq!(state.pending.take().0, DEFAULT_CHANGED | DEVICES_CHANGED);
    assert!(state.refresh_devices.swap(false, Ordering::AcqRel));

    drop(context);
    let new_context = CallbackContext::new(Arc::clone(&state));
    assert_ne!(old_id, new_context.0);
    unsafe {
        property_changed(
            0,
            addresses.len() as u32,
            addresses.as_ptr(),
            old_id as *mut c_void,
        );
    }
    assert_eq!(state.pending.take().0, 0);
    assert!(!state.refresh_devices.load(Ordering::Acquire));
}
