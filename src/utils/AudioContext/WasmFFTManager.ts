/**
 * WasmFFTManager - Wraps WASM FFTPlayer with dynamic peak normalization
 *
 * Pipeline:
 *   AudioWorklet PCM → FFTPlayer.pushDataF32() → FFTPlayer.read() → normalize → output
 *
 * Normalization (from AMLL AudioFFTVisualizer pattern):
 *   - Asymmetric peak tracking: fast attack (0.5 blend), slow release (0.995 decay)
 *   - Normalize: sqrt(magnitude / peakValue) * 255
 *   - Floor of 0.0001 prevents noise amplification during silence
 *   - JS-side temporal smoothing (EMA α≈0.35) reduces jitter
 */

import { FFTPlayer } from '@applemusic-like-lyrics/fft';

export class WasmFFTManager {
  private _fft: FFTPlayer | null = null;
  private _readBuffer: Float32Array | null = null;
  private _outputBuffer: number[] = [];
  private _smoothedBuffer: Float32Array | null = null;

  // Dynamic peak normalization state
  private _peakValue: number = 0.0001;

  // getRawBins cache
  private _cachedRawBins: number[] = [0, 0];
  private _cachedRawBinsCount: number = 2;
  private _rawBinsDirty: boolean = true;

  // Configuration
  private _freqMin: number = 80;
  private _freqMax: number = 2000;
  private _outputSize: number;

  constructor(outputSize: number = 1024) {
    this._outputSize = outputSize;

    try {
      this._fft = new FFTPlayer();
      this._fft.setFreqRange(this._freqMin, this._freqMax);

      // FFTPlayer uses a fixed 2048 result buffer internally
      this._readBuffer = new Float32Array(2048);
      this._smoothedBuffer = new Float32Array(this._outputSize);
      this._outputBuffer = new Array(this._outputSize).fill(0);
    } catch (err) {
      console.error('WasmFFTManager: Failed to create FFTPlayer', err);
      this._fft = null;
    }
  }

  /**
   * Push PCM audio data from AudioWorklet.
   * @param pcm Float32Array mono PCM data (typically 128 samples per block)
   * @param sampleRate Audio sample rate (e.g. 44100)
   * @param channels Number of audio channels (1 for mono from worklet)
   */
  pushData(pcm: Float32Array, sampleRate: number, channels: number): void {
    if (!this._fft) return;
    this._fft.pushDataF32(sampleRate, channels, pcm);
  }

  /**
   * Read FFT spectrum data, apply normalization and smoothing.
   * @returns number[] of values in 0-255 range, or empty array on failure
   */
  readSpectrum(): number[] {
    if (!this._fft || !this._readBuffer || !this._smoothedBuffer) {
      return this._outputBuffer;
    }

    // Read FFT data — returns false if < 2048 PCM samples queued
    const hasData = this._fft.read(this._readBuffer);
    if (!hasData) {
      return this._outputBuffer;
    }

    this._rawBinsDirty = true;
    this._normalizeAndSmooth();

    return this._outputBuffer;
  }

  /**
   * Apply dynamic peak normalization and EMA smoothing to _readBuffer,
   * writing results into _outputBuffer.
   *
   * Both paths (1:1 and interpolation) use consistent peak-after-normalize timing:
   * the PREVIOUS frame's peak normalizes the CURRENT frame, giving smoother visuals.
   */
  private _normalizeAndSmooth(): void {
    const rawBuf = this._readBuffer!;
    const smoothed = this._smoothedBuffer!;
    const outSize = this._outputSize;
    const output = this._outputBuffer;
    const srcLen = rawBuf.length;
    // Cache peak locally — avoid repeated property access in hot loop
    const peakValue = this._peakValue;
    const invPeak = 255 / Math.sqrt(peakValue);

    let framePeak = 0;

    if (outSize === srcLen) {
      // Fast path: 1:1 mapping, peak scan merged with normalize
      for (let i = 0; i < outSize; i++) {
        const mag = rawBuf[i];
        if (mag > framePeak) framePeak = mag;
        const normalized = Math.sqrt(mag > 0 ? mag : 0) * invPeak;
        smoothed[i] = smoothed[i] * 0.5 + (normalized < 255 ? normalized : 255) * 0.5;
        output[i] = smoothed[i];
      }
    } else {
      // Interpolation path: peak scan over raw, normalize over output
      const ratio = (srcLen - 1) / (outSize - 1);

      for (let i = 0; i < srcLen; i++) {
        if (rawBuf[i] > framePeak) framePeak = rawBuf[i];
      }

      for (let i = 0; i < outSize; i++) {
        const srcIdx = i * ratio;
        const lo = srcIdx | 0;
        const frac = srcIdx - lo;
        const hi = lo + 1 < srcLen ? lo + 1 : lo;
        const mag = rawBuf[lo] + (rawBuf[hi] - rawBuf[lo]) * frac;
        const normalized = Math.sqrt(mag > 0 ? mag : 0) * invPeak;
        smoothed[i] = smoothed[i] * 0.5 + (normalized < 255 ? normalized : 255) * 0.5;
        output[i] = smoothed[i];
      }
    }

    // Asymmetric peak tracking: fast attack, slow release
    if (framePeak > this._peakValue) {
      this._peakValue = this._peakValue * 0.5 + framePeak * 0.5;
    } else {
      this._peakValue *= 0.995;
    }
    if (this._peakValue < 0.0001) this._peakValue = 0.0001;
  }

  /**
   * Get raw FFT magnitudes aggregated to match AMLL's 128-bin resolution.
   * Results are cached per frame (dirty flag set by readSpectrum).
   *
   * @param count Number of aggregated bins to return
   * @returns number[] of aggregated raw magnitudes, or null if unavailable
   */
  getRawBins(count: number): number[] | null {
    if (!this._readBuffer) return null;

    if (!this._rawBinsDirty && count === this._cachedRawBinsCount) {
      return this._cachedRawBins;
    }

    const rawLen = this._readBuffer.length; // 2048
    const binsPerGroup = rawLen / 128; // ~16 bins per AMLL-equivalent bin

    if (count !== this._cachedRawBinsCount) {
      this._cachedRawBins = new Array(count).fill(0);
      this._cachedRawBinsCount = count;
    }

    for (let i = 0; i < count; i++) {
      const start = Math.floor(i * binsPerGroup);
      const end = Math.floor((i + 1) * binsPerGroup);
      let sum = 0;
      for (let j = start; j < end; j++) {
        sum += this._readBuffer[j];
      }
      this._cachedRawBins[i] = sum / (end - start);
    }

    this._rawBinsDirty = false;
    return this._cachedRawBins;
  }

  /**
   * Check if the FFTPlayer was successfully initialized.
   */
  isReady(): boolean {
    return this._fft !== null;
  }

  /**
   * Set frequency range for FFT analysis.
   */
  setFreqRange(min: number, max: number): void {
    this._freqMin = min;
    this._freqMax = max;
    if (this._fft) {
      this._fft.setFreqRange(min, max);
    }
  }

  /**
   * Reset normalization and smoothing state (e.g. on track change).
   */
  reset(): void {
    this._peakValue = 0.0001;
    this._rawBinsDirty = true;
    if (this._readBuffer) this._readBuffer.fill(0);
    if (this._smoothedBuffer) this._smoothedBuffer.fill(0);
    this._outputBuffer.fill(0);
  }

  /**
   * Clear FFTPlayer internal state (pcm_queue + result_buf EMA).
   * Recreates the FFTPlayer instance since clear() is not exposed via wasm_bindgen.
   * Call on seek to flush stale PCM data from old playback position.
   */
  clearQueue(): void {
    this.reset();
    if (this._fft) {
      try {
        this._fft.free();
      } catch (e) {
        // May already be freed
      }
      try {
        this._fft = new FFTPlayer();
        this._fft.setFreqRange(this._freqMin, this._freqMax);
      } catch (e) {
        console.error('WasmFFTManager: Failed to recreate FFTPlayer', e);
        this._fft = null;
      }
    }
  }

  /**
   * Release WASM memory and cleanup.
   */
  free(): void {
    if (this._fft) {
      try {
        this._fft.free();
      } catch (e) {
        // May already be freed
      }
      this._fft = null;
    }
    this._readBuffer = null;
    this._smoothedBuffer = null;
    this._outputBuffer = [];
  }
}
