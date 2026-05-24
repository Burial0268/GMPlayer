use rustfft::{num_complex::Complex, FftPlanner};
use std::sync::Arc;

use crate::types::SpectrumConfig;

pub struct SpectrumAnalyzer {
  config: SpectrumConfig,
  fft: Arc<dyn rustfft::Fft<f32>>,
  window: Vec<f32>,
  smoothed_magnitudes: Vec<f32>,
}

impl SpectrumAnalyzer {
  pub fn new(config: SpectrumConfig) -> Self {
    let fft_size = config.fft_size.next_power_of_two();
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);

    let window = hann_window(fft_size);
    let num_bins = fft_size / 2;

    SpectrumAnalyzer {
      config: SpectrumConfig { fft_size, ..config },
      fft,
      window,
      // `analyze()` slices `buffer[1..num_bins]` (skipping DC), so the
      // produced magnitudes vec has `num_bins - 1` elements. The smoothing
      // buffer must match — otherwise `copy_from_slice` in the no-smoothing
      // branch panics with mismatched lengths.
      smoothed_magnitudes: vec![0.0; num_bins.saturating_sub(1)],
    }
  }

  pub fn fft_size(&self) -> usize {
    self.config.fft_size
  }

  /// Analyze multichannel interleaved samples and return the magnitude spectrum.
  /// Returns a vector of `fft_size / 2` magnitude bins.
  pub fn analyze(&mut self, samples: &[f32], channels: u16) -> Vec<f32> {
    let fft_size = self.config.fft_size;

    if samples.is_empty() {
      return self.smoothed_magnitudes.clone();
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
      return self.smoothed_magnitudes.clone();
    }

    let offset = mono.len().saturating_sub(fft_size);
    let mut buffer: Vec<Complex<f32>> = mono[offset..]
      .iter()
      .enumerate()
      .map(|(i, &s)| Complex {
        re: s * self.window[i],
        im: 0.0,
      })
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

    let smoothing = self.config.smoothing.clamp(0.0, 0.99);
    if smoothing > 0.0 {
      let alpha = 1.0 - smoothing;
      for (i, m) in magnitudes.iter_mut().enumerate() {
        self.smoothed_magnitudes[i] =
          alpha * *m + smoothing * self.smoothed_magnitudes[i];
        *m = self.smoothed_magnitudes[i];
      }
    } else {
      self.smoothed_magnitudes.copy_from_slice(&magnitudes);
    }

    magnitudes
  }
}

fn hann_window(size: usize) -> Vec<f32> {
  let n = size as f64;
  (0..size)
    .map(|i| {
      (0.5 * (1.0 - (2.0 * std::f64::consts::PI * i as f64 / (n - 1.0)).cos())) as f32
    })
    .collect()
}
