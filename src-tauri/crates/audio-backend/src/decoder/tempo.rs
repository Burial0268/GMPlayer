//! Finite AutoMix stretching, adapted from SPlayer-Next PR #429.
//! Real look-ahead primes the algorithm; synthetic drain samples never advance source time.

use signalsmith_stretch::Stretch;

pub(crate) struct AutoMixStretch {
    stretch: Stretch,
    channels: usize,
    pub(crate) rate: f64,
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
        assert!((0.96..=1.04).contains(&rate));
        let stretch = Stretch::preset_default(u32::from(channels), sample_rate);
        let channels = channels as usize;
        let input_latency = stretch.input_latency();
        let output_latency = stretch.output_latency();
        let capacity = ((input_latency + 512) as f64 / rate).ceil() as usize + output_latency;
        Self {
            stretch,
            channels,
            rate,
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

    pub(crate) fn output_capacity(&self, drain: bool) -> usize {
        if drain {
            self.scratch.capacity() + self.tail.capacity()
        } else {
            ((512.0 / self.rate).ceil() as usize + 1) * self.channels
        }
    }

    pub(crate) fn process(&mut self, input: &[f32], output: &mut Vec<f32>) -> u64 {
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
        output.extend_from_slice(&self.scratch[discard * self.channels..]);
        let source_frames = (frames - discard) as f64 * self.rate + self.source_fraction;
        let consumed =
            (source_frames.floor() as u64 * self.channels as u64).min(self.pending_source);
        self.source_fraction = source_frames - source_frames.floor();
        self.pending_source -= consumed;
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
        output.extend_from_slice(&self.tail[discard..(discard + wanted).min(self.tail.len())]);
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
        for rate in [0.97, 1.03] {
            let mut processor = AutoMixStretch::new(2, 48_000, rate);
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
                "rate={rate}, process_ms={:.2}, realtime_x={:.1}",
                seconds * 1000.0,
                30.0 / seconds
            );
            std::hint::black_box(output);
        }
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
