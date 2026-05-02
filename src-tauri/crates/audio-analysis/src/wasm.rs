/// WASM bindings for the `audio-analysis` crate.
///
/// Exposes a unified `WasmAudioProcessor` that bundles FFT processing +
/// low-frequency volume analysis into a single WASM module.
///
/// JS-side usage (replaces WasmFFTManager + LowFreqVolumeAnalyzer):
/// ```js
/// const proc = new WasmAudioProcessor(1024, 80, 2000, 2, 10, 0.35, 0.003);
/// proc.pushPCM(sampleRate, pcmBlock);     // per AudioWorklet tick
/// const lowFreq = proc.processFrame(deltaMs, spectrum); // per RAF frame
/// const rawBins = proc.getRawBins(2);     // raw magnitudes for debug
/// ```

use wasm_bindgen::prelude::*;

use crate::{AudioProcessor, LowFreqOptions};

#[wasm_bindgen]
pub struct WasmAudioProcessor {
    inner: AudioProcessor,
    /// Cached spectrum for `getSpectrum` getter
    cached_spectrum: Vec<f32>,
    /// Cached lowFreq volume from last processFrame
    cached_low_freq: f32,
}

#[wasm_bindgen]
impl WasmAudioProcessor {
    /// Create a new unified audio processor.
    ///
    /// Parameters match TypeScript defaults:
    /// - `output_size`: spectrum output size (default 2048 desktop, 1024 mobile)
    /// - `freq_min`: min frequency in Hz (default 80)
    /// - `freq_max`: max frequency in Hz (default 2000)
    /// - `bin_count`: raw FFT bins for lowFreq analysis (default 2)
    /// - `window_size`: gradient sliding window size (default 10)
    /// - `gradient_threshold`: gradient trigger threshold (default 0.35 desktop, 0.1 mobile)
    /// - `smoothing_factor`: time-delta smoothing speed (default 0.003)
    #[wasm_bindgen(constructor)]
    pub fn new(
        output_size: usize,
        freq_min: f32,
        freq_max: f32,
        bin_count: usize,
        window_size: usize,
        gradient_threshold: f32,
        smoothing_factor: f32,
    ) -> Self {
        let inner = AudioProcessor::new(
            output_size, freq_min, freq_max,
            bin_count, window_size,
            gradient_threshold, smoothing_factor,
        );
        let out_size = output_size.max(1);
        Self {
            inner,
            cached_spectrum: vec![0.0; out_size],
            cached_low_freq: 1.0,
        }
    }

    /// Push mono PCM samples from AudioWorklet.
    /// Called ~86 times/sec (every ~512 samples at 44.1kHz).
    ///
    /// `sample_rate`: AudioContext sample rate (e.g. 44100, 48000)
    /// `samples`: Float32Array of mono PCM
    #[wasm_bindgen(js_name = "pushPCM")]
    pub fn push_pcm(&mut self, sample_rate: u32, samples: &[f32]) {
        self.inner.push_pcm(samples, sample_rate);
    }

    /// Single WASM call per RAF frame (~60 fps).
    ///
    /// Runs FFT (if ≥2048 PCM queued), normalizes to 0-255, computes raw bins,
    /// runs gradient + smoothing. Fills `spectrum` (Float32Array) with normalized
    /// spectrum values (0-255).
    ///
    /// `delta_ms`: milliseconds since last frame (from `performance.now()` diff).
    ///
    /// Returns smoothed low-frequency volume (0-1 range).
    #[wasm_bindgen(js_name = "processFrame")]
    pub fn process_frame(&mut self, delta_ms: f32, spectrum: &mut [f32]) -> f32 {
        let low_freq = self.inner.process_frame(delta_ms, spectrum);

        // Cache for getter accessors
        let cache_len = self.cached_spectrum.len().min(spectrum.len());
        self.cached_spectrum[..cache_len].copy_from_slice(&spectrum[..cache_len]);
        self.cached_low_freq = low_freq;

        low_freq
    }

    /// Get raw FFT magnitudes aggregated into `count` groups (128-group AMLL resolution).
    /// Returns a new Float32Array of aggregated raw magnitudes, or empty if not available.
    #[wasm_bindgen(js_name = "getRawBins")]
    pub fn get_raw_bins(&mut self, count: usize) -> Vec<f32> {
        self.inner.fft.get_raw_bins(count).map(|b| b.to_vec()).unwrap_or_default()
    }

    /// Get the cached normalized spectrum (0-255) from the last `processFrame` call.
    #[wasm_bindgen(js_name = "getSpectrum")]
    pub fn get_spectrum(&self) -> Vec<f32> {
        self.cached_spectrum.clone()
    }

    /// Get the cached low-frequency volume (0-1) from the last `processFrame` call.
    #[wasm_bindgen(js_name = "getLowFreq")]
    pub fn get_low_freq(&self) -> f32 {
        self.cached_low_freq
    }

    /// Update frequency range for FFT spectrum output at runtime.
    #[wasm_bindgen(js_name = "setFreqRange")]
    pub fn set_freq_range(&mut self, min: f32, max: f32) {
        self.inner.fft.set_freq_range(min, max);
    }

    /// Update low-frequency analyzer options at runtime.
    /// All parameters are optional — pass undefined/null to keep current value.
    #[wasm_bindgen(js_name = "setLFOptions")]
    pub fn set_lf_options(
        &mut self,
        bin_count: Option<usize>,
        window_size: Option<usize>,
        gradient_threshold: Option<f32>,
        smoothing_factor: Option<f32>,
    ) {
        self.inner.analyzer.set_options(&LowFreqOptions {
            bin_count,
            window_size,
            gradient_threshold,
            smoothing_factor,
        });
    }

    /// Get current low-frequency analyzer configuration.
    #[wasm_bindgen(js_name = "getLFOptions")]
    pub fn get_lf_options(&self) -> LFOptionsJs {
        let c = self.inner.analyzer.config();
        LFOptionsJs {
            bin_count: c.bin_count,
            window_size: c.window_size,
            gradient_threshold: c.gradient_threshold,
            smoothing_factor: c.smoothing_factor,
        }
    }

    /// Check if the processor is ready (always true — no WASM init failure possible).
    #[wasm_bindgen(js_name = "isReady")]
    pub fn is_ready(&self) -> bool {
        self.inner.fft.is_ready()
    }

    /// Reset normalization, smoothing, and gradient state (e.g. on track change).
    /// Does NOT clear the PCM queue.
    #[wasm_bindgen]
    pub fn reset(&mut self) {
        self.inner.reset();
        self.cached_spectrum.fill(0.0);
        self.cached_low_freq = 0.0;
    }

    /// Clear PCM queue and reset all state (e.g. on seek).
    #[wasm_bindgen]
    pub fn clear(&mut self) {
        self.inner.clear();
        self.cached_spectrum.fill(0.0);
        self.cached_low_freq = 0.0;
    }

    /// Release all heap allocations.
    #[wasm_bindgen]
    pub fn free(&mut self) {
        self.inner.fft.free();
        self.inner.analyzer.reset();
        self.cached_spectrum.clear();
        self.cached_spectrum.shrink_to_fit();
    }
}

/// Return type for `getLFOptions` (plain data object visible in JS).
#[wasm_bindgen]
pub struct LFOptionsJs {
    pub bin_count: usize,
    pub window_size: usize,
    pub gradient_threshold: f32,
    pub smoothing_factor: f32,
}
