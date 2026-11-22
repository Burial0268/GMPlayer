//! Finite AutoMix stretching, adapted from SPlayer-Next PR #429.
//! Real look-ahead primes the algorithm; synthetic drain samples never advance source time.

use signalsmith_stretch::Stretch;

/// Rates a stretch may reach. A rate held through a beat-mix stays within 4% (`beat_plan`); a ride
/// that eases into the next song's tempo over A's last bars may go further.
pub(crate) const RATE_RANGE: std::ops::RangeInclusive<f64> = 0.92..=1.08;

/// -1.5 dBFS: below it a stretched sample passes untouched.
const PEAK_KNEE: f32 = 0.841_395;
/// -0.26 dBFS, the mixer's overlap ceiling (`OVERLAP_LIMIT_DB`).
const PEAK_CEILING: f32 = 0.970_510;

/// Resynthesis loses a mastered track's phase alignment, not its power: on a limited master any
/// rate (even 0.99) leaves the RMS within 0.1 dB but raises peaks up to 6 dB over full scale, on
/// about 0.5% of samples. Run through the zero-attack overlap limiter, those peaks pulled A down
/// 2-3 dB at the fade (and a ride starts before the fade, unlimited, so they clipped). A soft
/// knee above -1.5 dBFS puts them back under the ceiling for 0.2 dB of level.
fn tame_peaks(samples: &mut [f32]) {
    const SPAN: f32 = PEAK_CEILING - PEAK_KNEE;
    for sample in samples {
        let magnitude = sample.abs();
        if magnitude > PEAK_KNEE {
            *sample =
                (PEAK_KNEE + SPAN * ((magnitude - PEAK_KNEE) / SPAN).tanh()).copysign(*sample);
        }
    }
}

pub(crate) struct AutoMixStretch {
    stretch: Stretch,
    channels: usize,
    /// The rate of the block just processed.
    pub(crate) rate: f64,
    target: f64,
    /// Source frames over which the rate eases from 1.0 to `target`; zero holds `target` throughout.
    ramp_frames: f64,
    emitted_frames: u64,
    priming: Vec<f32>,
    primed: bool,
    preroll: usize,
    scratch: Vec<f32>,
    zeros: Vec<f32>,
    tail: Vec<f32>,
    frame_fraction: f64,
    source_fraction: f64,
    pending_source: u64,
}

impl AutoMixStretch {
    /// Called on the prepare thread, never by the mixer or device callback.
    pub(crate) fn new(channels: u16, sample_rate: u32, rate: f64) -> Self {
        assert!(RATE_RANGE.contains(&rate));
        let stretch = Stretch::preset_default(u32::from(channels), sample_rate);
        let channels = channels as usize;
        let input_latency = stretch.input_latency();
        let output_latency = stretch.output_latency();
        // Sized for the slowest rate it passes through, so a ramp never reallocates.
        let capacity =
            ((input_latency + 512) as f64 / rate.min(1.0)).ceil() as usize + output_latency;
        Self {
            stretch,
            channels,
            rate,
            target: rate,
            ramp_frames: 0.0,
            emitted_frames: 0,
            priming: Vec::with_capacity((input_latency + 512) * channels),
            primed: false,
            preroll: output_latency,
            scratch: Vec::with_capacity(capacity * channels),
            zeros: vec![0.0; input_latency * channels],
            tail: Vec::with_capacity(capacity * channels),
            frame_fraction: 0.0,
            source_fraction: 0.0,
            pending_source: 0,
        }
    }

    /// Starts at the song's own speed and eases linearly, by source position, to the target rate
    /// over `frames` source frames. One multiply per block: the stretch takes its ratio from each
    /// call's input and output lengths, so a changing rate costs nothing more.
    pub(crate) fn ramping(mut self, frames: usize) -> Self {
        if frames > 0 {
            self.rate = 1.0;
            self.ramp_frames = frames as f64;
        }
        self
    }

    pub(crate) fn output_capacity(&self, drain: bool) -> usize {
        if drain {
            self.scratch.capacity() + self.tail.capacity()
        } else {
            ((512.0 / self.rate.min(self.target)).ceil() as usize + 1) * self.channels
        }
    }

    pub(crate) fn process(&mut self, input: &[f32], output: &mut Vec<f32>) -> u64 {
        if self.ramp_frames > 0.0 {
            let progress = (self.emitted_frames as f64 / self.ramp_frames).min(1.0);
            self.rate = 1.0 + (self.target - 1.0) * progress;
        }
        self.pending_source += input.len() as u64;
        let input = if self.primed {
            input
        } else {
            self.priming.extend_from_slice(input);
            let latency = self.stretch.input_latency() * self.channels;
            if self.priming.len() < latency {
                return 0;
            }
            self.stretch.seek(&self.priming[..latency], self.rate);
            self.primed = true;
            &self.priming[latency..]
        };
        let exact = input.len() as f64 / self.channels as f64 / self.rate + self.frame_fraction;
        let frames = exact.floor() as usize;
        self.frame_fraction = exact - frames as f64;
        self.scratch.resize(frames * self.channels, 0.0);
        if frames > 0 {
            self.stretch.process(input, &mut self.scratch);
        }
        self.priming.clear();
        let discard = self.preroll.min(frames);
        self.preroll -= discard;
        tame_peaks(&mut self.scratch[discard * self.channels..]);
        output.extend_from_slice(&self.scratch[discard * self.channels..]);
        let source_frames = (frames - discard) as f64 * self.rate + self.source_fraction;
        let consumed =
            (source_frames.floor() as u64 * self.channels as u64).min(self.pending_source);
        self.source_fraction = source_frames - source_frames.floor();
        self.pending_source -= consumed;
        self.emitted_frames += consumed / self.channels as u64;
        consumed
    }

    pub(crate) fn flush(&mut self, output: &mut Vec<f32>) -> u64 {
        let pending = self.pending_source;
        if !self.primed {
            output.extend_from_slice(&self.priming);
            self.priming.clear();
            self.pending_source = 0;
            return pending;
        }
        if pending == 0 {
            return 0;
        }
        let frames = (self.stretch.input_latency() as f64 / self.rate).ceil() as usize;
        self.scratch.resize(frames * self.channels, 0.0);
        self.stretch.process(&self.zeros, &mut self.scratch);
        self.tail.clear();
        self.tail.extend_from_slice(&self.scratch);
        let offset = self.tail.len();
        self.tail
            .resize(offset + self.stretch.output_latency() * self.channels, 0.0);
        self.stretch.flush(&mut self.tail[offset..]);
        let discard = self.preroll.min(self.tail.len() / self.channels) * self.channels;
        let wanted =
            (pending as f64 / self.channels as f64 / self.rate).round() as usize * self.channels;
        let end = (discard + wanted).min(self.tail.len());
        tame_peaks(&mut self.tail[discard..end]);
        output.extend_from_slice(&self.tail[discard..end]);
        self.pending_source = 0;
        pending
    }
}

#[cfg(test)]
mod stream_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "manual release benchmark: cargo test --release -p gmplayer-audio-backend benchmark_automix_tempo -- --ignored --nocapture"]
    fn benchmark_automix_tempo() {
        let input: Vec<f32> = (0..48_000 * 30 * 2)
            .map(|i| {
                let t = (i / 2) as f32 / 48_000.0;
                ((t * std::f32::consts::TAU * (440.0 + (i % 2) as f32 * 90.0)).sin() * 0.2
                    + (t * std::f32::consts::TAU * 1900.0).sin() * 0.1)
                    * (0.6 + 0.4 * (t * 2.0).fract())
            })
            .collect();
        for (rate, ramp) in [(0.97, 0), (1.03, 0), (0.93, 48_000 * 4)] {
            let mut processor = AutoMixStretch::new(2, 48_000, rate).ramping(ramp);
            let mut output = Vec::with_capacity((input.len() as f64 / rate).ceil() as usize + 4096);
            let begin = std::time::Instant::now();
            let mut source = 0;
            for block in input.chunks(1024) {
                source += processor.process(block, &mut output);
            }
            source += processor.flush(&mut output);
            let seconds = begin.elapsed().as_secs_f64();
            assert_eq!(source, input.len() as u64);
            println!(
                "rate={rate}, ramp={ramp}, process_ms={:.2}, realtime_x={:.1}",
                seconds * 1000.0,
                30.0 / seconds
            );
            std::hint::black_box(output);
        }
    }

    #[test]
    fn a_ramp_eases_into_its_rate_and_keeps_every_source_sample() {
        let (sample_rate, rate) = (48_000, 0.93);
        let input: Vec<f32> = (0..sample_rate * 6)
            .map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / sample_rate as f32).sin() * 0.5)
            .collect();
        let ramp = sample_rate * 2;
        let mut processor = AutoMixStretch::new(1, sample_rate as u32, rate).ramping(ramp);
        let mut output = Vec::with_capacity(input.len() * 2);
        let mut consumed = 0;
        for block in input.chunks(512) {
            consumed += processor.process(block, &mut output);
        }
        consumed += processor.flush(&mut output);
        assert_eq!(consumed, input.len() as u64);
        // Linear in source position: ln(rate) / (rate - 1) per ramped frame, 1 / rate after.
        let expected = ramp as f64 * rate.ln() / (rate - 1.0) + (input.len() - ramp) as f64 / rate;
        assert!(
            (output.len() as f64 - expected).abs() < 600.0,
            "{} vs {expected}",
            output.len()
        );
        let held = &output[output.len() - 72_000..output.len() - 24_000];
        let crossings = held
            .windows(2)
            .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
            .count();
        assert!(
            (crossings as f64 - 1000.0).abs() < 2.0,
            "pitch moved: {crossings} Hz"
        );
    }

    #[test]
    fn stretched_peaks_stay_under_the_ceiling_and_quiet_samples_pass_untouched() {
        let mut samples = vec![0.5, -0.84, 0.9, -1.0, 2.0, -4.0];
        tame_peaks(&mut samples);
        assert_eq!(&samples[..2], &[0.5, -0.84]);
        assert!(samples[2..]
            .iter()
            .all(|s| s.abs() > PEAK_KNEE && s.abs() <= PEAK_CEILING));
        assert!(samples[2] < samples[3].abs() && samples[3].abs() < samples[4]);
        assert!(samples[3] < 0.0 && samples[5] < 0.0);
    }

    #[test]
    fn pitch_is_preserved_when_tempo_changes() {
        let input: Vec<f32> = (0..96_000)
            .map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / 48_000.0).sin() * 0.5)
            .collect();
        let mut processor = AutoMixStretch::new(1, 48_000, 1.04);
        let mut output = Vec::with_capacity(input.len());
        for block in input.chunks(512) {
            processor.process(block, &mut output);
        }
        processor.flush(&mut output);
        let middle = &output[24_000..72_000];
        let crossings = middle
            .windows(2)
            .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
            .count();
        assert!(
            (crossings as f64 - 1000.0).abs() < 2.0,
            "pitch moved: {crossings} Hz"
        );
    }

    #[test]
    fn stretching_preserves_pitch_pulses_and_all_source_samples() {
        let sample_rate = 48_000;
        for rate in [0.97, 1.04] {
            let mut processor = AutoMixStretch::new(1, sample_rate, rate);
            let input: Vec<f32> = (0..sample_rate * 4)
                .map(|i| {
                    if (i + sample_rate / 10) % (sample_rate / 2) < sample_rate / 50 {
                        (std::f32::consts::TAU * i as f32 * 1000.0 / sample_rate as f32).sin() * 0.5
                    } else {
                        0.0
                    }
                })
                .collect();
            let mut output = Vec::with_capacity(input.len() * 2);
            let mut consumed = 0;
            for block in input.chunks(512) {
                consumed += processor.process(block, &mut output);
            }
            consumed += processor.flush(&mut output);
            assert_eq!(consumed, input.len() as u64);
            assert!((output.len() as f64 - input.len() as f64 / rate).abs() <= 2.0);
            for beat in 1..7 {
                let expected =
                    ((beat as f64 * 0.5 - 0.1 + 0.01) * sample_rate as f64 / rate) as usize;
                let energy: f32 = output[expected - 100..expected + 100]
                    .iter()
                    .map(|s| s * s)
                    .sum();
                assert!(energy > 1.0, "pulse shifted: {beat}, {rate}");
            }
        }
    }
}
