/**
 * Low Frequency Volume Analyzer
 * Based on AMLL (Apple Music-like Lyrics) implementation
 * - Averages low-frequency FFT bins
 * - Applies boost multiplier for dynamic response
 * - Floor threshold prevents visual flickering
 * - Relies on AnalyserNode.smoothingTimeConstant for temporal smoothing
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
   * Analyze FFT data and return low-frequency volume
   * @param fftData FFT amplitude data array (values 0-255)
   * @returns Low-frequency volume (typically 0-3 range, can exceed 1.0)
   */
  public analyze(fftData: number[]): number {
    const { binCount, boostMultiplier, minimumFloor, floorThreshold } = this.options;

    if (!fftData || fftData.length < binCount) {
      return 0;
    }

    // Average low-frequency bins
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
