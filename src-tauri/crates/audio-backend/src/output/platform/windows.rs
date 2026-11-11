//! Windows output policy: COM apartment selection and concrete default endpoints.
//!
//! Device notifications live in `output::device_watcher::windows`; keeping that
//! lifecycle separate prevents endpoint probing from registering listeners.

use std::cell::Cell;

use cpal::traits::HostTrait;
use tracing::warn;
use windows::core::PWSTR;
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{
    CoCreateInstance, CoGetApartmentType, CoInitializeEx, CoTaskMemFree, APTTYPE, APTTYPEQUALIFIER,
    APTTYPEQUALIFIER_IMPLICIT_MTA, APTTYPE_MAINSTA, APTTYPE_MTA, APTTYPE_NA, APTTYPE_STA,
    CLSCTX_ALL, COINIT_MULTITHREADED,
};

/// Join this thread to the COM multi-threaded apartment before CPAL can choose
/// one for us, and never leave it again.
///
/// CPAL's WASAPI backend initialises every thread it touches as an STA
/// (`host/com.rs`, picked for ASIO and winit drag-and-drop compatibility, both
/// irrelevant to a thread we spawned ourselves). MMDevAPI's activation path
/// initialises COM as an *MTA*, so a thread CPAL got to first meets it with a
/// mismatched apartment — which is the documented shape of the
/// `RPC_E_CHANGED_MODE` this module's header describes.
///
/// This is necessary and it is not sufficient. The failure came back with
/// `com=MTA` in the log — this thread was already in the apartment MMDevAPI
/// wants, and the STA it collided with was somewhere else entirely (see
/// [`default_output_device`]). Keep both defences: an apartment we chose costs
/// one call per thread, and the alternative is letting a library that documents
/// its choice as an ASIO workaround decide it for us.
///
/// Two details are load-bearing. The apartment is deliberately *not* left: a
/// `CoUninitialize()` here tears it down under CPAL's thread-local guard, which
/// assumes COM stays up for the thread's lifetime and will not re-initialise —
/// the next activation on that thread then fails with `CO_E_NOTINITIALIZED`
/// instead, which is the trade the `CoInitializeEx`/`CoUninitialize` pair that
/// used to live in `default_output_id` was making. And `RPC_E_CHANGED_MODE`
/// *here* is tolerated rather than fatal: a thread that already joined an STA
/// cannot be moved, COM still marshals the calls, and all we lose is the
/// guarantee — so it is worth a line in the log and nothing more.
pub(in crate::output) fn ensure_audio_apartment() {
    thread_local! {
        static JOINED: Cell<bool> = const { Cell::new(false) };
    }

    JOINED.with(|joined| {
        if joined.replace(true) {
            return;
        }
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_ok() {
            return;
        }
        if result == RPC_E_CHANGED_MODE {
            warn!(
                "音频线程已加入其他 COM apartment（{}），WASAPI 设备操作可能失败",
                apartment_label()
            );
        } else {
            warn!("音频线程 COM 初始化失败：{result:?}");
        }
    });
}

/// Current apartment of the calling thread, for the one log line that needs it.
/// `RPC_E_CHANGED_MODE` says only that two parties disagreed, never who won, so
/// a device failure that carries this is self-diagnosing — and it is what proved
/// the apartment fix alone was not the answer.
pub(in crate::output) fn apartment_note() -> String {
    format!("，com={}", apartment_label())
}

fn apartment_label() -> String {
    let mut apartment = APTTYPE::default();
    let mut qualifier = APTTYPEQUALIFIER::default();
    if unsafe { CoGetApartmentType(&mut apartment, &mut qualifier) }.is_err() {
        return "未初始化".to_string();
    }
    let kind = match apartment {
        APTTYPE_MTA => "MTA",
        APTTYPE_STA => "STA",
        APTTYPE_MAINSTA => "主STA",
        APTTYPE_NA => "NA",
        _ => "未知",
    };
    if qualifier == APTTYPEQUALIFIER_IMPLICIT_MTA {
        format!("{kind}(隐式)")
    } else {
        kind.to_string()
    }
}

/// Endpoint id of the system default render device, or `None` when there is no
/// default at all (no output hardware, or the audio service is down).
fn default_output_endpoint_id() -> Option<String> {
    struct IdGuard(PWSTR);

    impl Drop for IdGuard {
        fn drop(&mut self) {
            unsafe {
                CoTaskMemFree(Some(self.0.as_ptr() as *const _));
            }
        }
    }

    ensure_audio_apartment();

    // A fresh enumerator per call, on purpose: this is the one COM object here
    // that gets *used* after creation, and a cached one would be shared across
    // threads whose apartment we can only guarantee, not verify.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()? };
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()? };
    let id = IdGuard(unsafe { device.GetId().ok()? });
    unsafe { id.0.to_string().ok() }
}

pub(in crate::output) fn default_output_id(_device: &cpal::Device) -> Option<String> {
    default_output_endpoint_id().map(|id| format!("wasapi:{id}"))
}

/// The system default output, resolved to the *endpoint* it currently names.
///
/// `host.default_output_device()` hands back CPAL's virtual default device
/// (`DeviceHandle::DefaultOutput`), and the calls this crate makes on it —
/// `default_output_config()`, `supported_output_configs()`, `build_output_stream()`
/// — each activate its audio client through `ActivateAudioInterfaceAsync`. That
/// API is the one that answered `RPC_E_CHANGED_MODE`, and it must not be on this
/// crate's path at all.
///
/// The evidence says the collision is not ours: the failure carried `com=MTA`,
/// read live off the calling thread, which is the apartment MMDevAPI's
/// activation wants. It was also *intermittent* before it was permanent — one
/// probe failed and the reopen a second later succeeded — which is the signature
/// of dispatch landing on one thread rather than another. Windows completes this
/// activation on its own worker threads, and CPAL initialises COM as an **STA**
/// on foreign threads in exactly that machinery: a default-device stream
/// registers its `IMMNotificationClient`, and `OnDeviceStateChanged` /
/// `OnDeviceRemoved` call `current_default_endpoint()`, whose first statement is
/// `com_initialized()` → `CoInitializeEx(COINIT_APARTMENTTHREADED)`, never
/// undone. Those callbacks fire only on a device change — which is exactly when
/// this starts, and it never recovers because the thread keeps the apartment for
/// the life of the process.
///
/// An enumerated endpoint activates through `IMMDevice::Activate` instead:
/// in-proc, on the calling thread, no async worker and no notification client of
/// CPAL's anywhere. And it costs nothing, because CPAL does not rebind a
/// default-device stream either — `stream.rs::default_device_change_error` says
/// so in as many words ("WASAPI never rebinds the IAudioClient") and reports
/// `StreamInvalidated` for the caller to act on. Following the default is
/// `player::output_runtime`'s job, off its own device poll and device_watcher's
/// notifications. `OutputDeviceKey::platform_id` is still this endpoint's id,
/// so moving the default reads as a changed key.
pub(in crate::output) fn default_output_device(host: &cpal::Host) -> Option<cpal::Device> {
    ensure_audio_apartment();

    let Some(endpoint_id) = default_output_endpoint_id() else {
        return host.default_output_device();
    };
    let device_id = cpal::DeviceId::new(cpal::HostId::Wasapi, &endpoint_id);
    match host.device_by_id(&device_id) {
        Some(device) => Some(device),
        None => {
            // Nothing to gain from failing here: a default that names an
            // endpoint the enumeration does not list is CPAL's own default
            // device's problem to have, not a reason for silence.
            warn!("默认输出端点未出现在设备枚举中（{endpoint_id}），回退到 CPAL 默认设备");
            host.default_output_device()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use windows::Win32::System::Com::COINIT_APARTMENTTHREADED;

    /// The invariant the apartment half rests on: once this thread is in the
    /// MTA, CPAL's own `CoInitializeEx(COINIT_APARTMENTTHREADED)` can no longer
    /// move it, and says so with the very HRESULT that used to surface from
    /// device probing. Runs on a thread of its own because it changes
    /// thread-wide state, and `cargo test` shares its threads between tests.
    #[test]
    fn ensuring_the_audio_apartment_pins_the_thread_to_the_mta() {
        std::thread::spawn(|| {
            ensure_audio_apartment();

            let mut apartment = APTTYPE::default();
            let mut qualifier = APTTYPEQUALIFIER::default();
            unsafe { CoGetApartmentType(&mut apartment, &mut qualifier) }
                .expect("apartment joined");
            assert_eq!(apartment, APTTYPE_MTA);
            assert!(apartment_note().contains("MTA"), "{}", apartment_note());

            // What CPAL does on first use of this thread. Tolerated there, so it
            // must stay tolerable here: the thread keeps the MTA.
            let cpal_init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            assert_eq!(cpal_init, RPC_E_CHANGED_MODE);

            // Idempotent: the second call must not add a reference, or a thread
            // that probes devices for a living would leak one per probe.
            ensure_audio_apartment();
            unsafe { CoGetApartmentType(&mut apartment, &mut qualifier) }
                .expect("apartment still joined");
            assert_eq!(apartment, APTTYPE_MTA);
        })
        .join()
        .expect("apartment test thread");
    }
}
