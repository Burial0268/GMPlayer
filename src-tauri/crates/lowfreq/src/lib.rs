/// Core audio processing: FFT + dynamic peak normalization + low-frequency volume analysis.
///
/// Two independent components matching the TypeScript reference implementations:
/// - `FftProcessor`: mirrors `WasmFFTManager` (FFT, normalization, raw bins)
/// - `LowFreqAnalyzer`: mirrors `LowFreqVolumeAnalyzer` (gradient, smoothing)
///
/// ## Pipeline
///
/// ```text
/// PCM → push_pcm() → [ring buffer] → read_spectrum() → Hamming FFT → mirror(2048)
///   → normalize(sqrt(mag) * 255/sqrt(peak)) → EMA(α=0.5) → spectrum (0-255)
///   → get_raw_bins(count) → group 2048 into count averages
///   → LowFreqAnalyzer.analyze(raw_bins, delta) → amplitudeToLevel → gradient → smoothed lowFreq (0-1)
/// ```

use std::collections::VecDeque;

use microfft::real::rfft_2048;

// ── Constants ──────────────────────────────────────────────────────

const FFT_SIZE: usize = 2048;
const NYQUIST_BIN: usize = FFT_SIZE / 2; // 1024
const MIRRORED_SIZE: usize = FFT_SIZE; // 2048

// ── FftConfig ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FftConfig {
    /// Output spectrum size (default: 2048). When ≠2048, linear interpolation is used.
    pub output_size: usize,
}

impl Default for FftConfig {
    fn default() -> Self {
        Self { output_size: 2048 }
    }
}

// ── FftProcessor ───────────────────────────────────────────────────

/// Hamming-windowed real FFT with dynamic peak normalization.
/// Mirrors `WasmFFTManager` — PCM in, normalized spectrum (0-255) + raw bins out.
pub struct FftProcessor {
    config: FftConfig,

    /// PCM ring buffer (capped at FFT_SIZE * 8)
    pcm: VecDeque<f32>,

    /// Pre-computed Hamming window coefficients
    hamming: Vec<f32>,

    /// Mirrored raw FFT magnitudes (2048 elements, matching FFTPlayer output)
    raw_magnitudes: Vec<f32>,

    /// Normalized output spectrum (0-255)
    spectrum: Vec<f32>,

    /// EMA-smoothed spectrum (α=0.5)
    smoothed: Vec<f32>,

    /// Dynamic peak for normalization
    peak_value: f32,

    /// Cached raw bin groups
    raw_bins: Vec<f32>,
    raw_bins_dirty: bool,
}

impl FftProcessor {
    pub fn new(config: FftConfig) -> Self {
        let out = config.output_size.max(1);

        let hamming: Vec<f32> = (0..FFT_SIZE)
            .map(|n| {
                0.54 - 0.46 * (2.0 * std::f32::consts::PI * n as f32 / (FFT_SIZE - 1) as f32).cos()
            })
            .collect();

        Self {
            config,
            pcm: VecDeque::with_capacity(FFT_SIZE * 8),
            hamming,
            raw_magnitudes: vec![0.0; MIRRORED_SIZE],
            spectrum: vec![0.0; out],
            smoothed: vec![0.0; out],
            peak_value: 0.0001,
            raw_bins: vec![0.0; 2],
            raw_bins_dirty: true,
        }
    }

    /// Push mono PCM samples into the ring buffer.
    /// Capped at FFT_SIZE * 8 to prevent unbounded growth.
    pub fn push_pcm(&mut self, samples: &[f32]) {
        self.pcm.extend(samples);
        while self.pcm.len() > FFT_SIZE * 8 {
            self.pcm.pop_front();
        }
    }

    /// Run FFT (if ≥2048 PCM samples queued), normalize, smooth.
    /// Returns reference to the normalized spectrum (0-255).
    pub fn read_spectrum(&mut self) -> &[f32] {
        if self.pcm.len() < FFT_SIZE {
            return &self.spectrum;
        }

        // Build Hamming-windowed input
        let mut fft_buf = [0.0f32; FFT_SIZE];
        for (i, &sample) in self.pcm.iter().take(FFT_SIZE).enumerate() {
            fft_buf[i] = sample * self.hamming[i];
        }

        // Drain exactly FFT_SIZE consumed samples
        for _ in 0..FFT_SIZE {
            self.pcm.pop_front();
        }

        // Real FFT (in-place, microfft)
        let spectrum = rfft_2048(&mut fft_buf);
        let inv_sqrt_n = 1.0 / (FFT_SIZE as f32).sqrt();

        // Compute mirrored magnitudes (2048 elements, matching FFTPlayer output format)
        // DC bin
        self.raw_magnitudes[0] = spectrum[0].re.abs() * inv_sqrt_n;
        // Bins 1..1023
        for k in 1..NYQUIST_BIN {
            let c = &spectrum[k];
            self.raw_magnitudes[k] = (c.re * c.re + c.im * c.im).sqrt() * inv_sqrt_n;
        }
        // Nyquist bin (packed in spectrum[0].im)
        self.raw_magnitudes[NYQUIST_BIN] = spectrum[0].im.abs() * inv_sqrt_n;
        // Mirror: [1025..2047] = [1023..1]
        for k in 1..NYQUIST_BIN {
            self.raw_magnitudes[FFT_SIZE - k] = self.raw_magnitudes[k];
        }

        self._normalize_and_smooth();
        self.raw_bins_dirty = true;

        &self.spectrum
    }

    /// Normalize raw magnitudes → 0-255 using dynamic peak, then EMA smooth (α=0.5).
    /// Peak is tracked AFTER normalization (previous frame's peak normalizes current frame).
    fn _normalize_and_smooth(&mut self) {
        let out_size = self.config.output_size;
        let inv_peak = 255.0 / self.peak_value.sqrt();

        // Scan for frame peak over RAW magnitudes (matching TS: peak scanned from rawBuf)
        let mut frame_peak = 0.0f32;
        for &mag in &self.raw_magnitudes {
            if mag > frame_peak {
                frame_peak = mag;
            }
        }

        if out_size == MIRRORED_SIZE {
            // Fast path: 1:1 mapping
            for i in 0..out_size {
                let mag = self.raw_magnitudes[i];
                let norm = if mag > 0.0 { mag.sqrt() * inv_peak } else { 0.0 };
                self.smoothed[i] = self.smoothed[i] * 0.5 + norm.min(255.0) * 0.5;
                self.spectrum[i] = self.smoothed[i];
            }
        } else {
            // Interpolation path
            let ratio = (MIRRORED_SIZE - 1) as f32 / (out_size - 1) as f32;
            for i in 0..out_size {
                let src_idx = i as f32 * ratio;
                let lo = src_idx as usize;
                let frac = src_idx - lo as f32;
                let hi = if lo + 1 < MIRRORED_SIZE { lo + 1 } else { lo };
                let mag = self.raw_magnitudes[lo]
                    + (self.raw_magnitudes[hi] - self.raw_magnitudes[lo]) * frac;
                let norm = if mag > 0.0 { mag.sqrt() * inv_peak } else { 0.0 };
                self.smoothed[i] = self.smoothed[i] * 0.5 + norm.min(255.0) * 0.5;
                self.spectrum[i] = self.smoothed[i];
            }
        }

        // Asymmetric peak tracking: fast attack (0.5 blend), slow release (0.995 decay)
        if frame_peak > self.peak_value {
            self.peak_value = self.peak_value * 0.5 + frame_peak * 0.5;
        } else {
            self.peak_value *= 0.995;
        }
        if self.peak_value < 0.0001 {
            self.peak_value = 0.0001;
        }
    }

    /// Get raw FFT magnitudes aggregated into `count` groups (128-group AMLL resolution).
    /// Groups of 16 consecutive mirrored bins are averaged.
    /// Results are cached per frame (dirtied by `read_spectrum`).
    pub fn get_raw_bins(&mut self, count: usize) -> Option<&[f32]> {
        if !self.raw_bins_dirty && count == self.raw_bins.len() {
            return Some(&self.raw_bins);
        }

        if count != self.raw_bins.len() {
            self.raw_bins = vec![0.0; count];
        }

        // Group size: 2048 / 128 = 16 bins per group
        let group_size = MIRRORED_SIZE >> 7; // 16
        for i in 0..count {
            let start = i * group_size;
            let end = start + group_size;
            self.raw_bins[i] =
                self.raw_magnitudes[start..end].iter().sum::<f32>() / group_size as f32;
        }

        self.raw_bins_dirty = false;
        Some(&self.raw_bins)
    }

    /// Current normalized spectrum (0-255).
    pub fn spectrum(&self) -> &[f32] {
        &self.spectrum
    }

    /// Always true — no WASM initialization failure possible with native microfft.
    pub fn is_ready(&self) -> bool {
        true
    }

    /// Change output spectrum size at runtime.
    pub fn set_output_size(&mut self, size: usize) {
        let size = size.max(1);
        self.config.output_size = size;
        self.spectrum = vec![0.0; size];
        self.smoothed = vec![0.0; size];
    }

    /// Reset normalization and smoothing state (e.g. on track change).
    /// Does NOT clear the PCM queue.
    pub fn reset(&mut self) {
        self.peak_value = 0.0001;
        self.raw_bins_dirty = true;
        self.raw_magnitudes.fill(0.0);
        self.smoothed.fill(0.0);
        self.spectrum.fill(0.0);
    }

    /// Drain all queued PCM by running repeated FFT cycles (discarding results),
    /// then zero all buffers. Used on seek to flush stale data.
    pub fn clear_queue(&mut self) {
        // Drain PCM by consuming FFT_SIZE chunks (matching TS: loop-read until empty)
        let mut fft_buf = [0.0f32; FFT_SIZE];
        let mut safety: i32 = 100;
        while self.pcm.len() >= FFT_SIZE && safety > 0 {
            safety -= 1;
            for (i, &sample) in self.pcm.iter().take(FFT_SIZE).enumerate() {
                fft_buf[i] = sample * self.hamming[i];
            }
            for _ in 0..FFT_SIZE {
                self.pcm.pop_front();
            }
            let _ = rfft_2048(&mut fft_buf);
        }
        // Drop any remaining partial chunk
        self.pcm.clear();
        self.reset();
    }

    /// Release all heap allocations (equivalent to JS `free()`).
    pub fn free(&mut self) {
        self.pcm.clear();
        // Shrink to release capacity
        self.pcm.shrink_to_fit();
        self.raw_magnitudes.clear();
        self.raw_magnitudes.shrink_to_fit();
        self.spectrum.clear();
        self.spectrum.shrink_to_fit();
        self.smoothed.clear();
        self.smoothed.shrink_to_fit();
        self.raw_bins.clear();
        self.raw_bins.shrink_to_fit();
    }
}

// ── LowFreqConfig ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LowFreqConfig {
    /// Number of raw FFT bins to average for low-freq detection (default: 2)
    pub bin_count: usize,
    /// Sliding window size for gradient calculation (default: 10)
    pub window_size: usize,
    /// Gradient threshold — above this, use max² (default: 0.35)
    pub gradient_threshold: f32,
    /// Time-delta smoothing factor (default: 0.003)
    pub smoothing_factor: f32,
}

impl Default for LowFreqConfig {
    fn default() -> Self {
        Self {
            bin_count: 2,
            window_size: 10,
            gradient_threshold: 0.35,
            smoothing_factor: 0.003,
        }
    }
}

/// Partial update for LowFreqAnalyzer options.
/// All fields are optional — only `Some` values are applied.
#[derive(Debug, Clone, Default)]
pub struct LowFreqOptions {
    pub bin_count: Option<usize>,
    pub window_size: Option<usize>,
    pub gradient_threshold: Option<f32>,
    pub smoothing_factor: Option<f32>,
}

// ── LowFreqAnalyzer ────────────────────────────────────────────────

/// Low-frequency volume analyzer for background animation effects.
/// Matches AMLL FFTToLowPassContext algorithm (commit 48fb050d).
///
/// Input: raw FFTPlayer magnitudes (NOT 0-255 normalized).
/// Output: smoothed low-frequency volume (0-1 range).
pub struct LowFreqAnalyzer {
    config: LowFreqConfig,

    /// Chronological sliding window (Vec with remove(0)/push, matching TS shift+push)
    gradient: Vec<f32>,

    /// Smoothed output state
    cur_value: f32,
}

impl LowFreqAnalyzer {
    pub fn new(config: LowFreqConfig) -> Self {
        Self {
            gradient: Vec::with_capacity(config.window_size),
            cur_value: 1.0,
            config,
        }
    }

    /// Convert raw FFT amplitude to log10 level.
    /// Matches TS: `0.5 * Math.log10(amplitude / 255 + 1)`
    fn _amplitude_to_level(&self, amplitude: f32) -> f32 {
        let normalized = amplitude / 255.0;
        0.5 * (normalized + 1.0).log10()
    }

    /// Calculate gradient from sliding window of low-freq bin levels.
    /// Uses first `binCount` bins averaged, tracks window of `windowSize` values.
    /// Returns either max² (if diff > threshold) or min * 0.25.
    fn _calculate_gradient(&mut self, fft_data: &[f32]) -> f32 {
        let count = self.config.bin_count.min(fft_data.len());
        let mut sum = 0.0f32;
        for i in 0..count {
            sum += self._amplitude_to_level(fft_data[i]);
        }
        let volume = sum / count as f32;

        if self.gradient.len() < self.config.window_size {
            // TS: only push if not a duplicate
            if !self.gradient.iter().any(|&v| v == volume) {
                self.gradient.push(volume);
            }
            return 0.0;
        }

        // Chronological sliding window: shift + push (matching TS Array.shift + push)
        self.gradient.remove(0);
        self.gradient.push(volume);

        let mut gmax = f32::NEG_INFINITY;
        let mut gmin = f32::INFINITY;
        for &v in &self.gradient {
            if v > gmax {
                gmax = v;
            }
            if v < gmin {
                gmin = v;
            }
        }
        let max_sq = gmax * gmax;
        let diff = max_sq - gmin;

        if diff > self.config.gradient_threshold {
            max_sq
        } else {
            gmin * 0.25
        }
    }

    /// Analyze FFT raw bin data and return smoothed low-frequency volume.
    ///
    /// `fft_data`: raw FFT magnitude bins (from `FftProcessor::get_raw_bins`).
    /// `delta_ms`: milliseconds since last frame (from `performance.now()` difference).
    pub fn analyze(&mut self, fft_data: &[f32], delta_ms: f32) -> f32 {
        if fft_data.len() < self.config.bin_count {
            return self.cur_value;
        }

        let delta = delta_ms.clamp(1.0, 100.0);
        let value = self._calculate_gradient(fft_data);

        // Time-delta based smoothing (matching TS onFrame)
        let step = (value - self.cur_value) * self.config.smoothing_factor * delta;
        if self.cur_value < value {
            self.cur_value = value.min(self.cur_value + step);
        } else {
            self.cur_value = value.max(self.cur_value + step);
        }

        if self.cur_value.is_nan() {
            self.cur_value = 1.0;
        }

        self.cur_value
    }

    /// Current smoothed low-freq volume.
    pub fn value(&self) -> f32 {
        self.cur_value
    }

    /// Update analysis parameters at runtime.
    /// Resets gradient window if `window_size` changes.
    pub fn set_options(&mut self, options: &LowFreqOptions) {
        if let Some(bc) = options.bin_count {
            self.config.bin_count = bc.max(1);
        }
        if let Some(ws) = options.window_size {
            let ws = ws.max(2);
            if ws != self.config.window_size {
                self.gradient.clear();
                self.gradient.reserve(ws);
                self.config.window_size = ws;
            }
        }
        if let Some(gt) = options.gradient_threshold {
            self.config.gradient_threshold = gt;
        }
        if let Some(sf) = options.smoothing_factor {
            self.config.smoothing_factor = sf;
        }
    }

    /// Get current configuration.
    pub fn config(&self) -> &LowFreqConfig {
        &self.config
    }

    /// Reset analyzer: clear gradient window, set cur_value to 0.
    /// Matches TS reset() which sets `_curValue = 0` (not 1).
    pub fn reset(&mut self) {
        self.gradient.clear();
        self.cur_value = 0.0;
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_amplitude_to_level() {
        let analyzer = LowFreqAnalyzer::new(LowFreqConfig::default());
        // amplitudeToLevel(amp) = 0.5 * log10(amp/255 + 1)
        // With amp=0: 0.5 * log10(1) = 0
        assert!((analyzer._amplitude_to_level(0.0)).abs() < 1e-6);
        // With amp=255: 0.5 * log10(2) ≈ 0.1505
        let level = analyzer._amplitude_to_level(255.0);
        assert!((level - 0.1505).abs() < 0.001);
    }

    #[test]
    fn test_fft_processor_initial_state() {
        let proc = FftProcessor::new(FftConfig::default());
        assert!(proc.is_ready());
        assert_eq!(proc.spectrum().len(), 2048);
        // Spectrum should be all zeros initially
        assert!(proc.spectrum().iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_fft_push_and_read_empty() {
        let mut proc = FftProcessor::new(FftConfig::default());
        // Not enough PCM — should return previous spectrum (zeros)
        let samples = vec![0.1f32; 1024];
        proc.push_pcm(&samples);
        let spec = proc.read_spectrum();
        assert!(spec.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_fft_push_and_read_full() {
        let mut proc = FftProcessor::new(FftConfig::default());
        // Push exactly FFT_SIZE samples (sine wave to get non-zero spectrum)
        let rate = 44100.0;
        let freq = 440.0;
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate).sin())
            .collect();
        proc.push_pcm(&samples);
        let spec = proc.read_spectrum().to_vec();
        // Should have non-zero spectrum after FFT
        assert!(spec.iter().any(|&v| v > 0.0));
        // PCM should be drained
        assert!(proc.pcm.is_empty());
    }

    #[test]
    fn test_raw_bins_caching() {
        let mut proc = FftProcessor::new(FftConfig::default());
        let rate = 44100.0;
        let freq = 440.0;
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate).sin())
            .collect();
        proc.push_pcm(&samples);
        proc.read_spectrum();

        let bins = proc.get_raw_bins(2).unwrap();
        assert_eq!(bins.len(), 2);
        // Raw bins from a sine wave should be non-zero
        assert!(bins.iter().any(|&v| v > 0.0));
    }

    #[test]
    fn test_lowfreq_analyzer_window_fill() {
        // Tests window-fill logic: curValue decays while window incomplete,
        // then rises once window is full and gradient triggers.
        // Uses large FFT magnitudes (>16000) so amplitudeToLevel > 0.9
        // and max_sq can overcome the decay from initial 1.0.
        let mut analyzer = LowFreqAnalyzer::new(LowFreqConfig {
            bin_count: 2,
            window_size: 3,
            gradient_threshold: 0.01,
            ..Default::default()
        });

        // Call 1: push value, curValue decays from 1.0
        let v1 = analyzer.analyze(&[50000.0, 25000.0], 16.0);
        assert!(v1 < 1.0, "decay from initial 1.0");

        // Call 2: duplicate → skipped, still decaying
        let v2 = analyzer.analyze(&[50000.0, 25000.0], 16.0);
        assert!(v2 < v1, "duplicate skipped, still decaying");

        // Call 3: very quiet → pushes to window (len=2), still decaying
        let v3 = analyzer.analyze(&[10.0, 5.0], 16.0);
        assert!(v3 < v2, "window has 2 entries, not yet full");

        // Call 4: fills window (len becomes 3), value=0 for this frame
        let v4 = analyzer.analyze(&[200.0, 100.0], 16.0);
        assert!(v4 < v3, "fills window but gradient computed before push");

        // Call 5: window already full, loud signal → max_sq ≈ 1.32 > curValue ≈ 0.82
        let v5 = analyzer.analyze(&[50000.0, 25000.0], 16.0);
        assert!(v5 > v4, "window full, loud bass → curValue should rise");
    }

    #[test]
    fn test_lowfreq_analyzer_reset() {
        let mut analyzer = LowFreqAnalyzer::new(LowFreqConfig::default());
        analyzer.analyze(&[100.0, 50.0], 16.0);
        analyzer.reset();
        assert_eq!(analyzer.value(), 0.0);
        assert!(analyzer.gradient.is_empty());
    }

    #[test]
    fn test_peak_tracking() {
        let mut proc = FftProcessor::new(FftConfig::default());
        // Peak starts at 0.0001
        assert!((proc.peak_value - 0.0001).abs() < 1e-8);

        // Push loud signal
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44100.0).sin())
            .collect();
        proc.push_pcm(&samples);
        proc.read_spectrum();

        // Peak should have increased
        assert!(proc.peak_value > 0.0001);
    }

    #[test]
    fn test_clear_queue() {
        let mut proc = FftProcessor::new(FftConfig::default());
        let samples: Vec<f32> = (0..FFT_SIZE * 3)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44100.0).sin())
            .collect();
        proc.push_pcm(&samples);
        proc.clear_queue();
        // After clear: PCM queue emptied, buffers zeroed
        assert!(proc.pcm.is_empty());
        assert!(proc.spectrum().iter().all(|&v| v == 0.0));
        assert!((proc.peak_value - 0.0001).abs() < 1e-8);
    }
}
