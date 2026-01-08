/**
 * Low Frequency Volume Analyzer
 * Based on applemusic-like-lyrics implementation
 * Extracts and smooths low-frequency (80-120Hz) volume data from FFT
 */

/**
 * Convert amplitude (0-255) to logarithmic level (0-1)
 */
function amplitudeToLevel(amplitude: number): number {
  const normalizedAmplitude = amplitude / 255;
  const level = 0.5 * Math.log10(normalizedAmplitude + 1);
  return level;
}

/**
 * Low Frequency Volume Analyzer with sliding window smoothing
 */
export class LowFreqVolumeAnalyzer {
  private volumeHistory: number[] = [];
  private readonly windowSize = 10;

  /**
   * Calculate raw low-frequency volume from FFT data
   * Uses the first two frequency bins (80-120Hz range)
   */
  private calculateRawVolume(fftData: number[]): number {
    if (!fftData || fftData.length < 2) {
      return 0;
    }

    // Average the first two bins for low frequency range
    const volume = (amplitudeToLevel(fftData[0]) + amplitudeToLevel(fftData[1])) * 0.5;
    return Math.max(0, Math.min(1, volume));
  }

  /**
   * Apply sliding window smoothing with adaptive threshold
   * Matches AMLL implementation exactly
   */
  private smoothVolume(volume: number): number {
    // Add to history
    this.volumeHistory.push(volume);

    // Maintain window size
    if (this.volumeHistory.length > this.windowSize) {
      this.volumeHistory.shift();
    }

    // Need at least a few samples for smoothing
    if (this.volumeHistory.length < 3) {
      return volume;
    }

    // Find max and min in window
    // AMLL: maxInInterval is already squared, minInInterval is not
    const maxInInterval = Math.max(...this.volumeHistory) ** 2;
    const minInInterval = Math.min(...this.volumeHistory);

    // Adaptive threshold: difference = max² - min (not min²)
    const difference = maxInInterval - minInInterval;

    // Return appropriate value based on threshold
    // AMLL: returns max² or min * 0.25 (which is min * 0.5²)
    if (difference > 0.35) {
      return maxInInterval;  // Already squared
    } else {
      return minInInterval * 0.25;  // min * 0.5² = min * 0.25
    }
  }

  /**
   * Analyze FFT data and return smoothed low-frequency volume
   * @param fftData FFT amplitude data array (should be 256 elements)
   * @returns Smoothed low-frequency volume (0-1 range)
   */
  public analyze(fftData: number[]): number {
    const rawVolume = this.calculateRawVolume(fftData);
    const smoothedVolume = this.smoothVolume(rawVolume);
    return smoothedVolume;
  }

  /**
   * Reset the analyzer state (clear history)
   */
  public reset(): void {
    this.volumeHistory = [];
  }
}
