use cpal::{BufferSize, SupportedBufferSize};

// Match Android's deep software queue: Pulse/PipeWire server scheduling adds
// the same kind of decode→callback jitter as the Android scheduler, and an
// underrunning pulse stream accumulates extra sink latency it never gives
// back. Absorb that jitter in our own preallocated ring rather than in the
// device buffer requested below.
pub(in crate::output) const DEFAULT_QUEUE_BLOCKS: usize = 48;

// CPAL's PulseAudio backend maps `BufferSize::Default` onto an all-`u32::MAX`
// `BufferAttr`, the wire-protocol sentinel for "server picks", and only sets
// the `adjust_latency` stream flag for `BufferSize::Fixed`. A real PulseAudio
// daemon happens to pick a small buffer; pipewire-pulse picks ~2s, and nothing
// then asks it to target a latency at all (RustAudio/cpal#1190). One oversized
// server buffer surfaces as four separate bugs:
//   * ~1.5s between pressing play and hearing audio;
//   * an equally large lyric/progress lead, because the render clock counts
//     samples handed to the server, not samples played;
//   * the same wait on every resume, since the paused callback keeps feeding
//     the server silence instead of stopping the stream;
//   * up to ~1.5s of pre-seek audio after a seek, because flushing our ring
//     cannot revoke PCM the server already holds.
// Requesting an explicit period fixes all four: CPAL then sends a bounded
// `BufferAttr` and asks the server to hit that latency end-to-end.
//
// Sized in milliseconds rather than frames because CPAL doubles the period
// into the Pulse max/target length; a frame constant would silently halve the
// latency target on a 96kHz sink and quarter it at 192kHz. 20ms mirrors the
// Windows/macOS policy in `desktop.rs` and stays far inside the 48-block
// (~512ms) mixer ring, which remains the scheduler-jitter margin.
const STABLE_OUTPUT_BUFFER_MS: u32 = 20;
const MIN_STABLE_OUTPUT_BUFFER_FRAMES: u32 = 512;

pub(in crate::output) fn stable_buffer_size(
    sample_rate: u32,
    supported: &SupportedBufferSize,
) -> BufferSize {
    let target_frames =
        ((sample_rate.max(1) as u64 * STABLE_OUTPUT_BUFFER_MS as u64) / 1_000) as u32;
    let target_frames = target_frames.max(MIN_STABLE_OUTPUT_BUFFER_FRAMES);
    match supported {
        // Clamp without assuming `min <= max`: the bounds come from the
        // backend, and `u32::clamp` would panic the output thread on an
        // inverted range instead of degrading to a supported size.
        SupportedBufferSize::Range { min, max } => {
            BufferSize::Fixed(target_frames.max(*min).min(*max))
        }
        // A backend that cannot report its range is the one case where forcing
        // a period risks failing stream creation outright rather than merely
        // raising latency. CPAL's ALSA host reports `Unknown` when hw params
        // are unreadable, and ALSA-direct users have no Pulse buffer to fix,
        // so leave device policy in charge there.
        SupportedBufferSize::Unknown => BufferSize::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_buffer_size_requests_bounded_period_instead_of_server_default() {
        let buffer = stable_buffer_size(
            48_000,
            &SupportedBufferSize::Range {
                min: 128,
                max: 2_048,
            },
        );

        assert_eq!(buffer, BufferSize::Fixed(960));
    }

    #[test]
    fn stable_buffer_size_holds_latency_constant_across_sample_rates() {
        let range = SupportedBufferSize::Range {
            min: 128,
            max: 8_192,
        };

        // 20ms at every rate, instead of a frame count that shrinks the
        // latency target as the sink rate climbs.
        assert_eq!(stable_buffer_size(44_100, &range), BufferSize::Fixed(882));
        assert_eq!(stable_buffer_size(48_000, &range), BufferSize::Fixed(960));
        assert_eq!(stable_buffer_size(96_000, &range), BufferSize::Fixed(1_920));
        assert_eq!(
            stable_buffer_size(192_000, &range),
            BufferSize::Fixed(3_840)
        );
    }

    #[test]
    fn stable_buffer_size_keeps_a_floor_for_low_rate_sinks() {
        let buffer = stable_buffer_size(
            8_000,
            &SupportedBufferSize::Range {
                min: 128,
                max: 2_048,
            },
        );

        assert_eq!(buffer, BufferSize::Fixed(MIN_STABLE_OUTPUT_BUFFER_FRAMES));
    }

    #[test]
    fn stable_buffer_size_clamps_to_supported_range() {
        let buffer = stable_buffer_size(48_000, &SupportedBufferSize::Range { min: 256, max: 512 });

        assert_eq!(buffer, BufferSize::Fixed(512));
        assert_eq!(
            stable_buffer_size(
                8_000,
                &SupportedBufferSize::Range {
                    min: 1_024,
                    max: 4_096
                }
            ),
            BufferSize::Fixed(1_024),
        );
    }

    #[test]
    fn stable_buffer_size_survives_an_inverted_range() {
        let buffer = stable_buffer_size(
            48_000,
            &SupportedBufferSize::Range {
                min: 2_048,
                max: 128,
            },
        );

        assert_eq!(buffer, BufferSize::Fixed(128));
    }

    #[test]
    fn stable_buffer_size_defers_to_device_policy_for_unknown_range() {
        assert_eq!(
            stable_buffer_size(48_000, &SupportedBufferSize::Unknown),
            BufferSize::Default,
        );
    }
}
