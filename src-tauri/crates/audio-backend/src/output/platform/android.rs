use cpal::{BufferSize, SupportedBufferSize};

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
    BufferSize::Default
}

#[cfg(test)]
mod tests {
    use super::*;

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
