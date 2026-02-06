/**
 * Low Frequency Volume Analyzer
 *
 * Calculates bass volume from WASM FFTPlayer spectrum data (Hamming window).
 * Matching AMLL FFTToLowPassContext logic:
 *   - Average the first binCount bins (low-frequency portion of FFT output)
 *   - Normalize: (average / 255) * boostMultiplier
 *   - Floor: if volume > floorThreshold, clamp to at least minimumFloor
 *
 * Bin count is configured by AudioEffectManager based on frequency range:
 *   WASM FFT covers 80-2000Hz → binCount ≈ 91 (desktop) covers 80-250Hz
 */

import type { LowFreqAnalyzerOptions } from './types';

const DEFAULT_OPTIONS: Required<LowFreqAnalyzerOptions> = {
  binCount: 10,
  boostMultiplier: 3.0,
  minimumFloor: 0.4,
  floorThreshold: 0.1,
};

/**
 * Low Frequency Volume Analyzer for background animation effects
 */
export class LowFreqVolumeAnalyzer {
  private readonly options: Required<LowFreqAnalyzerOptions>;

  constructor(options?: LowFreqAnalyzerOptions) {
    this.options = { ...DEFAULT_OPTIONS, ...options };
  }

  /**
   * Analyze FFT data and return low-frequency volume.
   * Accepts number[] from WASM FFTPlayer or Uint8Array for compatibility.
   * Values expected in 0-255 range.
   * @param fftData FFT amplitude data (values 0-255)
   * @returns Low-frequency volume (0-3 range)
   */
  public analyze(fftData: ArrayLike<number>): number {
    const { binCount, boostMultiplier, minimumFloor, floorThreshold } = this.options;

    if (!fftData || fftData.length < binCount) {
      return 0;
    }

    // Average low-frequency bins [0, binCount)
    let sum = 0;
    for (let i = 0; i < binCount; i++) {
      sum += fftData[i];
    }

    const average = sum / binCount;
    let volume = (average / 255) * boostMultiplier;

    // Apply floor to prevent flickering when audio is active
    if (volume > floorThreshold) {
      volume = Math.max(volume, minimumFloor);
    }

    return volume;
  }

  /**
   * Reset the analyzer state
   */
  public reset(): void {
    // Stateless analyzer — no state to reset
  }
}
