/// WASM bindings for the `lowfreq` crate.
///
/// Exposes a unified `WasmAudioProcessor` that bundles FFT processing +
/// low-frequency volume analysis into a single WASM module.
///
/// JS-side usage (replaces WasmFFTManager + LowFreqVolumeAnalyzer):
/// ```js
/// const proc = new WasmAudioProcessor(2048, 2, 10, 0.35, 0.003);
/// proc.pushPCM(pcmBlock);          // per AudioWorklet tick
/// const lowFreq = proc.processFrame(deltaMs, spectrum); // per RAF frame
/// const fft = proc.getSpectrum();  // normalized 0-255
/// ```

use lowfreq::{FftConfig, FftProcessor, LowFreqAnalyzer, LowFreqConfig, LowFreqOptions};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WasmAudioProcessor {
    fft: FftProcessor,
    analyzer: LowFreqAnalyzer,
    /// Cached spectrum for the `spectrum` getter (owned Vec, returned by reference to JS)
    cached_spectrum: Vec<f32>,
}

#[wasm_bindgen]
impl WasmAudioProcessor {
    /// Create a new unified audio processor.
    ///
    /// All parameters match the TypeScript defaults:
    /// - `output_size`: spectrum output size (default 2048)
    /// - `bin_count`: number of raw FFT bins for lowFreq (default 2)
    /// - `window_size`: gradient sliding window size (default 10)
    /// - `gradient_threshold`: gradient trigger threshold (default 0.35 desktop, 0.1 mobile)
    /// - `smoothing_factor`: time-delta smoothing speed (default 0.003)
    #[wasm_bindgen(constructor)]
    pub fn new(
        output_size: usize,
        bin_count: usize,
        window_size: usize,
        gradient_threshold: f32,
        smoothing_factor: f32,
    ) -> Self {
        let fft_config = FftConfig {
            output_size: output_size.max(1),
        };
        let lf_config = LowFreqConfig {
            bin_count: bin_count.max(1),
            window_size: window_size.max(2),
            gradient_threshold,
            smoothing_factor,
        };
        let out_size = fft_config.output_size;
        Self {
            fft: FftProcessor::new(fft_config),
            analyzer: LowFreqAnalyzer::new(lf_config),
            cached_spectrum: vec![0.0; out_size],
        }
    }

    /// Push mono PCM samples from AudioWorklet.
    /// Called ~86 times/sec (every ~512 samples at 44.1kHz).
    #[wasm_bindgen(js_name = "pushPCM")]
    pub fn push_pcm(&mut self, samples: &[f32]) {
        self.fft.push_pcm(samples);
    }

    /// Single WASM call per RAF frame (~60 fps).
    ///
    /// Runs FFT (if ≥2048 PCM queued), normalizes to 0-255, computes raw bins,
    /// runs gradient + smoothing, caches the spectrum.
    ///
    /// `delta_ms`: milliseconds since last frame (from `performance.now()` diff).
    /// `spectrum`: output Float32Array filled with normalized spectrum (0-255).
    ///
    /// Returns smoothed low-frequency volume (0-1 range).
    #[wasm_bindgen(js_name = "processFrame")]
    pub fn process_frame(&mut self, delta_ms: f32, spectrum: &mut [f32]) -> f32 {
        // FFT + normalize
        let spec = self.fft.read_spectrum();
        let len = spectrum.len().min(spec.len());
        spectrum[..len].copy_from_slice(&spec[..len]);

        // Cache for getSpectrum()
        let cache_len = self.cached_spectrum.len().min(spec.len());
        self.cached_spectrum[..cache_len].copy_from_slice(&spec[..cache_len]);

        // LowFreq: raw bins → gradient → smoothing
        let bin_count = self.analyzer.config().bin_count;
        if let Some(raw_bins) = self.fft.get_raw_bins(bin_count) {
            self.analyzer.analyze(raw_bins, delta_ms)
        } else {
            self.analyzer.analyze(&[], delta_ms)
        }
    }

    /// Get the cached normalized spectrum (0-255) from the last `processFrame` call.
    #[wasm_bindgen(js_name = "getSpectrum")]
    pub fn get_spectrum(&self) -> Vec<f32> {
        self.cached_spectrum.clone()
    }

    /// Get the cached low-frequency volume (0-1) from the last `processFrame` call.
    #[wasm_bindgen(js_name = "getLowFreq")]
    pub fn get_low_freq(&self) -> f32 {
        self.analyzer.value()
    }

    /// Update low-frequency analyzer options at runtime.
    #[wasm_bindgen(js_name = "setLFOptions")]
    pub fn set_lf_options(
        &mut self,
        bin_count: Option<usize>,
        window_size: Option<usize>,
        gradient_threshold: Option<f32>,
        smoothing_factor: Option<f32>,
    ) {
        self.analyzer.set_options(&LowFreqOptions {
            bin_count,
            window_size,
            gradient_threshold,
            smoothing_factor,
        });
    }

    /// Check if the processor is ready (always true — no WASM init failure possible).
    #[wasm_bindgen(js_name = "isReady")]
    pub fn is_ready(&self) -> bool {
        self.fft.is_ready()
    }

    /// Get current low-frequency analyzer configuration.
    #[wasm_bindgen(js_name = "getLFOptions")]
    pub fn get_lf_options(&self) -> LFOptionsJs {
        let c = self.analyzer.config();
        LFOptionsJs {
            bin_count: c.bin_count,
            window_size: c.window_size,
            gradient_threshold: c.gradient_threshold,
            smoothing_factor: c.smoothing_factor,
        }
    }

    /// Reset normalization, smoothing, and gradient state (e.g. on track change).
    /// Does NOT clear the PCM queue.
    #[wasm_bindgen]
    pub fn reset(&mut self) {
        self.fft.reset();
        self.analyzer.reset();
        self.cached_spectrum.fill(0.0);
    }

    /// Clear PCM queue and reset all state (e.g. on seek).
    #[wasm_bindgen]
    pub fn clear(&mut self) {
        self.fft.clear_queue();
        self.analyzer.reset();
        self.cached_spectrum.fill(0.0);
    }

    /// Release all heap allocations.
    #[wasm_bindgen]
    pub fn free(&mut self) {
        self.fft.free();
        self.analyzer.reset();
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
