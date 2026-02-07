/**
 * Low Frequency Volume Analyzer
 *
 * Matching AMLL FFTToLowPassContext (commit 48fb050d):
 *   - amplitudeToLevel: log10-based conversion (amplitude / 255)
 *   - calculateGradient: sliding window of 10, first 2 FFT bins
 *   - Time-delta smoothing: curValue += (value - curValue) * 0.003 * delta
 *
 * Input: Raw WASM FFTPlayer magnitudes (NOT 0-255 normalized).
 *   In official AMLL, fftDataAtom contains raw float magnitudes from native FFTPlayer,
 *   so amplitudeToLevel(rawMagnitude) with values >> 255 produces levels in 0.x range,
 *   yielding final output in the 0.x-1.0 range (matching official behavior).
 */

/**
 * Low Frequency Volume Analyzer for background animation effects
 * Matches AMLL FFTToLowPassContext algorithm
 */
export class LowFreqVolumeAnalyzer {
  // Gradient sliding window state
  private _gradient: number[] = [];
  private readonly _windowSize: number = 10;

  // Smoothed output state
  private _curValue: number = 1;
  private _lastTime: number = 0;

  /**
   * Convert FFT amplitude (0-255) to log10 level.
   * From AMLL: 0.5 * Math.log10(normalizedAmplitude + 1)
   */
  private _amplitudeToLevel(amplitude: number): number {
    const normalizedAmplitude = amplitude / 255;
    return 0.5 * Math.log10(normalizedAmplitude + 1);
  }

  /**
   * Calculate gradient from sliding window of low-freq bin levels.
   * Uses first 2 FFT bins averaged, tracks window of 10 values.
   * Returns either maxInInterval² (if difference > 0.35) or minInInterval * 0.25.
   */
  private _calculateGradient(fftData: ArrayLike<number>): number {
    const volume =
      (this._amplitudeToLevel(fftData[0]) + this._amplitudeToLevel(fftData[1])) * 0.5;

    if (this._gradient.length < this._windowSize && !this._gradient.includes(volume)) {
      this._gradient.push(volume);
      return 0;
    }

    this._gradient.shift();
    this._gradient.push(volume);

    const maxInInterval = Math.max(...this._gradient) ** 2;
    const minInInterval = Math.min(...this._gradient);
    const difference = maxInInterval - minInInterval;

    return difference > 0.35 ? maxInInterval : minInInterval * 0.25;
  }

  /**
   * Analyze FFT data and return smoothed low-frequency volume.
   * Accepts number[] from WASM FFTPlayer (values 0-255).
   * @param fftData FFT amplitude data (values 0-255)
   * @returns Smoothed low-frequency volume
   */
  public analyze(fftData: ArrayLike<number>): number {
    if (!fftData || fftData.length < 2) {
      return this._curValue;
    }

    const now = performance.now();
    const delta = this._lastTime > 0 ? now - this._lastTime : 16;
    this._lastTime = now;

    const value = this._calculateGradient(fftData);

    // Time-delta based smoothing (matching AMLL onFrame)
    const increasing = this._curValue < value;
    if (increasing) {
      this._curValue = Math.min(
        value,
        this._curValue + (value - this._curValue) * 0.003 * delta,
      );
    } else {
      this._curValue = Math.max(
        value,
        this._curValue + (value - this._curValue) * 0.003 * delta,
      );
    }

    if (Number.isNaN(this._curValue)) this._curValue = 1;

    return this._curValue;
  }

  /**
   * Reset the analyzer state
   */
  public reset(): void {
    this._gradient = [];
    this._curValue = 1;
    this._lastTime = 0;
  }
}
