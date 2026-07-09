pub(in crate::output) fn default_output_id() -> Option<String> {
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_APARTMENTTHREADED,
    };

    struct ComGuard(bool);

    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    struct IdGuard(windows::core::PWSTR);

    impl Drop for IdGuard {
        fn drop(&mut self) {
            unsafe {
                CoTaskMemFree(Some(self.0.as_ptr() as *const _));
            }
        }
    }

    let init_result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if !init_result.is_ok() && init_result != RPC_E_CHANGED_MODE {
        return None;
    }
    let _com_guard = ComGuard(init_result.is_ok());

    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()? };
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()? };
    let id = IdGuard(unsafe { device.GetId().ok()? });
    let id = unsafe { id.0.to_string().ok()? };

    Some(format!("wasapi:{id}"))
}
