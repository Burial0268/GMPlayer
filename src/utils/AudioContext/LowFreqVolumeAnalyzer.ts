/**
 * Low Frequency Volume Analyzer
 * Combines approaches from AMLL and SPlayer implementations
 * - Threshold-based filtering for noise rejection
 * - Power function for dynamic range expansion
 * - EMA smoothing for stability
 * - Configurable parameters
 */

import type { LowFreqAnalyzerOptions } from './types';

const DEFAULT_OPTIONS: Required<LowFreqAnalyzerOptions> = {
  binCount: 3,
  smoothFactor: 0.28,
  threshold: 180,
  powerExponent: 2,
};

/**
 * Low Frequency Volume Analyzer with threshold filtering and EMA smoothing
 */
export class LowFreqVolumeAnalyzer {
  private smoothedVolume = 0;
  private readonly options: Required<LowFreqAnalyzerOptions>;

  constructor(options?: LowFreqAnalyzerOptions) {
    this.options = { ...DEFAULT_OPTIONS, ...options };
  }

  /**
   * Calculate raw low-frequency volume from FFT data
   * Uses threshold-based normalization for better bass response
   * @param fftData FFT amplitude data array (Uint8Array values 0-255)
   */
  private calculateRawVolume(fftData: number[]): number {
    const { binCount, threshold, powerExponent } = this.options;

    if (!fftData || fftData.length < binCount) {
      return 0;
    }

    // Calculate average of low-frequency bins
    let sum = 0;
    for (let i = 0; i < binCount; i++) {
      sum += fftData[i];
    }
    const avg = sum / binCount;

    // Threshold-based normalization (values below threshold treated as silence)
    const maxValue = 255;
    const normalized = Math.max(0, (avg - threshold) / (maxValue - threshold));

    // Power function for dynamic range expansion
    // Makes quiet signals quieter while preserving louder values
    return Math.pow(normalized, powerExponent);
  }

  /**
   * Apply EMA (Exponential Moving Average) smoothing
   */
  private applySmoothing(rawVolume: number): number {
    const { smoothFactor } = this.options;
    this.smoothedVolume += smoothFactor * (rawVolume - this.smoothedVolume);
    return this.smoothedVolume;
  }

  /**
   * Analyze FFT data and return smoothed low-frequency volume
   * @param fftData FFT amplitude data array (values 0-255)
   * @returns Smoothed low-frequency volume (0-1 range)
   */
  public analyze(fftData: number[]): number {
    const rawVolume = this.calculateRawVolume(fftData);
    return this.applySmoothing(rawVolume);
  }

  /**
   * Get current smoothed volume without processing new data
   */
  public getCurrentVolume(): number {
    return this.smoothedVolume;
  }

  /**
   * Reset the analyzer state
   */
  public reset(): void {
    this.smoothedVolume = 0;
  }

  /**
   * Update options dynamically
   */
  public setOptions(options: Partial<LowFreqAnalyzerOptions>): void {
    Object.assign(this.options, options);
  }
}
