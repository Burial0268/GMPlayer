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

    // --- Dynamic peak normalization ---
    const rawBuf = this._readBuffer;
    const smoothed = this._smoothedBuffer;
    const outSize = this._outputSize;

    // Find frame peak from raw FFT magnitudes
    let framePeak = 0;
    for (let i = 0; i < rawBuf.length; i++) {
      if (rawBuf[i] > framePeak) framePeak = rawBuf[i];
    }

    // Asymmetric peak tracking: fast attack, slow release
    if (framePeak > this._peakValue) {
      // Fast attack: blend toward new peak
      this._peakValue = this._peakValue * 0.5 + framePeak * 0.5;
    } else {
      // Slow release: gradual decay
      this._peakValue *= 0.995;
    }

    // Floor prevents noise amplification during silence
    if (this._peakValue < 0.0001) this._peakValue = 0.0001;

    // Interpolate raw 2048 buffer to output size, normalize, and smooth
    const srcLen = rawBuf.length;
    const ratio = (srcLen - 1) / (outSize - 1);
    const EMA_ALPHA = 0.5;

    for (let i = 0; i < outSize; i++) {
      // Linear interpolation from 2048 → outputSize
      const srcIdx = i * ratio;
      const lo = srcIdx | 0;
      const hi = Math.min(lo + 1, srcLen - 1);
      const frac = srcIdx - lo;
      const magnitude = rawBuf[lo] * (1 - frac) + rawBuf[hi] * frac;

      // Normalize with sqrt compression: sqrt(mag / peak) * 255
      const normalized = Math.sqrt(Math.max(0, magnitude) / this._peakValue) * 255;
      const clamped = Math.min(255, normalized);

      // EMA temporal smoothing to reduce jitter
      smoothed[i] = smoothed[i] * (1 - EMA_ALPHA) + clamped * EMA_ALPHA;

      this._outputBuffer[i] = smoothed[i];
    }

    return this._outputBuffer;
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
    if (this._smoothedBuffer) this._smoothedBuffer.fill(0);
    this._outputBuffer.fill(0);
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
