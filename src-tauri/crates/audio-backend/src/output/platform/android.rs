use cpal::{BufferSize, SupportedBufferSize};

// Keep the hardware callback buffer modest, but give the mixer a deeper
// software queue to absorb Android scheduler jitter between decode and AAudio.
pub(in crate::output) const DEFAULT_QUEUE_BLOCKS: usize = 48;

const STABLE_OUTPUT_BUFFER_MS: u32 = 40;
const MIN_STABLE_OUTPUT_BUFFER_FRAMES: u32 = 512;

pub(in crate::output) fn stable_buffer_size(
    sample_rate: u32,
    supported: &SupportedBufferSize,
) -> BufferSize {
    let target_frames =
        ((sample_rate.max(1) as u64 * STABLE_OUTPUT_BUFFER_MS as u64) / 1_000) as u32;
    let target_frames = target_frames.max(MIN_STABLE_OUTPUT_BUFFER_FRAMES);
    match supported {
        SupportedBufferSize::Range { min, max } => {
            BufferSize::Fixed(target_frames.clamp(*min, *max))
        }
        SupportedBufferSize::Unknown => BufferSize::Fixed(target_frames),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_buffer_size_uses_android_stable_range() {
        let buffer = stable_buffer_size(
            48_000,
            &SupportedBufferSize::Range {
                min: 128,
                max: 4_096,
            },
        );

        assert_eq!(buffer, BufferSize::Fixed(1_920));
    }
}
