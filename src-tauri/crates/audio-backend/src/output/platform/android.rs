use cpal::{BufferSize, DeviceDescription, DeviceType, InterfaceType, SupportedBufferSize};

// Keep a deep software queue to absorb Android scheduler jitter between decode
// and AAudio without forcing large real-time callbacks.
pub(in crate::output) const DEFAULT_QUEUE_BLOCKS: usize = 48;

pub(in crate::output) fn stable_buffer_size(
    _sample_rate: u32,
    _supported: &SupportedBufferSize,
) -> BufferSize {
    // CPAL's AAudio backend uses Default to request the native burst-sized
    // callback and dynamically grows the hardware buffer after underruns.
    // A Fixed size disables that tuning and turns a 40 ms target into one
    // large real-time callback, which is more vulnerable to CPU-pressure
    // deadline misses. The preallocated 48-block rings and player prebuffer
    // remain the scheduler-jitter safety margin outside the callback.
    //
    // That tuning only grows `buffer_size_in_frames` up to the stream's
    // *capacity*, which is why this crate does not enable CPAL's `realtime`
    // feature for Android: it would open the stream in
    // AAUDIO_PERFORMANCE_MODE_LOW_LATENCY, where the capacity is a couple of
    // bursts and the self-healing has almost nothing to grow into. See the
    // `cpal` tables in Cargo.toml.
    BufferSize::Default
}

/// Whether an `AudioManager.getDevices()` output is somewhere a user would send music.
///
/// The list also carries the call and radio paths the phone has wired up: the earpiece, the
/// FM tuner, `TYPE_TELEPHONY` (CPAL knows neither its type nor its interface, so it showed as a
/// bare model name) and `TYPE_BUS`. CPAL's description is all there is to tell them apart by —
/// it keeps the raw Android type private — which is why A2DP, which CPAL also leaves `Unknown`,
/// is kept by its Bluetooth interface.
pub(in crate::output) fn is_media_output(description: &DeviceDescription) -> bool {
    match (description.device_type(), description.interface_type()) {
        (
            DeviceType::Earpiece
            | DeviceType::Handset
            | DeviceType::Tuner
            | DeviceType::Microphone
            | DeviceType::Virtual,
            _,
        ) => false,
        // shortcut: this is Bluetooth SCO (the call link, mono 8-16 kHz) but also an LE Audio
        // headset, which CPAL maps the same way. LE Audio still plays through the default
        // route; tell them apart through our own `getDevices()` call if that is ever needed.
        (DeviceType::Headset, InterfaceType::Bluetooth) => false,
        (DeviceType::Unknown, InterfaceType::Unknown) => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpal::DeviceDescriptionBuilder;

    fn described(device_type: DeviceType, interface: InterfaceType) -> DeviceDescription {
        DeviceDescriptionBuilder::new("2510DRK44C")
            .device_type(device_type)
            .interface_type(interface)
            .build()
    }

    #[test]
    fn only_media_sinks_are_offered() {
        for (device_type, interface, offered) in [
            (DeviceType::Speaker, InterfaceType::BuiltIn, true),
            (DeviceType::Headphones, InterfaceType::Unknown, true),
            (DeviceType::Headset, InterfaceType::Usb, true),
            // A2DP: CPAL has no device type for it.
            (DeviceType::Unknown, InterfaceType::Bluetooth, true),
            (DeviceType::Unknown, InterfaceType::Hdmi, true),
            (DeviceType::Earpiece, InterfaceType::BuiltIn, false),
            (DeviceType::Tuner, InterfaceType::Unknown, false),
            (DeviceType::Headset, InterfaceType::Bluetooth, false),
            // TYPE_TELEPHONY, TYPE_BUS.
            (DeviceType::Unknown, InterfaceType::Unknown, false),
        ] {
            assert_eq!(
                is_media_output(&described(device_type, interface)),
                offered,
                "{device_type:?} over {interface:?}"
            );
        }
    }

    #[test]
    fn stable_buffer_size_enables_aaudio_dynamic_tuning_for_known_range() {
        assert_eq!(
            stable_buffer_size(
                48_000,
                &SupportedBufferSize::Range {
                    min: 128,
                    max: 4_096,
                },
            ),
            BufferSize::Default,
        );
    }

    #[test]
    fn stable_buffer_size_enables_aaudio_dynamic_tuning_for_unknown_range() {
        assert_eq!(
            stable_buffer_size(44_100, &SupportedBufferSize::Unknown),
            BufferSize::Default,
        );
    }
}
