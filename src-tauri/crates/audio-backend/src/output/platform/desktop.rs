use cpal::{BufferSize, SupportedBufferSize};

pub(in crate::output) const DEFAULT_QUEUE_BLOCKS: usize = 8;

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
        SupportedBufferSize::Range { min, max } => {
            BufferSize::Fixed(target_frames.clamp(*min, *max))
        }
        SupportedBufferSize::Unknown => BufferSize::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_buffer_size_uses_supported_range() {
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
    fn stable_buffer_size_clamps_to_supported_range() {
        let buffer = stable_buffer_size(44_100, &SupportedBufferSize::Range { min: 128, max: 512 });

        assert_eq!(buffer, BufferSize::Fixed(512));
    }
}
