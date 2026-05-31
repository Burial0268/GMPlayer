use rustfft::{num_complex::Complex, FftPlanner};
use std::sync::Arc;

use crate::types::SpectrumConfig;

pub struct SpectrumAnalyzer {
    config: SpectrumConfig,
    fft: Arc<dyn rustfft::Fft<f32>>,
    last_magnitudes: Vec<f32>,
}

impl SpectrumAnalyzer {
    pub fn new(config: SpectrumConfig) -> Self {
        let fft_size = config.fft_size.next_power_of_two();
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(fft_size);

        let num_bins = fft_size / 2;

        SpectrumAnalyzer {
            config: SpectrumConfig { fft_size, ..config },
            fft,
            // `analyze()` slices `buffer[1..num_bins]` (skipping DC), so the
            // produced magnitudes vec has `num_bins - 1` elements.
            last_magnitudes: vec![0.0; num_bins.saturating_sub(1)],
        }
    }

    pub fn fft_size(&self) -> usize {
        self.config.fft_size
    }

    /// Analyze multichannel interleaved samples and return the magnitude spectrum.
    /// Returns raw, unwindowed magnitude bins, excluding DC.
    pub fn analyze(&mut self, samples: &[f32], channels: u16) -> Vec<f32> {
        let fft_size = self.config.fft_size;

        if samples.is_empty() {
            return self.last_magnitudes.clone();
        }

        let mono: Vec<f32> = if channels > 1 {
            samples
                .chunks(channels as usize)
                .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
                .collect()
        } else {
            samples.to_vec()
        };

        if mono.len() < fft_size {
            return self.last_magnitudes.clone();
        }

        let offset = mono.len().saturating_sub(fft_size);
        let mut buffer: Vec<Complex<f32>> = mono[offset..]
            .iter()
            .map(|&s| Complex { re: s, im: 0.0 })
            .collect();

        self.fft.process(&mut buffer);

        let num_bins = fft_size / 2;
        let mut magnitudes: Vec<f32> = buffer[1..num_bins]
            .iter()
            .map(|c| (c.re * c.re + c.im * c.im).sqrt())
            .collect();

        let scale = 1.0 / fft_size as f32;
        for m in magnitudes.iter_mut() {
            *m *= scale;
        }

        self.last_magnitudes.copy_from_slice(&magnitudes);

        magnitudes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn analyze_returns_unsmoothed_raw_frame() {
        let mut analyzer = SpectrumAnalyzer::new(SpectrumConfig {
            fft_size: 2048,
            smoothing: 0.99,
            max_freq: None,
        });

        let rate = 44_100.0;
        let tone: Vec<f32> = (0..analyzer.fft_size())
            .map(|i| (TAU * 440.0 * i as f32 / rate).sin())
            .collect();

        let tone_peak = analyzer
            .analyze(&tone, 1)
            .into_iter()
            .fold(0.0f32, f32::max);
        assert!(tone_peak > 0.0);

        let silence = vec![0.0f32; analyzer.fft_size()];
        let silence_peak = analyzer
            .analyze(&silence, 1)
            .into_iter()
            .fold(0.0f32, f32::max);

        assert!(
      silence_peak < tone_peak * 0.001,
      "spectrum should not retain smoothed tone energy, tone={tone_peak} silence={silence_peak}"
    );
    }
}
