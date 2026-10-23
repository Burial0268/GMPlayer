#[cfg(target_os = "android")]
mod android;
#[cfg(not(any(target_os = "android", target_os = "linux")))]
mod desktop;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod other;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "android")]
pub(super) use android::{stable_buffer_size, DEFAULT_QUEUE_BLOCKS};
#[cfg(not(any(target_os = "android", target_os = "linux")))]
pub(super) use desktop::{stable_buffer_size, DEFAULT_QUEUE_BLOCKS};
#[cfg(target_os = "linux")]
pub(super) use linux::{stable_buffer_size, DEFAULT_QUEUE_BLOCKS};

#[cfg(target_os = "linux")]
pub(super) use linux::default_output_id;
#[cfg(target_os = "macos")]
pub(super) use macos::default_output_id;
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub(super) use other::default_output_id;
#[cfg(target_os = "windows")]
pub(super) use windows::{
    apartment_note, default_output_device, default_output_id, ensure_audio_apartment,
    take_default_output_changed,
};

/// COM apartments are a Windows-only concern — see
/// `windows::ensure_audio_apartment` for what depends on it there. Every other
/// host talks to its audio API from whatever thread calls it.
#[cfg(not(target_os = "windows"))]
pub(super) fn ensure_audio_apartment() {}

#[cfg(not(target_os = "windows"))]
pub(super) fn apartment_note() -> String {
    String::new()
}

/// Let CPAL resolve the default device everywhere except Windows. No other host
/// reaches it through an activation API that can fail the way WASAPI's does, and
/// the one that reroutes a live stream when the server default moves
/// (PulseAudio) is better off keeping the alias it resolves server-side — see
/// `resolved_default_output_id` below.
#[cfg(not(target_os = "windows"))]
pub(super) fn default_output_device(host: &cpal::Host) -> Option<cpal::Device> {
    use cpal::traits::HostTrait;

    host.default_output_device()
}

/// Only Windows pushes default-device changes to this crate (see
/// `windows::take_default_output_changed`); everywhere else the periodic device
/// probe is the signal, and `poll_output_device_tick` already owns it.
#[cfg(not(target_os = "windows"))]
pub(super) fn take_default_output_changed() -> bool {
    false
}

/// Accept a CPAL device id as the identity of the *system default* output only
/// when it names the device the default actually resolves to.
///
/// CPAL's PulseAudio host resolves `@DEFAULT_SINK@` server-side and reports the
/// sink it landed on, so the id moves as soon as the server default moves — an
/// exact, roundtrip-free identity. Its ALSA host always reports the virtual
/// `default` PCM instead, an id that never changes and would therefore hide
/// every device switch; those hosts must keep the enumeration-based signature.
#[cfg(any(target_os = "linux", test))]
pub(super) fn resolved_default_output_id(device_id: &str) -> Option<&str> {
    let sink = device_id.strip_prefix("pulseaudio:")?.trim();
    // `@DEFAULT_SINK@`/`default` are the server-side aliases, not a resolved
    // sink: they stay constant across device switches.
    if sink.is_empty() || sink.starts_with('@') || sink.eq_ignore_ascii_case("default") {
        return None;
    }
    Some(device_id)
}

/// Device half of the same rule. It lives here rather than in `linux.rs` so it
/// is compiled — and therefore type-checked — by `cargo test` on every host;
/// Linux-gated code cannot be built from Windows or macOS.
#[cfg(any(target_os = "linux", test))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(super) fn resolved_default_output_device_id(device: &cpal::Device) -> Option<String> {
    use cpal::traits::DeviceTrait;

    let device_id = device.id().ok()?.to_string();
    resolved_default_output_id(&device_id).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_sink_names_identify_the_default_output() {
        assert_eq!(
            resolved_default_output_id("pulseaudio:alsa_output.pci-0000_00_1f.3.analog-stereo"),
            Some("pulseaudio:alsa_output.pci-0000_00_1f.3.analog-stereo"),
        );
    }

    #[test]
    fn alias_and_non_pulse_ids_fall_back_to_the_device_signature() {
        // ALSA always reports the virtual default PCM.
        assert_eq!(resolved_default_output_id("alsa:default"), None);
        assert_eq!(resolved_default_output_id("alsa:hw:CARD=PCH,DEV=0"), None);
        // Unresolved PulseAudio aliases carry no device identity.
        assert_eq!(
            resolved_default_output_id("pulseaudio:@DEFAULT_SINK@"),
            None
        );
        assert_eq!(resolved_default_output_id("pulseaudio:default"), None);
        assert_eq!(resolved_default_output_id("pulseaudio:"), None);
    }
}
