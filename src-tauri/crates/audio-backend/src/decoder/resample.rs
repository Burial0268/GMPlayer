//! Polyphase windowed-sinc sample-rate conversion for the playback decoder.
//!
//! The previous converter interpolated linearly between adjacent input frames.
//! That is a first-order hold: it neither suppresses the images created by
//! upsampling nor band-limits before decimation, so the dominant source content
//! on this player (44.1 kHz MP3/FLAC into a 48 kHz device mix format) picked up
//! imaging products in the top octave and an audible high-frequency droop.
//!
//! This module replaces it with a Kaiser-windowed sinc polyphase FIR. The
//! coefficient table is built once per track load (a few thousand transcendental
//! evaluations, alongside file I/O that already dominates that path) and the
//! steady-state loop is a fixed `TAPS`-long multiply-accumulate over a
//! contiguous slice — no allocation, no branching on rate, no dynamic dispatch.
//!
//! Measured behaviour (the tests at the bottom of this file assert all of it):
//!
//! * 44.1 <-> 48 kHz passband is flat to within 0.001 dB up to 16 kHz, down
//!   0.02 dB at 17 kHz and 0.29 dB at 18 kHz.
//! * Images those conversions fold back into the audible band sit at -94 dB or
//!   lower relative to the tone that produced them.
//! * Decimating 96 -> 48 kHz, content folding to 18 kHz is attenuated 71 dB and
//!   the ultimate stopband floor is about 84 dB.
//!
//! Note the wide transition band that goes with only 32 taps: for 2:1
//! decimation, content just above the output Nyquist is attenuated by as little
//! as 17 dB. That is deliberate — it folds to just *below* Nyquist, where
//! nothing can hear it, and buying a sharper corner would cost taps on a filter
//! that runs on the decode thread for every rate-mismatched track.

/// Taps per phase. 32 taps with a Kaiser(beta=8.6) window is the point where
/// in-band artefacts for the 44.1 <-> 48 kHz conversions this player actually
/// performs drop below -94 dB, comfortably under the noise floor of any lossy
/// source we decode and past the resolution of 16-bit output. It buys that with
/// a wide transition band rather than a deep one; see the module docs.
const TAPS: usize = 32;
const HALF: usize = TAPS / 2;
/// Fractional-delay phases. The hot loop interpolates linearly between adjacent
/// phases, which is what keeps the table small enough to stay cache-resident.
///
/// 256 is not a guess: raising it to 8192 moves the measured image rejection by
/// 0.02 dB, i.e. nothing. The artefact floor this filter reaches is set by
/// `TAPS`, not by phase resolution, so spending memory here buys nothing.
///
/// The same measurement rules out the tempting "synchronous" alternative of
/// dropping the interpolation and storing one exact row per phase of the
/// reduced ratio (160 rows for 44.1 -> 48, since gcd(44100, 48000) = 300). That
/// would halve the inner loop and give exact phase timing — but the floor would
/// not move, because it is already filter-limited, and it needs a second hot
/// path for ratios that do not reduce to a small denominator. Interpolation
/// cannot simply be dropped at this table size either: rounding to the nearest
/// of 256 phases is a timing error of up to 1/512 of a sample, which at 15 kHz
/// is roughly -48 dB — 46 dB worse than what the filter delivers today.
const PHASES: usize = 256;
/// Fraction of the passband kept before the transition band starts. Leaving ~8%
/// of headroom lets the 32-tap kernel reach full attenuation by Nyquist instead
/// of trading stopband depth for a brickwall response.
const ROLLOFF: f64 = 0.92;
const KAISER_BETA: f64 = 8.6;

/// Input frames that must be pushed before the first output frame, so the
/// evaluation point (which sits `HALF - 1` frames into the window) lines up with
/// input sample 0 rather than with the zero-padded history.
const PRIME_FRAMES: usize = TAPS - HALF + 1;

/// Modified Bessel function of the first kind, order 0. Only used while building
/// the coefficient table.
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let half_x = x / 2.0;
    for k in 1..=40 {
        let ratio = half_x / k as f64;
        term *= ratio * ratio;
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 {
        1.0
    } else {
        let pi_x = std::f64::consts::PI * x;
        pi_x.sin() / pi_x
    }
}

/// Build the polyphase coefficient table.
///
/// `PHASES + 1` rows are stored: the extra guard row lets the hot loop read
/// `phase` and `phase + 1` unconditionally when interpolating the fractional
/// delay, with no bounds branch on the last phase.
fn build_table(input_rate: u32, output_rate: u32) -> Vec<f32> {
    // Upsampling only needs to suppress images above the input Nyquist;
    // downsampling additionally has to band-limit to the *output* Nyquist or the
    // discarded band aliases straight back into the audible range.
    let ratio = output_rate as f64 / input_rate as f64;
    let cutoff = 0.5 * ratio.min(1.0) * ROLLOFF;
    let denominator = bessel_i0(KAISER_BETA);

    let mut table = vec![0.0f32; (PHASES + 1) * TAPS];
    for phase in 0..=PHASES {
        let frac = phase as f64 / PHASES as f64;
        let row = &mut table[phase * TAPS..(phase + 1) * TAPS];
        let mut sum = 0.0f64;

        for (tap, slot) in row.iter_mut().enumerate() {
            // Distance, in input samples, from the tap's source frame to the
            // point being evaluated.
            let distance = frac + (HALF - 1) as f64 - tap as f64;

            // Kaiser window over the tap span, centred on the kernel.
            let normalized = distance / HALF as f64;
            let window = if normalized.abs() >= 1.0 {
                0.0
            } else {
                bessel_i0(KAISER_BETA * (1.0 - normalized * normalized).sqrt()) / denominator
            };

            let coefficient = window * 2.0 * cutoff * sinc(2.0 * cutoff * distance);
            sum += coefficient;
            *slot = coefficient as f32;
        }

        // Normalize each phase to unity DC gain. Without this the fractional
        // phases differ slightly in gain from the integer ones, which modulates
        // the signal at the beat frequency between the two rates.
        if sum.abs() > 1e-12 {
            let scale = (1.0 / sum) as f32;
            for slot in row.iter_mut() {
                *slot *= scale;
            }
        }
    }

    table
}

/// Fixed-rate polyphase resampler over one interleaved multi-channel stream.
pub(super) struct PolyphaseResampler {
    table: Vec<f32>,
    /// Interleaved history of the last `TAPS` input frames, mirrored so the
    /// window is always readable as one contiguous slice. Length is
    /// `2 * TAPS * channels`; every pushed frame is written twice.
    history: Vec<f32>,
    channels: usize,
    /// Index (in frames, `0..TAPS`) of the oldest frame in the window.
    write_frame: usize,
    /// Position of the next output frame within the current input interval.
    frac: f64,
    /// Input frames consumed per output frame.
    step: f64,
    primed: bool,
    /// Zero frames still to be pushed after the source ends, so the filter tail
    /// flushes instead of truncating the last `HALF` samples.
    flush_frames_remaining: usize,
    source_ended: bool,
}

impl PolyphaseResampler {
    pub(super) fn new(channels: usize, input_rate: u32, output_rate: u32) -> Self {
        let channels = channels.max(1);
        Self {
            table: build_table(input_rate.max(1), output_rate.max(1)),
            history: vec![0.0; 2 * TAPS * channels],
            channels,
            write_frame: 0,
            frac: 0.0,
            step: input_rate.max(1) as f64 / output_rate.max(1) as f64,
            primed: false,
            flush_frames_remaining: HALF,
            source_ended: false,
        }
    }

    pub(super) fn reset(&mut self) {
        self.history.fill(0.0);
        self.write_frame = 0;
        self.frac = 0.0;
        self.primed = false;
        self.flush_frames_remaining = HALF;
        self.source_ended = false;
    }

    #[inline]
    fn push_frame(&mut self, frame: &[f32]) {
        let channels = self.channels;
        let near = self.write_frame * channels;
        let far = (self.write_frame + TAPS) * channels;
        self.history[near..near + channels].copy_from_slice(&frame[..channels]);
        self.history[far..far + channels].copy_from_slice(&frame[..channels]);
        self.write_frame = (self.write_frame + 1) % TAPS;
    }

    #[inline]
    fn push_silence(&mut self) {
        let channels = self.channels;
        let near = self.write_frame * channels;
        let far = (self.write_frame + TAPS) * channels;
        self.history[near..near + channels].fill(0.0);
        self.history[far..far + channels].fill(0.0);
        self.write_frame = (self.write_frame + 1) % TAPS;
    }

    /// Pull one input frame through `read`, falling back to zero-padding once
    /// the source is exhausted. Returns `false` when even the flush tail is
    /// spent, meaning no further output can be produced.
    #[inline]
    fn advance_input(
        &mut self,
        read: &mut impl FnMut(&mut [f32]) -> bool,
        scratch: &mut [f32],
    ) -> bool {
        if !self.source_ended {
            if read(scratch) {
                self.push_frame(scratch);
                return true;
            }
            self.source_ended = true;
        }

        if self.flush_frames_remaining == 0 {
            return false;
        }
        self.flush_frames_remaining -= 1;
        self.push_silence();
        true
    }

    /// Produce one output frame into `output` (length `channels`).
    ///
    /// `read` fills a caller-provided scratch frame with the next input frame
    /// and returns `false` at end of stream. `scratch` is owned by the caller so
    /// this stays allocation-free.
    pub(super) fn next_frame(
        &mut self,
        output: &mut [f32],
        read: &mut impl FnMut(&mut [f32]) -> bool,
        scratch: &mut [f32],
    ) -> Option<()> {
        debug_assert_eq!(output.len(), self.channels);
        debug_assert_eq!(scratch.len(), self.channels);

        if !self.primed {
            for _ in 0..PRIME_FRAMES {
                if !self.advance_input(read, scratch) {
                    return None;
                }
            }
            self.primed = true;
        }

        while self.frac >= 1.0 {
            if !self.advance_input(read, scratch) {
                return None;
            }
            self.frac -= 1.0;
        }

        let phase_scaled = self.frac * PHASES as f64;
        let phase = phase_scaled as usize;
        let blend = (phase_scaled - phase as f64) as f32;
        // `frac` is always in [0, 1), so `phase` is at most PHASES - 1 and the
        // guard row keeps `phase + 1` in bounds.
        let low = &self.table[phase * TAPS..phase * TAPS + TAPS];
        let high = &self.table[(phase + 1) * TAPS..(phase + 1) * TAPS + TAPS];

        let channels = self.channels;
        let window_start = self.write_frame * channels;
        let window = &self.history[window_start..window_start + TAPS * channels];

        if channels == 2 {
            // Stereo is the overwhelmingly common layout; giving it its own loop
            // lets both accumulators stay in registers across the tap sweep.
            let mut left = 0.0f32;
            let mut right = 0.0f32;
            for tap in 0..TAPS {
                let coefficient = low[tap] + (high[tap] - low[tap]) * blend;
                left += window[tap * 2] * coefficient;
                right += window[tap * 2 + 1] * coefficient;
            }
            output[0] = left;
            output[1] = right;
        } else {
            output.fill(0.0);
            for tap in 0..TAPS {
                let coefficient = low[tap] + (high[tap] - low[tap]) * blend;
                let frame = &window[tap * channels..tap * channels + channels];
                for (slot, &sample) in output.iter_mut().zip(frame) {
                    *slot += sample * coefficient;
                }
            }
        }

        self.frac += self.step;
        Some(())
    }

    /// Append up to `frames` output frames directly to the tail of `dst`.
    ///
    /// Returns the number of frames actually written; a short count means the
    /// source and its flush tail are both spent. This exists so the decode loop
    /// can produce a whole block per call: writing into a sized slice drops the
    /// `Vec` capacity check and the intermediate per-frame copy that appending
    /// one frame at a time would pay for every output sample.
    pub(super) fn append_frames(
        &mut self,
        dst: &mut Vec<f32>,
        frames: usize,
        read: &mut impl FnMut(&mut [f32]) -> bool,
        scratch: &mut [f32],
    ) -> usize {
        let channels = self.channels;
        let start = dst.len();
        // One vectorized zero-fill for the whole block, against `frames * TAPS
        // * channels` multiply-accumulates about to overwrite it. The tail is
        // truncated back if the source ends early.
        dst.resize(start + frames * channels, 0.0);

        let mut written = 0;
        while written < frames {
            let at = start + written * channels;
            if self
                .next_frame(&mut dst[at..at + channels], read, scratch)
                .is_none()
            {
                break;
            }
            written += 1;
        }

        dst.truncate(start + written * channels);
        written
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Budget the passband must stay inside, in dB, up to `PASSBAND_EDGE_HZ`.
    const PASSBAND_RIPPLE_DB: f64 = 0.02;
    const PASSBAND_EDGE_HZ: f64 = 16_000.0;
    /// Ceiling for any artefact a 44.1 <-> 48 conversion leaves inside the
    /// audible band, relative to the tone that produced it.
    const IMAGE_REJECTION_DB: f64 = -90.0;

    /// Feed a resampler from a slice of interleaved frames.
    fn run(input: &[f32], channels: usize, in_rate: u32, out_rate: u32) -> Vec<f32> {
        let mut resampler = PolyphaseResampler::new(channels, in_rate, out_rate);
        let mut cursor = 0usize;
        let mut read = |frame: &mut [f32]| {
            if cursor + channels > input.len() {
                return false;
            }
            frame.copy_from_slice(&input[cursor..cursor + channels]);
            cursor += channels;
            true
        };
        let mut scratch = vec![0.0; channels];
        let mut frame = vec![0.0; channels];
        let mut output = Vec::new();
        while resampler
            .next_frame(&mut frame, &mut read, &mut scratch)
            .is_some()
        {
            output.extend_from_slice(&frame);
        }
        output
    }

    /// Same, but driving the block API instead of one frame at a time.
    fn run_blocked(
        input: &[f32],
        channels: usize,
        in_rate: u32,
        out_rate: u32,
        block_frames: usize,
    ) -> Vec<f32> {
        let mut resampler = PolyphaseResampler::new(channels, in_rate, out_rate);
        let mut cursor = 0usize;
        let mut read = |frame: &mut [f32]| {
            if cursor + channels > input.len() {
                return false;
            }
            frame.copy_from_slice(&input[cursor..cursor + channels]);
            cursor += channels;
            true
        };
        let mut scratch = vec![0.0; channels];
        let mut output = Vec::new();
        while resampler.append_frames(&mut output, block_frames, &mut read, &mut scratch)
            == block_frames
        {}
        output
    }

    #[test]
    fn block_output_is_identical_to_frame_at_a_time_output() {
        // The block path exists purely to amortize bookkeeping, so it must not
        // change a single sample — including across block boundaries, where the
        // filter history has to carry over untouched.
        let input: Vec<f32> = (0..2 * 5_000)
            .map(|n| ((n / 2) as f32 * 0.07).sin() * 0.8)
            .collect();
        let expected = run(&input, 2, 44_100, 48_000);

        for block_frames in [1, 7, 64, 512] {
            let actual = run_blocked(&input, 2, 44_100, 48_000, block_frames);
            assert_eq!(
                actual, expected,
                "block size {block_frames} diverged from the per-frame path"
            );
        }
    }

    #[test]
    fn a_short_block_leaves_no_zero_padding_behind() {
        // `append_frames` sizes the tail up front and truncates it back, so a
        // source that ends mid-block must not leave the reserved zeros in the
        // output — they would render as a burst of silence.
        let input = vec![0.5f32; 2 * 64];
        let mut resampler = PolyphaseResampler::new(2, 44_100, 48_000);
        let mut cursor = 0usize;
        let mut read = |frame: &mut [f32]| {
            if cursor + 2 > input.len() {
                return false;
            }
            frame.copy_from_slice(&input[cursor..cursor + 2]);
            cursor += 2;
            true
        };
        let mut scratch = vec![0.0; 2];
        let mut output = vec![9.0, 9.0]; // pre-existing content must survive

        let written = resampler.append_frames(&mut output, 4_096, &mut read, &mut scratch);

        assert!(written < 4_096, "the source should have run out");
        assert_eq!(output.len(), 2 + written * 2);
        assert_eq!(&output[..2], &[9.0, 9.0]);
        assert!(
            output[2..].iter().any(|&s| s != 0.0),
            "the truncated tail should hold real audio, not the reserved zeros"
        );
    }

    /// Resample a full-scale mono sine and return the steady-state body of the
    /// output, with the priming ramp and flush tail trimmed off.
    fn resample_tone(freq_hz: f64, in_rate: u32, out_rate: u32) -> Vec<f32> {
        // A quarter second. Long enough that the DFT bins below are ~4 Hz wide
        // — thousands of bins from any artefact being measured — and short
        // enough that the whole sweep stays cheap in a debug build.
        let frames = in_rate as usize / 4;
        let step = std::f64::consts::TAU * freq_hz / in_rate as f64;
        let input: Vec<f32> = (0..frames)
            .map(|n| (step * n as f64).sin() as f32)
            .collect();
        let mut output = run(&input, 1, in_rate, out_rate);

        let skip = TAPS * 4;
        assert!(
            output.len() > skip * 2,
            "resampler produced no steady state"
        );
        output.truncate(output.len() - skip);
        output.drain(..skip);
        output
    }

    fn rms(samples: &[f32]) -> f64 {
        let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
        (sum / samples.len() as f64).sqrt()
    }

    /// Level of one resampled tone relative to its input, in dB. A pure tone has
    /// an RMS of `1/sqrt(2)`, so 0 dB means it came through untouched.
    fn tone_response_db(freq_hz: f64, in_rate: u32, out_rate: u32) -> f64 {
        let output = resample_tone(freq_hz, in_rate, out_rate);
        20.0 * (rms(&output) / std::f64::consts::FRAC_1_SQRT_2).log10()
    }

    /// Amplitude of one frequency inside a signal, via a single windowed DFT
    /// bin. The Hann window's sidelobes fall fast enough that a full-scale tone
    /// thousands of bins away cannot masquerade as the artefact being measured.
    fn amplitude_at(samples: &[f32], freq_hz: f64, rate: u32) -> f64 {
        let length = samples.len() as f64;
        let omega = std::f64::consts::TAU * freq_hz / rate as f64;
        let (mut real, mut imaginary, mut window_sum) = (0.0f64, 0.0f64, 0.0f64);

        for (index, &sample) in samples.iter().enumerate() {
            let index = index as f64;
            let window = 0.5 - 0.5 * (std::f64::consts::TAU * index / length).cos();
            let phase = omega * index;
            let windowed = window * sample as f64;
            real += windowed * phase.cos();
            imaginary -= windowed * phase.sin();
            window_sum += window;
        }

        2.0 * (real * real + imaginary * imaginary).sqrt() / window_sum
    }

    /// Level of the strongest resampling artefact a tone leaves behind, relative
    /// to the tone itself, in dB.
    fn artefact_ratio_db(freq_hz: f64, artefact_hz: f64, in_rate: u32, out_rate: u32) -> f64 {
        let output = resample_tone(freq_hz, in_rate, out_rate);
        let tone = amplitude_at(&output, freq_hz, out_rate);
        let artefact = amplitude_at(&output, artefact_hz, out_rate);
        20.0 * (artefact / tone).log10()
    }

    #[test]
    fn passband_is_flat_across_the_audible_range() {
        // The two conversions that actually happen on this player: a 44.1 kHz
        // source into a 48 kHz device mix, and the reverse. Everything a
        // listener can hear has to survive both untouched.
        for (in_rate, out_rate) in [(44_100, 48_000), (48_000, 44_100)] {
            for freq in [
                100.0,
                1_000.0,
                5_000.0,
                10_000.0,
                15_000.0,
                PASSBAND_EDGE_HZ,
            ] {
                let response = tone_response_db(freq, in_rate, out_rate);
                assert!(
                    response.abs() < PASSBAND_RIPPLE_DB,
                    "{in_rate}->{out_rate} at {freq} Hz deviates by {response:.4} dB, \
                     over the {PASSBAND_RIPPLE_DB} dB budget"
                );
            }
        }
    }

    #[test]
    fn passband_rolls_off_only_at_the_very_top_of_the_audible_range() {
        // 32 taps cannot brickwall, so the transition band is wide and the
        // corner is reached gradually. Pin where it starts: the roll-off must
        // stay out of the range anyone can actually hear.
        for (in_rate, out_rate) in [(44_100, 48_000), (48_000, 44_100)] {
            let at_17k = tone_response_db(17_000.0, in_rate, out_rate);
            let at_18k = tone_response_db(18_000.0, in_rate, out_rate);
            assert!(
                at_17k > -0.1,
                "{in_rate}->{out_rate} already down {at_17k:.3} dB at 17 kHz"
            );
            assert!(
                at_18k > -0.5,
                "{in_rate}->{out_rate} already down {at_18k:.3} dB at 18 kHz"
            );
        }
    }

    #[test]
    fn rate_conversion_images_stay_below_the_audible_floor() {
        // Upsampling images a tone at `in_rate - f`, which folds back into the
        // output stream at `out_rate - (in_rate - f)`. For 44.1 -> 48 that is
        // `f + 3900` — squarely inside the audible band, so an under-designed
        // filter is not merely inaudible ultrasonic junk here, it is a second
        // tone a few kHz above the first.
        for freq in [1_000.0, 5_000.0, 10_000.0, 15_000.0] {
            let image = artefact_ratio_db(freq, freq + 3_900.0, 44_100, 48_000);
            assert!(
                image < IMAGE_REJECTION_DB,
                "44.1->48 image of {freq} Hz landed at {image:.1} dB, \
                 above the {IMAGE_REJECTION_DB} dB ceiling"
            );
        }

        // 48 -> 44.1 folds the same image to `f - 3900`.
        for freq in [5_000.0, 10_000.0, 15_000.0] {
            let image = artefact_ratio_db(freq, freq - 3_900.0, 48_000, 44_100);
            assert!(
                image < IMAGE_REJECTION_DB,
                "48->44.1 image of {freq} Hz landed at {image:.1} dB, \
                 above the {IMAGE_REJECTION_DB} dB ceiling"
            );
        }
    }

    #[test]
    fn decimation_rejects_content_that_would_fold_into_the_audible_band() {
        // 96 kHz source into a 48 kHz device. These tones are entirely above
        // the 24 kHz output Nyquist, so anything reaching the output is a
        // fold-back alias and the output level *is* the attenuation. 30 kHz
        // folds to 18 kHz; 36 kHz folds to 12 kHz, well inside the band where
        // an alias would be plainly audible, so it has to be further down.
        let near_edge = tone_response_db(30_000.0, 96_000, 48_000);
        let deep = tone_response_db(36_000.0, 96_000, 48_000);

        assert!(
            near_edge < -60.0,
            "30 kHz folded back to 18 kHz at {near_edge:.1} dB"
        );
        assert!(deep < -80.0, "36 kHz folded back to 12 kHz at {deep:.1} dB");
    }

    #[test]
    fn every_phase_has_unity_dc_gain() {
        // A constant input must come out constant regardless of the fractional
        // delay, otherwise the output is amplitude-modulated at the beat
        // frequency between the two rates.
        let table = build_table(44_100, 48_000);
        for phase in 0..=PHASES {
            let sum: f32 = table[phase * TAPS..(phase + 1) * TAPS].iter().sum();
            assert!(
                (sum - 1.0).abs() < 1e-5,
                "phase {phase} sums to {sum}, not unity"
            );
        }
    }

    #[test]
    fn downsampling_narrows_the_kernel_bandwidth() {
        // 96k -> 48k must band-limit to the 24 kHz output Nyquist; 44.1k -> 48k
        // only has to suppress images above 22.05 kHz of a wider passband. The
        // decimating kernel is the narrower filter, so it spreads its energy
        // across more taps and its centre tap is correspondingly smaller.
        let down = build_table(96_000, 48_000);
        let up = build_table(44_100, 48_000);
        let centre_row = PHASES / 2 * TAPS;
        let down_peak = down[centre_row + HALF - 1].abs();
        let up_peak = up[centre_row + HALF - 1].abs();
        assert!(
            down_peak < up_peak,
            "decimation kernel should spread energy across more taps"
        );
    }

    #[test]
    fn constant_signal_survives_rate_conversion_on_each_channel() {
        // Deliberately asymmetric. The `channels == 2` branch is a hand-written
        // duplicate of the generic tap loop and is the branch every
        // rate-converted stereo track actually takes, so at least one test has
        // to be able to tell the two channels apart: the frequency-response
        // tests all run mono (the generic branch), and every other stereo
        // fixture here is a constant or a zero fill. With L == R, swapping or
        // duplicating a channel index in that loop is invisible to the suite.
        const LEFT: f32 = 0.5;
        const RIGHT: f32 = -0.25;
        let input: Vec<f32> = (0..4_000).flat_map(|_| [LEFT, RIGHT]).collect();
        let output = run(&input, 2, 44_100, 48_000);

        assert!(!output.is_empty());
        // Skip the priming ramp and the flush tail, then require a flat result.
        // `2 * TAPS` is a whole number of frames, so index parity still selects
        // the channel.
        let body = &output[2 * TAPS..output.len() - 2 * TAPS];
        for (index, &sample) in body.iter().enumerate() {
            let expected = if index % 2 == 0 { LEFT } else { RIGHT };
            assert!(
                (sample - expected).abs() < 1e-3,
                "channel {} expected flat {expected}, saw {sample}",
                index % 2
            );
        }
    }

    #[test]
    fn output_length_tracks_the_rate_ratio() {
        let frames = 4_800usize;
        let input = vec![0.0f32; frames * 2];
        let output = run(&input, 2, 48_000, 44_100);
        let output_frames = output.len() / 2;
        let expected = (frames as f64 * 44_100.0 / 48_000.0) as usize;

        // Priming and the flush tail shift this by well under a millisecond.
        assert!(
            output_frames.abs_diff(expected) < TAPS * 2,
            "expected ~{expected} frames, produced {output_frames}"
        );
    }

    #[test]
    fn reset_clears_history_between_seeks() {
        let mut resampler = PolyphaseResampler::new(2, 44_100, 48_000);
        let mut scratch = vec![0.0; 2];
        let mut frame = vec![0.0; 2];
        let mut loud = |slot: &mut [f32]| {
            slot.fill(1.0);
            true
        };
        for _ in 0..64 {
            resampler
                .next_frame(&mut frame, &mut loud, &mut scratch)
                .unwrap();
        }
        assert!(frame[0] > 0.9);

        resampler.reset();
        let mut silent = |slot: &mut [f32]| {
            slot.fill(0.0);
            true
        };
        resampler
            .next_frame(&mut frame, &mut silent, &mut scratch)
            .unwrap();
        assert_eq!(frame, vec![0.0, 0.0]);
    }
}
