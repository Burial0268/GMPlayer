#[cfg(target_arch = "wasm32")]
mod wasm;

/// Core audio processing: AMLL FFTPlayer pipeline + dynamic peak normalization
/// + low-frequency volume analysis.
///
/// ## Pipeline (matching AMLL player-core fft_player.rs)
///
/// ```text
/// PCM → push_pcm() → [ring buffer] → read_spectrum() →
///   Hamming window → samples_fft_to_spectrum(divide_by_N_sqrt, Range(freq_min,freq_max))
///   → freq_val_exact × 2048 → EMA(α=0.5) → result_buf → peak normalization → spectrum (0-255)
///   → raw bins → LowFreqAnalyzer → lowFreq (0-1)
/// ```
///
/// Uses spectrum-analyzer (same crate as AMLL) for bit-exact Hamming window,
/// FFT scaling, and frequency interpolation.

use std::collections::VecDeque;

use spectrum_analyzer::{
    windows, samples_fft_to_spectrum, FrequencyLimit, scaling,
};

// ── Constants ──────────────────────────────────────────────────────

const FFT_SIZE: usize = 2048;
const RESULT_BUF_SIZE: usize = 2048;
const MAX_QUEUE_LEN: usize = FFT_SIZE * 4; // 8192

// ── FftConfig ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FftConfig {
    /// Output spectrum size. When ≠2048, vec_interp is used.
    pub output_size: usize,
    /// Minimum frequency for spectrum output (Hz). Default: 80.
    pub freq_min: f32,
    /// Maximum frequency for spectrum output (Hz). Default: 2000.
    pub freq_max: f32,
}

impl Default for FftConfig {
    fn default() -> Self {
        Self { output_size: 2048, freq_min: 80.0, freq_max: 2000.0 }
    }
}

// ── FftProcessor ───────────────────────────────────────────────────

/// Hamming-windowed FFT with divide_by_N_sqrt scaling, frequency-range filtering,
/// and dynamic peak normalization. Mirrors the AMLL player-core FFTPlayer pipeline.
pub struct FftProcessor {
    config: FftConfig,

    /// PCM ring buffer (capped at MAX_QUEUE_LEN)
    pcm_queue: VecDeque<f32>,

    /// Sample rate of incoming PCM (from AudioContext)
    sample_rate: u32,

    /// AMLL-style 2048-element freq-sampled buffer.
    /// Each element is freq_val_exact at a uniformly-spaced frequency
    /// in [freq_min, freq_max], EMA-smoothed (α=0.5).
    /// Pre-peak-normalization — used by get_raw_bins for lowFreq analysis.
    pub result_buf: [f32; RESULT_BUF_SIZE],

    /// Normalized output spectrum (0-255)
    spectrum: Vec<f32>,

    /// EMA-smoothed normalized output (α=0.5)
    smoothed: Vec<f32>,

    /// Dynamic peak for normalization (tracked from previous frame)
    pub peak_value: f32,

    /// Cached raw bin groups (from result_buf, pre-normalization)
    raw_bins: Vec<f32>,
    raw_bins_dirty: bool,
}

impl FftProcessor {
    pub fn new(config: FftConfig) -> Self {
        let out = config.output_size.max(1);
        Self {
            config,
            pcm_queue: VecDeque::with_capacity(MAX_QUEUE_LEN),
            sample_rate: 44100,
            result_buf: [0.0; RESULT_BUF_SIZE],
            spectrum: vec![0.0; out],
            smoothed: vec![0.0; out],
            peak_value: 0.0001,
            raw_bins: vec![0.0; 2],
            raw_bins_dirty: true,
        }
    }

    /// Push mono PCM samples into the ring buffer.
    pub fn push_pcm(&mut self, samples: &[f32], sample_rate: u32) {
        if sample_rate != self.sample_rate {
            self.sample_rate = sample_rate;
        }
        self.pcm_queue.extend(samples);
        while self.pcm_queue.len() > MAX_QUEUE_LEN {
            self.pcm_queue.pop_front();
        }
    }

    /// Run FFT (if ≥2048 PCM samples queued), freq-sample, normalize, smooth.
    /// `delta_ms` is wall-clock time since last call (from performance.now()).
    /// Returns reference to the normalized spectrum (0-255).
    pub fn read_spectrum(&mut self, delta_ms: f32) -> &[f32] {
        if self.pcm_queue.len() < FFT_SIZE {
            return &self.spectrum;
        }

        let (start_freq, end_freq) = (self.config.freq_min, self.config.freq_max);

        // Build input buffer (matching AMLL: take first 2048, don't pop yet)
        let mut fft_buf = [0.0f32; FFT_SIZE];
        for i in 0..FFT_SIZE {
            fft_buf[i] = self.pcm_queue[i];
        }

        // AMLL pipeline: Hamming window → FFT with divide_by_N_sqrt → freq range filter
        let fft_buf = windows::hamming_window(&fft_buf);

        match samples_fft_to_spectrum(
            &fft_buf,
            44100,
            FrequencyLimit::Range(start_freq, end_freq),
            Some(&scaling::divide_by_N_sqrt),
        ) {
            Ok(spec) => {
                let freq_min = spec.min_fr().val();
                let freq_max = spec.max_fr().val();
                let freq_range = freq_max - freq_min;
                for i in 0..RESULT_BUF_SIZE {
                    let freq =
                        i as f32 / RESULT_BUF_SIZE as f32 * freq_range + freq_min;
                    let freq = freq.clamp(freq_min, freq_max);
                    self.result_buf[i] += spec.freq_val_exact(freq).val();
                    self.result_buf[i] /= 2.0;
                }
            }
            Err(e) => {
                // FFT error — return previous spectrum, don't drain PCM
                eprintln!("FFT error: {:?}", e);
                return &self.spectrum;
            }
        }

        // Time-based drain (matching AMLL: cut_len = elapsed_sec * 44100)
        let drain_samples =
            ((delta_ms / 1000.0) * self.sample_rate as f32) as usize;
        for _ in 0..drain_samples.min(self.pcm_queue.len()) {
            self.pcm_queue.pop_front();
        }
        self.pcm_queue.truncate(MAX_QUEUE_LEN);

        self._normalize_and_smooth();
        self.raw_bins_dirty = true;

        &self.spectrum
    }

    /// Normalize result_buf → 0-255 using dynamic peak, then EMA smooth (α=0.5).
    fn _normalize_and_smooth(&mut self) {
        let out_size = self.config.output_size;
        let inv_peak = 255.0 / self.peak_value.sqrt();

        // Scan for frame peak over result_buf (pre-normalization magnitudes)
        let mut frame_peak = 0.0f32;
        for &mag in &self.result_buf {
            if mag > frame_peak {
                frame_peak = mag;
            }
        }

        if out_size == RESULT_BUF_SIZE {
            // Fast path: 1:1 mapping
            for i in 0..out_size {
                let mag = self.result_buf[i];
                let norm = if mag > 0.0 { mag.sqrt() * inv_peak } else { 0.0 };
                self.smoothed[i] =
                    self.smoothed[i] * 0.5 + norm.min(255.0) * 0.5;
                self.spectrum[i] = self.smoothed[i];
            }
        } else {
            // Interpolation path: vec_interp from 2048 → output_size
            // (matching AMLL vec_interp behavior)
            let src_len = RESULT_BUF_SIZE as f32;
            let dst_len = out_size as f32;
            let src_step = src_len / dst_len;
            let mut src_idx = 0.0f32;
            for i in 0..out_size {
                let src_idx_int = src_idx as usize;
                let src_idx_frac = src_idx - src_idx_int as f32;
                let next_int = if src_idx_int + 1 < RESULT_BUF_SIZE {
                    src_idx_int + 1
                } else {
                    src_idx_int
                };
                let mag = self.result_buf[src_idx_int]
                    + (self.result_buf[next_int] - self.result_buf[src_idx_int])
                        * src_idx_frac;
                let norm = if mag > 0.0 { mag.sqrt() * inv_peak } else { 0.0 };
                self.smoothed[i] =
                    self.smoothed[i] * 0.5 + norm.min(255.0) * 0.5;
                self.spectrum[i] = self.smoothed[i];
                src_idx += src_step;
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

    /// Get raw FFT magnitudes aggregated into `count` groups.
    /// Groups of 16 consecutive result_buf bins are averaged.
    /// Results are cached per frame (dirtied by `read_spectrum`).
    pub fn get_raw_bins(&mut self, count: usize) -> Option<&[f32]> {
        if !self.raw_bins_dirty && count == self.raw_bins.len() {
            return Some(&self.raw_bins);
        }

        if count != self.raw_bins.len() {
            self.raw_bins = vec![0.0; count];
        }

        let group_size = RESULT_BUF_SIZE >> 7; // 16
        for i in 0..count {
            let start = i * group_size;
            let end = start + group_size;
            self.raw_bins[i] = self.result_buf[start..end].iter().sum::<f32>()
                / group_size as f32;
        }

        self.raw_bins_dirty = false;
        Some(&self.raw_bins)
    }

    /// Current normalized spectrum (0-255).
    pub fn spectrum(&self) -> &[f32] {
        &self.spectrum
    }

    pub fn is_ready(&self) -> bool {
        true
    }

    pub fn set_freq_range(&mut self, min: f32, max: f32) {
        self.config.freq_min = min;
        self.config.freq_max = max;
    }

    pub fn set_output_size(&mut self, size: usize) {
        let size = size.max(1);
        self.config.output_size = size;
        self.spectrum = vec![0.0; size];
        self.smoothed = vec![0.0; size];
    }

    /// Reset normalization and smoothing (e.g. track change).
    pub fn reset(&mut self) {
        self.peak_value = 0.0001;
        self.raw_bins_dirty = true;
        self.result_buf.fill(0.0);
        self.smoothed.fill(0.0);
        self.spectrum.fill(0.0);
    }

    /// Drain all queued PCM + zero buffers (e.g. seek).
    pub fn clear_queue(&mut self) {
        self.reset();
        // Drain by running FFT cycles until queue is empty (matching AMLL)
        let mut fft_buf = [0.0f32; FFT_SIZE];
        let mut safety: i32 = 100;
        while self.pcm_queue.len() >= FFT_SIZE && safety > 0 {
            safety -= 1;
            for i in 0..FFT_SIZE {
                fft_buf[i] = self.pcm_queue[i];
            }
            for _ in 0..FFT_SIZE {
                self.pcm_queue.pop_front();
            }
            let fft_buf = windows::hamming_window(&fft_buf);
            if samples_fft_to_spectrum(
                &fft_buf,
                44100,
                FrequencyLimit::Range(self.config.freq_min, self.config.freq_max),
                Some(&scaling::divide_by_N_sqrt),
            )
            .is_err()
            {
                break;
            }
        }
        self.pcm_queue.clear();
        self.reset();
    }

    /// Release heap allocations.
    pub fn free(&mut self) {
        self.pcm_queue.clear();
        self.pcm_queue.shrink_to_fit();
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
    pub bin_count: usize,
    pub window_size: usize,
    pub gradient_threshold: f32,
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

#[derive(Debug, Clone, Default)]
pub struct LowFreqOptions {
    pub bin_count: Option<usize>,
    pub window_size: Option<usize>,
    pub gradient_threshold: Option<f32>,
    pub smoothing_factor: Option<f32>,
}

// ── LowFreqAnalyzer ────────────────────────────────────────────────

/// Low-frequency volume analyzer. Matches AMLL FFTToLowPassContext (commit 48fb050d).
pub struct LowFreqAnalyzer {
    config: LowFreqConfig,
    gradient: Vec<f32>,
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

    /// Calculate gradient from sliding window. Matches TS _calculateGradient exactly:
    /// - New value in partial window → push, return 0
    /// - Duplicate value in partial window → fall through to shift+push+compute
    /// - Window full → shift+push+compute
    /// Returns max² (if diff > threshold) or min * 0.25.
    fn _calculate_gradient(&mut self, fft_data: &[f32]) -> f32 {
        let count = self.config.bin_count.min(fft_data.len());
        let mut sum = 0.0f32;
        for i in 0..count {
            sum += self._amplitude_to_level(fft_data[i]);
        }
        let volume = sum / count as f32;

        if self.gradient.len() < self.config.window_size
            && !self.gradient.iter().any(|&v| v == volume)
        {
            self.gradient.push(volume);
            return 0.0;
        }

        // Fall-through: window full OR duplicate in partial window
        if self.gradient.len() >= self.config.window_size {
            self.gradient.remove(0);
        }
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

    pub fn value(&self) -> f32 {
        self.cur_value
    }

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

    pub fn config(&self) -> &LowFreqConfig {
        &self.config
    }

    pub fn reset(&mut self) {
        self.gradient.clear();
        self.cur_value = 0.0;
    }
}

// ── AudioProcessor ─────────────────────────────────────────────────

pub struct AudioProcessor {
    pub fft: FftProcessor,
    pub analyzer: LowFreqAnalyzer,
}

impl AudioProcessor {
    pub fn new(
        output_size: usize,
        freq_min: f32,
        freq_max: f32,
        bin_count: usize,
        window_size: usize,
        gradient_threshold: f32,
        smoothing_factor: f32,
    ) -> Self {
        Self {
            fft: FftProcessor::new(FftConfig {
                output_size: output_size.max(1),
                freq_min,
                freq_max,
            }),
            analyzer: LowFreqAnalyzer::new(LowFreqConfig {
                bin_count: bin_count.max(1),
                window_size: window_size.max(2),
                gradient_threshold,
                smoothing_factor,
            }),
        }
    }

    /// Run the full pipeline: FFT → normalize → raw bins → lowFreq analysis.
    pub fn process_frame(
        &mut self,
        delta_ms: f32,
        output_spectrum: &mut [f32],
    ) -> f32 {
        let spec = self.fft.read_spectrum(delta_ms);
        let len = output_spectrum.len().min(spec.len());
        output_spectrum[..len].copy_from_slice(&spec[..len]);

        let bin_count = self.analyzer.config().bin_count;
        if let Some(raw_bins) = self.fft.get_raw_bins(bin_count) {
            let bins: Vec<f32> = raw_bins.to_vec();
            self.analyzer.analyze(&bins, delta_ms)
        } else {
            self.analyzer.analyze(&[], delta_ms)
        }
    }

    pub fn push_pcm(&mut self, samples: &[f32], sample_rate: u32) {
        self.fft.push_pcm(samples, sample_rate);
    }

    pub fn reset(&mut self) {
        self.fft.reset();
        self.analyzer.reset();
    }

    pub fn clear(&mut self) {
        self.fft.clear_queue();
        self.analyzer.reset();
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn test_amplitude_to_level() {
        let analyzer = LowFreqAnalyzer::new(LowFreqConfig::default());
        assert!((analyzer._amplitude_to_level(0.0)).abs() < 1e-6);
        let level = analyzer._amplitude_to_level(255.0);
        assert!((level - 0.1505).abs() < 0.001);
    }

    #[test]
    fn test_amplitude_to_level_values() {
        let analyzer = LowFreqAnalyzer::new(LowFreqConfig::default());
        assert!((analyzer._amplitude_to_level(0.0)).abs() < 1e-6);
        assert!((analyzer._amplitude_to_level(255.0) - 0.1505).abs() < 0.001);
        assert!((analyzer._amplitude_to_level(2550.0) - 0.5207).abs() < 0.001);
        assert!((analyzer._amplitude_to_level(25500.0) - 1.0022).abs() < 0.001);
    }

    #[test]
    fn test_fft_processor_initial_state() {
        let proc = FftProcessor::new(FftConfig::default());
        assert!(proc.is_ready());
        assert_eq!(proc.spectrum().len(), 2048);
        assert!(proc.spectrum().iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_fft_push_and_read_empty() {
        let mut proc = FftProcessor::new(FftConfig::default());
        let samples = vec![0.1f32; 1024];
        proc.push_pcm(&samples, 44100);
        let spec = proc.read_spectrum(16.0);
        assert!(spec.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn test_fft_push_and_read_full() {
        let mut proc = FftProcessor::new(FftConfig::default());
        let rate = 44100.0;
        let freq = 440.0;
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (TAU * freq * i as f32 / rate).sin())
            .collect();
        proc.push_pcm(&samples, 44100);
        let spec = proc.read_spectrum(16.0).to_vec();
        assert!(spec.iter().any(|&v| v > 0.0));
        assert!(proc.pcm_queue.len() < FFT_SIZE);
    }

    #[test]
    fn test_raw_bins_caching() {
        let mut proc = FftProcessor::new(FftConfig::default());
        let rate = 44100.0;
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (TAU * 440.0 * i as f32 / rate).sin())
            .collect();
        proc.push_pcm(&samples, 44100);
        proc.read_spectrum(16.0);
        let bins = proc.get_raw_bins(2).unwrap();
        assert_eq!(bins.len(), 2);
        assert!(bins.iter().any(|&v| v > 0.0));
    }

    #[test]
    fn test_lowfreq_analyzer_window_fill() {
        let mut analyzer = LowFreqAnalyzer::new(LowFreqConfig {
            bin_count: 2,
            window_size: 3,
            gradient_threshold: 0.01,
            ..Default::default()
        });

        // Call 1: new value → push to window, value=0 → decay from 1.0
        let v1 = analyzer.analyze(&[50000.0, 25000.0], 16.0);
        assert!(v1 < 1.0, "decay from initial 1.0, got {}", v1);

        // Call 2: duplicate → fall through to gradient → curValue rises
        let v2 = analyzer.analyze(&[50000.0, 25000.0], 16.0);
        assert!(v2 > v1, "duplicate triggers gradient, got {} > {}", v2, v1);

        // Call 3: new quiet → push+0 → decay
        let v3 = analyzer.analyze(&[10.0, 5.0], 16.0);
        assert!(v3 < v2, "quiet value decays, got {} < {}", v3, v2);

        // Call 4: new value → full window → gradient → rises
        let v4 = analyzer.analyze(&[200.0, 100.0], 16.0);
        assert!(v4 > v3, "window full, should rise, got {} > {}", v4, v3);

        // Call 5: loud signal → continues rising
        let v5 = analyzer.analyze(&[50000.0, 25000.0], 16.0);
        assert!(v5 > v4, "loud bass keeps rising, got {} > {}", v5, v4);
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
        assert!((proc.peak_value - 0.0001).abs() < 1e-8);

        let rate = 44100.0;
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (TAU * 440.0 * i as f32 / rate).sin())
            .collect();
        proc.push_pcm(&samples, 44100);
        proc.read_spectrum(16.0);
        assert!(proc.peak_value > 0.0001);
    }

    #[test]
    fn test_clear_queue() {
        let mut proc = FftProcessor::new(FftConfig::default());
        let rate = 44100.0;
        let samples: Vec<f32> = (0..FFT_SIZE * 3)
            .map(|i| (TAU * 440.0 * i as f32 / rate).sin())
            .collect();
        proc.push_pcm(&samples, 44100);
        proc.clear_queue();
        assert!(proc.pcm_queue.is_empty());
        assert!(proc.spectrum().iter().all(|&v| v == 0.0));
        assert!((proc.peak_value - 0.0001).abs() < 1e-8);
    }

    #[test]
    fn test_time_based_drain() {
        let mut proc = FftProcessor::new(FftConfig::default());
        let rate = 44100.0;
        let samples: Vec<f32> = (0..FFT_SIZE * 4)
            .map(|i| (TAU * 440.0 * i as f32 / rate).sin())
            .collect();
        proc.push_pcm(&samples, 44100);
        proc.read_spectrum(16.0);
        let remaining = proc.pcm_queue.len();
        assert!(remaining < 8192, "Queue should be drained, got {}", remaining);
        assert!(remaining > 7000, "Time-based drain only, got {}", remaining);
    }

    #[test]
    fn test_audio_processor_process_frame() {
        let mut proc = AudioProcessor::new(
            1024, 80.0, 2000.0, 2, 10, 0.35, 0.003,
        );
        let rate = 44100.0;
        let samples: Vec<f32> = (0..FFT_SIZE * 2)
            .map(|i| (TAU * 440.0 * i as f32 / rate).sin())
            .collect();
        proc.push_pcm(&samples, 44100);

        let mut spectrum = vec![0.0f32; 1024];
        let low_freq = proc.process_frame(16.0, &mut spectrum);

        assert_eq!(spectrum.len(), 1024);
        assert!(spectrum.iter().any(|&v| v > 0.0));
        assert!(
            low_freq >= 0.0 && low_freq <= 1.0,
            "lowFreq {} out of [0, 1]",
            low_freq
        );
    }

    #[test]
    fn test_bass_vs_treble_lowfreq() {
        let mut proc_bass =
            AudioProcessor::new(2048, 80.0, 2000.0, 2, 10, 0.01, 0.003);
        let mut proc_treble =
            AudioProcessor::new(2048, 80.0, 2000.0, 2, 10, 0.01, 0.003);

        let rate = 44100.0;

        for _ in 0..15 {
            let bass: Vec<f32> = (0..FFT_SIZE)
                .map(|i| (TAU * 80.0 * i as f32 / rate).sin())
                .collect();
            proc_bass.push_pcm(&bass, 44100);

            let treble: Vec<f32> = (0..FFT_SIZE)
                .map(|i| (TAU * 5000.0 * i as f32 / rate).sin())
                .collect();
            proc_treble.push_pcm(&treble, 44100);

            let mut spectrum = vec![0.0f32; 2048];
            proc_bass.process_frame(16.0, &mut spectrum);
            proc_treble.process_frame(16.0, &mut spectrum);
        }

        let raw_bass = proc_bass.fft.get_raw_bins(2).unwrap().to_vec();
        let raw_treble = proc_treble.fft.get_raw_bins(2).unwrap().to_vec();
        let sum_bass = raw_bass.iter().sum::<f32>();
        let sum_treble = raw_treble.iter().sum::<f32>();

        assert!(
            sum_bass > 100.0 * sum_treble,
            "Bass raw_bins sum={:.6} should be >> treble sum={:.6}",
            sum_bass,
            sum_treble,
        );
    }
}
