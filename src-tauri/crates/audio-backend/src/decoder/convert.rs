//! Turning decoded source frames into device frames: sample-rate conversion and
//! channel mixing.
//!
//! This is the stateless-per-stream half of the decoder. It owns no thread, no
//! file handle and no control protocol — it is handed a source and asked for
//! output frames, which makes it the part of the decode path that can be
//! reasoned about (and tested) on its own. `resample` sits beside it as the
//! rate-conversion kernel; this module is the channel-mixing layer plus the glue
//! that decides which of the two hot paths a given stream needs.

use std::time::Duration;

use rodio::Source;

use super::{resample, SeekableSymphoniaSource};

pub(super) struct FrameConverter {
    // Playback always uses our Symphonia source. Keeping the concrete type
    // here lets the compiler inline the per-sample iterator hot path instead
    // of paying a trait-object dispatch for every decoded sample.
    source: SeekableSymphoniaSource,
    input_channels: usize,
    output_channels: usize,
    matrix: Vec<Vec<(usize, f32)>>,
    /// Fixed-size scratch buffers, never reallocated or resized, so the
    /// per-frame hot path only overwrites existing samples.
    current_frame: Vec<f32>,
    /// One resampled input-channel frame, before channel mixing.
    resampled_frame: Vec<f32>,
    /// Scratch the resampler fills with each raw input frame it pulls.
    resample_scratch: Vec<f32>,
    /// `Some` only when input and output sample rates differ. Rate-matched
    /// playback skips the filter entirely rather than running it at unity.
    resampler: Option<resample::PolyphaseResampler>,
    /// `true` when channels map 1:1 (identity matrix), letting the no-resample
    /// path read straight into the output frame with no matrix mixing.
    passthrough: bool,
}

impl FrameConverter {
    pub(super) fn new(
        source: SeekableSymphoniaSource,
        input_channels: u16,
        input_sample_rate: u32,
        output_channels: u16,
        output_sample_rate: u32,
    ) -> Self {
        let input_channels = input_channels.max(1) as usize;
        let output_channels = output_channels.max(1) as usize;
        let input_sample_rate = input_sample_rate.max(1);
        let output_sample_rate = output_sample_rate.max(1);
        let matrix = build_mix_matrix(input_channels, output_channels);
        let passthrough = input_channels == output_channels
            && matrix
                .iter()
                .enumerate()
                .all(|(out, row)| row.len() == 1 && row[0].0 == out && row[0].1 == 1.0);
        let resampler = (input_sample_rate != output_sample_rate).then(|| {
            resample::PolyphaseResampler::new(input_channels, input_sample_rate, output_sample_rate)
        });
        Self {
            source,
            input_channels,
            output_channels,
            matrix,
            current_frame: vec![0.0; input_channels],
            resampled_frame: vec![0.0; input_channels],
            resample_scratch: vec![0.0; input_channels],
            resampler,
            passthrough,
        }
    }

    pub(super) fn seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.source.try_seek(pos)?;
        self.reset();
        Ok(())
    }

    fn reset(&mut self) {
        if let Some(resampler) = self.resampler.as_mut() {
            resampler.reset();
        }
    }

    #[inline]
    pub(super) fn can_bulk_append(&self) -> bool {
        // Channel mixing is the only thing that has to run frame by frame here;
        // rate conversion has its own block API.
        self.passthrough
    }

    /// Append up to `frames` complete output frames, copied straight through
    /// when the rates match and produced by the resampler when they do not.
    /// Only valid while `can_bulk_append()`; a short return means end of stream.
    pub(super) fn append_frames(&mut self, output: &mut Vec<f32>, frames: usize) -> usize {
        debug_assert!(self.can_bulk_append());

        let Self {
            source,
            input_channels,
            output_channels,
            resample_scratch,
            resampler,
            ..
        } = self;

        let Some(resampler) = resampler.as_mut() else {
            return source.append_interleaved_frames(output, *output_channels, frames);
        };

        let input_channels = *input_channels;
        let mut read = |frame: &mut [f32]| read_input_frame(source, input_channels, frame);
        resampler.append_frames(output, frames, &mut read, resample_scratch)
    }

    pub(super) fn next_frame(&mut self, output: &mut [f32]) -> Option<()> {
        debug_assert_eq!(output.len(), self.output_channels);

        // Destructured so the resampler's `read` closure can borrow the source
        // while the resampler itself is mutably borrowed.
        let Self {
            source,
            input_channels,
            matrix,
            current_frame,
            resampled_frame,
            resample_scratch,
            resampler,
            passthrough,
            ..
        } = self;
        let input_channels = *input_channels;

        let Some(resampler) = resampler.as_mut() else {
            // No sample-rate conversion: one input frame maps to one output frame.
            if *passthrough {
                // Hottest path (e.g. stereo file → stereo device): pull straight
                // into the output frame — no buffering, no matrix, no alloc.
                for slot in output.iter_mut() {
                    *slot = source.next()?;
                }
                return Some(());
            }
            if !read_input_frame(source, input_channels, current_frame) {
                return None;
            }
            apply_mix_matrix(matrix, current_frame, output);
            return Some(());
        };

        let mut read = |frame: &mut [f32]| read_input_frame(source, input_channels, frame);
        if *passthrough {
            // Identity channel map, so the resampler's frame *is* the output
            // frame — no staging buffer in between.
            resampler.next_frame(output, &mut read, resample_scratch)?;
        } else {
            resampler.next_frame(resampled_frame, &mut read, resample_scratch)?;
            apply_mix_matrix(matrix, resampled_frame, output);
        }
        Some(())
    }
}

/// Pull one interleaved input frame from `source` into a fixed-size scratch
/// buffer. Returns `false` if the source ends, leaving any partial frame to
/// be discarded by the caller — matching rodio's frame-aligned EOF behaviour.
#[inline]
fn read_input_frame(
    source: &mut SeekableSymphoniaSource,
    channels: usize,
    buf: &mut [f32],
) -> bool {
    debug_assert_eq!(buf.len(), channels);

    // Fast path: the whole frame sits inside the current decoded buffer, so
    // copy it as one slice — the resample/mix hot loop calls this per output
    // frame and the per-sample `next()` chain costs a branch per sample.
    let samples = source.buffer.samples();
    let offset = source.buffer_offset;
    if offset + channels <= samples.len() {
        buf[..channels].copy_from_slice(&samples[offset..offset + channels]);
        source.buffer_offset = offset + channels;
        return true;
    }

    // Slow path: the frame straddles a refill boundary (or the source ends).
    for slot in &mut buf[..channels] {
        match source.next() {
            Some(sample) => *slot = sample,
            None => return false,
        }
    }
    true
}

/// Apply the channel mix matrix for a single frame (no interpolation).
#[inline]
fn apply_mix_matrix(matrix: &[Vec<(usize, f32)>], frame: &[f32], output: &mut [f32]) {
    for (out, row) in output.iter_mut().zip(matrix) {
        let mut mixed = 0.0;
        for &(input, gain) in row {
            mixed += frame[input] * gain;
        }
        *out = mixed;
    }
}

fn build_mix_matrix(input_channels: usize, output_channels: usize) -> Vec<Vec<(usize, f32)>> {
    debug_assert!(input_channels > 0);
    debug_assert!(output_channels > 0);

    if output_channels == 1 {
        let gain = 1.0 / input_channels as f32;
        return vec![(0..input_channels).map(|ch| (ch, gain)).collect()];
    }

    if input_channels == 1 {
        return (0..output_channels).map(|_| vec![(0, 1.0)]).collect();
    }

    if output_channels == 2 {
        let mut rows = vec![vec![(0, 1.0)], vec![(1, 1.0)]];
        if input_channels > 2 {
            let gain = 0.5 / (input_channels - 2) as f32;
            for ch in 2..input_channels {
                rows[0].push((ch, gain));
                rows[1].push((ch, gain));
            }
        }
        return rows;
    }

    let mut rows = Vec::with_capacity(output_channels);
    for out in 0..output_channels {
        if out < input_channels {
            rows.push(vec![(out, 1.0)]);
        } else {
            rows.push(Vec::new());
        }
    }

    if input_channels > output_channels {
        let extra_count = input_channels - output_channels;
        let gain = 0.5 / extra_count as f32;
        for ch in output_channels..input_channels {
            rows[ch % output_channels].push((ch, gain));
        }
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_channel_counts_produce_the_identity_matrix() {
        // `FrameConverter::new` derives `passthrough` from exactly this shape,
        // and the decode loop's entire bulk path hangs off that flag — so this
        // is what keeps a stereo file on a stereo device out of the mixer.
        for channels in 1..=8 {
            let matrix = build_mix_matrix(channels, channels);
            assert_eq!(matrix.len(), channels);
            for (out, row) in matrix.iter().enumerate() {
                assert_eq!(
                    row.as_slice(),
                    [(out, 1.0)],
                    "channel {out} of {channels} is not a passthrough"
                );
            }
        }
    }

    #[test]
    fn mono_sources_feed_every_output_channel_at_unity() {
        assert_eq!(build_mix_matrix(1, 2), vec![vec![(0, 1.0)], vec![(0, 1.0)]]);
        assert_eq!(build_mix_matrix(1, 4).len(), 4);
    }

    #[test]
    fn downmixing_to_mono_averages_the_inputs() {
        let matrix = build_mix_matrix(2, 1);

        assert_eq!(matrix.len(), 1);
        let sum: f32 = matrix[0].iter().map(|&(_, gain)| gain).sum();
        assert!(
            (sum - 1.0).abs() < 1e-6,
            "mono downmix sums to {sum}, not unity"
        );
    }

    #[test]
    fn surround_to_stereo_keeps_the_front_pair_at_unity() {
        // 5.1 into a stereo device: front L/R pass through untouched and the
        // remaining four channels are spread across both at a reduced gain, so
        // a centre-heavy mix cannot swamp the front pair.
        let matrix = build_mix_matrix(6, 2);

        assert_eq!(matrix.len(), 2);
        assert_eq!(matrix[0][0], (0, 1.0));
        assert_eq!(matrix[1][0], (1, 1.0));
        for row in &matrix {
            let folded: f32 = row[1..].iter().map(|&(_, gain)| gain).sum();
            assert!(
                (folded - 0.5).abs() < 1e-6,
                "folded channels contribute {folded}, not 0.5"
            );
        }
    }

    #[test]
    fn a_mix_matrix_row_sums_its_contributions() {
        let matrix = build_mix_matrix(2, 1);
        let mut output = [0.0f32];

        apply_mix_matrix(&matrix, &[1.0, 0.0], &mut output);

        assert!((output[0] - 0.5).abs() < 1e-6);
    }
}
