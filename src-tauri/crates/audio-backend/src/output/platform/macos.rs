pub(in crate::output) fn default_output_id() -> Option<String> {
    use coreaudio::sys::{
        kAudioHardwareNoError, kAudioHardwarePropertyDefaultOutputDevice,
        kAudioObjectPropertyElementMaster, kAudioObjectPropertyScopeGlobal,
        kAudioObjectSystemObject, AudioDeviceID, AudioObjectGetPropertyData,
        AudioObjectPropertyAddress,
    };
    use std::mem;
    use std::ptr::null;

    let property_address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    let mut audio_device_id: AudioDeviceID = 0;
    let mut data_size = mem::size_of::<AudioDeviceID>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &property_address as *const _,
            0,
            null(),
            &mut data_size as *mut _,
            &mut audio_device_id as *mut _ as *mut _,
        )
    };
    if status != kAudioHardwareNoError as i32 || audio_device_id == 0 {
        return None;
    }

    Some(format!("coreaudio:{audio_device_id}"))
}
