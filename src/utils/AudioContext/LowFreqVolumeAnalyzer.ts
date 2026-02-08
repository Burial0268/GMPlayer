/**
 * Low Frequency Volume Analyzer
 *
 * Matching AMLL FFTToLowPassContext (commit 48fb050d):
 *   - amplitudeToLevel: log10-based conversion (amplitude / 255)
 *   - calculateGradient: sliding window, first N FFT bins averaged
 *   - Time-delta smoothing: curValue += (value - curValue) * smoothingFactor * delta
 *
 * Input: Raw WASM FFTPlayer magnitudes (NOT 0-255 normalized).
 *   In official AMLL, fftDataAtom contains raw float magnitudes from native FFTPlayer,
 *   so amplitudeToLevel(rawMagnitude) with values >> 255 produces levels in 0.x range,
 *   yielding final output in the 0.x-1.0 range (matching official behavior).
 */

export interface LowFreqVolumeOptions {
  /** Number of FFT bins to average for low-freq detection (default: 2) */
  binCount?: number;
  /** Sliding window size for gradient calculation (default: 10) */
  windowSize?: number;
  /** Gradient threshold — above this, use maxInInterval² (default: 0.35) */
  gradientThreshold?: number;
  /** Time-delta smoothing factor (default: 0.003) */
  smoothingFactor?: number;
}

const DEFAULT_OPTIONS: Required<LowFreqVolumeOptions> = {
  binCount: 2,
  windowSize: 10,
  gradientThreshold: 0.35,
  smoothingFactor: 0.003,
};

/**
 * Low Frequency Volume Analyzer for background animation effects
 * Matches AMLL FFTToLowPassContext algorithm
 */
export class LowFreqVolumeAnalyzer {
  // Gradient sliding window state
  private _gradient: number[] = [];

  // Configurable parameters
  private _binCount: number;
  private _windowSize: number;
  private _gradientThreshold: number;
  private _smoothingFactor: number;

  // Smoothed output state
  private _curValue: number = 1;
  private _lastTime: number = 0;

  constructor(options?: LowFreqVolumeOptions) {
    const opts = { ...DEFAULT_OPTIONS, ...options };
    this._binCount = opts.binCount;
    this._windowSize = opts.windowSize;
    this._gradientThreshold = opts.gradientThreshold;
    this._smoothingFactor = opts.smoothingFactor;
  }

  /**
   * Update analysis parameters at runtime.
   */
  setOptions(options: Partial<LowFreqVolumeOptions>): void {
    if (options.binCount !== undefined) this._binCount = options.binCount;
    if (options.windowSize !== undefined) this._windowSize = options.windowSize;
    if (options.gradientThreshold !== undefined) this._gradientThreshold = options.gradientThreshold;
    if (options.smoothingFactor !== undefined) this._smoothingFactor = options.smoothingFactor;
  }

  /**
   * Get current options.
   */
  getOptions(): Required<LowFreqVolumeOptions> {
    return {
      binCount: this._binCount,
      windowSize: this._windowSize,
      gradientThreshold: this._gradientThreshold,
      smoothingFactor: this._smoothingFactor,
    };
  }

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
   * Uses first N FFT bins averaged, tracks window of configurable size.
   * Returns either maxInInterval² (if difference > threshold) or minInInterval * 0.25.
   */
  private _calculateGradient(fftData: ArrayLike<number>): number {
    let sum = 0;
    const count = Math.min(this._binCount, fftData.length);
    for (let i = 0; i < count; i++) {
      sum += this._amplitudeToLevel(fftData[i]);
    }
    const volume = sum / count;

    if (this._gradient.length < this._windowSize && !this._gradient.includes(volume)) {
      this._gradient.push(volume);
      return 0;
    }

    this._gradient.shift();
    this._gradient.push(volume);

    const maxInInterval = Math.max(...this._gradient) ** 2;
    const minInInterval = Math.min(...this._gradient);
    const difference = maxInInterval - minInInterval;

    return difference > this._gradientThreshold ? maxInInterval : minInInterval * 0.25;
  }

  /**
   * Analyze FFT data and return smoothed low-frequency volume.
   * Accepts number[] from WASM FFTPlayer (values 0-255).
   * @param fftData FFT amplitude data (values 0-255)
   * @returns Smoothed low-frequency volume
   */
  public analyze(fftData: ArrayLike<number>): number {
    if (!fftData || fftData.length < this._binCount) {
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
        this._curValue + (value - this._curValue) * this._smoothingFactor * delta,
      );
    } else {
      this._curValue = Math.max(
        value,
        this._curValue + (value - this._curValue) * this._smoothingFactor * delta,
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
