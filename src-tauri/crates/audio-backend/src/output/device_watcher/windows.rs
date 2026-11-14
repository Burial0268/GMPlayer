//! MMDevAPI notifications. The process registration outlives every output stream.

use std::sync::{Arc, OnceLock};

use tracing::warn;
use windows::core::PCWSTR;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{
    eConsole, eRender, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, MMDeviceEnumerator, DEVICE_STATE,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

use super::{PendingChanges, PlatformBackend, DEFAULT_CHANGED};

pub(super) struct Backend {
    watch: &'static ProcessWatch,
}

impl PlatformBackend for Backend {
    fn start() -> Option<Self> {
        static WATCH: OnceLock<Option<ProcessWatch>> = OnceLock::new();
        WATCH
            .get_or_init(|| {
                super::super::platform::ensure_audio_apartment();
                let enumerator: IMMDeviceEnumerator =
                    match unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) } {
                        Ok(enumerator) => enumerator,
                        Err(error) => {
                            warn!("默认输出设备监听不可用（枚举器创建失败）：{error:?}");
                            return None;
                        }
                    };
                let events = Arc::new(PendingChanges::default());
                let client: IMMNotificationClient = DefaultOutputNotifier {
                    events: Arc::clone(&events),
                }
                .into();
                if let Err(error) =
                    unsafe { enumerator.RegisterEndpointNotificationCallback(&client) }
                {
                    warn!("默认输出设备监听注册失败，改由设备轮询兜底：{error:?}");
                    return None;
                }
                Some(ProcessWatch {
                    events,
                    _enumerator: enumerator,
                    _client: client,
                })
            })
            .as_ref()
            .map(|watch| Self { watch })
    }

    fn events(&self) -> &PendingChanges {
        &self.watch.events
    }
}

// No shutdown COM call: these interfaces are only stored after registration,
// and MMDevAPI invokes the client without changing an OS-owned thread's apartment.
struct ProcessWatch {
    events: Arc<PendingChanges>,
    _enumerator: IMMDeviceEnumerator,
    _client: IMMNotificationClient,
}

// SAFETY: created and registered on one initialized thread, then never called
// again. The callback touches only the shared atomic event flags.
unsafe impl Send for ProcessWatch {}
unsafe impl Sync for ProcessWatch {}

#[windows::core::implement(IMMNotificationClient)]
struct DefaultOutputNotifier {
    events: Arc<PendingChanges>,
}

impl IMMNotificationClient_Impl for DefaultOutputNotifier_Impl {
    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        _device: &PCWSTR,
    ) -> windows::core::Result<()> {
        // This is the role resolved by platform::default_output_device.
        if flow == eRender && role == eConsole {
            self.events.record(DEFAULT_CHANGED);
        }
        Ok(())
    }

    // Other changes are caught by output failure/stall checks and the probe.
    // Querying devices here would initialize COM on MMDevAPI's shared threads.
    fn OnDeviceStateChanged(
        &self,
        _device: &PCWSTR,
        _state: DEVICE_STATE,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDeviceAdded(&self, _device: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDeviceRemoved(&self, _device: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnPropertyValueChanged(
        &self,
        _device: &PCWSTR,
        _key: &PROPERTYKEY,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Media::Audio::{eCapture, eCommunications};

    #[test]
    fn the_console_render_default_is_reported_once_through_the_real_vtable() {
        let events = Arc::new(PendingChanges::default());
        let client: IMMNotificationClient = DefaultOutputNotifier {
            events: Arc::clone(&events),
        }
        .into();
        unsafe {
            client.OnDeviceAdded(PCWSTR::null()).unwrap();
            client.OnDeviceRemoved(PCWSTR::null()).unwrap();
            client
                .OnDefaultDeviceChanged(eCapture, eConsole, PCWSTR::null())
                .unwrap();
            client
                .OnDefaultDeviceChanged(eRender, eCommunications, PCWSTR::null())
                .unwrap();
        }
        assert_eq!(events.take().0, 0);
        unsafe {
            client
                .OnDefaultDeviceChanged(eRender, eConsole, PCWSTR::null())
                .unwrap();
        }
        assert_eq!(events.take().0, DEFAULT_CHANGED);
        assert_eq!(events.take().0, 0);
    }
}
