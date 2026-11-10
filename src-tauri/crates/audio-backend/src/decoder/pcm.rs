//! Block access shared by concrete decoders; selected once before spawning playback.

use rodio::Source;

pub(crate) trait PcmSource: Source<Item = f32> + Send + Sized + 'static {
    fn samples_and_offset(&mut self) -> (&[f32], &mut usize);
    fn refill_buffer(&mut self) -> bool;

    #[inline]
    fn read_frame(&mut self, frame: &mut [f32]) -> bool {
        let channels = frame.len();
        let (samples, offset) = self.samples_and_offset();
        if *offset + channels <= samples.len() {
            frame.copy_from_slice(&samples[*offset..*offset + channels]);
            *offset += channels;
            return true;
        }
        for slot in frame {
            let Some(sample) = self.next() else {
                return false;
            };
            *slot = sample;
        }
        true
    }

    fn append_interleaved_frames(
        &mut self,
        dst: &mut Vec<f32>,
        channels: usize,
        max_frames: usize,
    ) -> usize {
        debug_assert_eq!(channels, self.channels().get() as usize);
        let mut copied_frames = 0;
        while copied_frames < max_frames {
            let (samples, offset) = self.samples_and_offset();
            if *offset >= samples.len() {
                if !self.refill_buffer() {
                    break;
                }
                continue;
            }
            let copied = super::append_available_interleaved_frames(
                samples,
                offset,
                dst,
                channels,
                max_frames - copied_frames,
            );
            if copied == 0 {
                *offset = samples.len();
            }
            copied_frames += copied;
        }
        copied_frames
    }
}
