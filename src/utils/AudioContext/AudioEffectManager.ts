/**
 * AudioEffectManager - Optimized spectrum analysis and audio effects
 *
 * Key optimizations:
 * - Reusable buffer to avoid GC pressure
 * - Mobile-aware FFT size selection
 * - Throttled frequency data retrieval
 * - Integrated LowFreqVolumeAnalyzer
 */

import { LowFreqVolumeAnalyzer } from './LowFreqVolumeAnalyzer';
import { AudioContextManager } from './AudioContextManager';

export interface EffectManagerOptions {
  /** FFT size (power of 2, 32-32768). Default: 2048 desktop, 1024 mobile */
  fftSize?: number;
  /** AnalyserNode smoothing for inter-block averaging (0-1). Default: 0.8 */
  smoothingTimeConstant?: number;
  /** Min update interval in ms for getFrequencyData. Default: 16 (~60fps) */
  minUpdateInterval?: number;
}

const DEFAULT_OPTIONS: Required<EffectManagerOptions> = {
  fftSize: 2048,
  smoothingTimeConstant: 0.8,  // inter-block smoothing (simulates AMLL's window overlap)
  minUpdateInterval: 16,
};

const MOBILE_OPTIONS: Required<EffectManagerOptions> = {
  fftSize: 1024,
  smoothingTimeConstant: 0.8,
  minUpdateInterval: 33,  // ~30fps on mobile
};

/**
 * Linear interpolation (port of AMLL FFTPlayer's vec_interp / numpy.interp)
 * Resamples src array into dst array of any size using linear interpolation.
 */
function vecInterp(src: Uint8Array<ArrayBuffer>, dst: Float32Array): void {
  const srcLen = src.length;
  const dstLen = dst.length;

  if (srcLen === 0) { dst.fill(0); return; }
  if (dstLen === 0) return;
  if (srcLen === dstLen) {
    for (let i = 0; i < srcLen; i++) dst[i] = src[i];
    return;
  }

  const ratio = (srcLen - 1) / (dstLen - 1);
  for (let i = 0; i < dstLen; i++) {
    const srcIdx = i * ratio;
    const lo = srcIdx | 0; // floor
    const hi = Math.min(lo + 1, srcLen - 1);
    const frac = srcIdx - lo;
    dst[i] = src[lo] * (1 - frac) + src[hi] * frac;
  }
}

/**
 * AudioEffectManager - Manages spectrum analysis with performance optimizations
 */
export class AudioEffectManager {
  private audioCtx: AudioContext;
  private analyserNode: AnalyserNode | null = null;
  private lowFreqAnalyzer: LowFreqVolumeAnalyzer;
  private options: Required<EffectManagerOptions>;

  // Bandpass filter nodes for spectrum analysis range (80-2000Hz)
  private highpassFilter: BiquadFilterNode | null = null;
  private lowpassFilter: BiquadFilterNode | null = null;

  // Frequency range constants
  private static readonly FREQ_MIN = 80;
  private static readonly FREQ_MAX = 2000;

  // Reusable buffers to avoid GC pressure
  private _frequencyBuffer: Uint8Array<ArrayBuffer> | null = null;
  private _tempFrequencyArray: number[] = [];
  private _interpolatedBuffer: Float32Array | null = null;

  // Per-frame EMA smoothing buffer (AMLL FFTPlayer style: v = (v + new) / 2)
  private _smoothedBuffer: Float32Array | null = null;

  // Throttling state
  private _lastUpdateTime: number = 0;
  private _cachedFrequencyData: Uint8Array<ArrayBuffer> | null = null;

  // Connection state
  private _isConnected: boolean = false;

  constructor(audioCtx: AudioContext, options?: EffectManagerOptions) {
    this.audioCtx = audioCtx;

    // Apply mobile-optimized defaults
    const baseOptions = AudioContextManager.isMobile() ? MOBILE_OPTIONS : DEFAULT_OPTIONS;
    this.options = { ...baseOptions, ...options };

    this.lowFreqAnalyzer = new LowFreqVolumeAnalyzer();

    this._initNodes();
  }

  private _initNodes(): void {
    try {
      this.analyserNode = this.audioCtx.createAnalyser();
      this.analyserNode.fftSize = this.options.fftSize;
      // Inter-block smoothing handles averaging across audio blocks between frames.
      // Per-frame EMA (in getFrequencyData) adds cross-frame smoothing on top.
      this.analyserNode.smoothingTimeConstant = this.options.smoothingTimeConstant;

      // Create bandpass filter for spectrum analysis range (80-2000Hz)
      // Highpass filter: removes frequencies below 80Hz
      this.highpassFilter = this.audioCtx.createBiquadFilter();
      this.highpassFilter.type = 'highpass';
      this.highpassFilter.frequency.value = AudioEffectManager.FREQ_MIN;
      this.highpassFilter.Q.value = 0.7071; // Butterworth response (flat passband)

      // Lowpass filter: removes frequencies above 2000Hz
      this.lowpassFilter = this.audioCtx.createBiquadFilter();
      this.lowpassFilter.type = 'lowpass';
      this.lowpassFilter.frequency.value = AudioEffectManager.FREQ_MAX;
      this.lowpassFilter.Q.value = 0.7071; // Butterworth response

      // Pre-allocate buffers
      const bufferSize = this.analyserNode.frequencyBinCount;
      this._frequencyBuffer = new Uint8Array(bufferSize);
      this._cachedFrequencyData = new Uint8Array(bufferSize);
      this._smoothedBuffer = new Float32Array(bufferSize); // persistent EMA state
      this._tempFrequencyArray = new Array(bufferSize).fill(0);
    } catch (err) {
      console.error('AudioEffectManager: Failed to initialize nodes', err);
    }
  }

  /**
   * Connect input node to spectrum analysis chain (bandpass filtered)
   *
   * IMPORTANT: This creates a parallel analysis branch that does NOT affect audio output.
   * The bandpass filter (80-2000Hz) only applies to the analyser, not the actual sound.
   *
   * Audio graph structure:
   *                       ┌─→ highpass → lowpass → analyser (spectrum analysis only)
   *   inputNode (source) ─┤
   *                       └─→ (caller connects to gainNode → destination)
   *
   * @param inputNode The input audio node (source)
   * @returns The same inputNode for chaining to audio output
   */
  connect(inputNode: AudioNode): AudioNode {
    if (!this.analyserNode || !this.highpassFilter || !this.lowpassFilter) {
      console.warn('AudioEffectManager: Nodes not initialized');
      return inputNode;
    }

    try {
      // Connect input to bandpass filter chain for spectrum analysis ONLY
      // This is a parallel branch - does NOT affect audio output
      inputNode.connect(this.highpassFilter);
      this.highpassFilter.connect(this.lowpassFilter);
      this.lowpassFilter.connect(this.analyserNode);
      // Note: analyserNode is NOT connected to destination - it's analysis only

      this._isConnected = true;

      // Return the original inputNode so caller can connect it to audio output
      return inputNode;
    } catch (err) {
      console.error('AudioEffectManager: Failed to connect', err);
      return inputNode;
    }
  }

  /**
   * Check if connected to audio graph
   */
  isConnected(): boolean {
    return this._isConnected;
  }

  /**
   * Get frequency data for spectrum visualization.
   * Two-layer smoothing:
   *   1. AnalyserNode.smoothingTimeConstant handles inter-block averaging
   *      (compensates for data lost between frames, similar to AMLL's window overlap)
   *   2. Per-frame EMA: smoothed[i] = (smoothed[i] + raw[i]) / 2
   *      (matches AMLL FFTPlayer's cross-frame smoothing)
   * @param force Skip throttling check
   * @returns Uint8Array containing smoothed frequency data
   */
  getFrequencyData(force: boolean = false): Uint8Array<ArrayBuffer> {
    if (!this.analyserNode || !this._frequencyBuffer || !this._smoothedBuffer) {
      return this._cachedFrequencyData || new Uint8Array(0);
    }

    const now = performance.now();
    const elapsed = now - this._lastUpdateTime;

    // Return cached data if within throttle window (unless forced)
    if (!force && elapsed < this.options.minUpdateInterval && this._cachedFrequencyData) {
      return this._cachedFrequencyData;
    }

    // Fetch raw (unsmoothed) FFT data from AnalyserNode
    this.analyserNode.getByteFrequencyData(this._frequencyBuffer);

    // Apply AMLL-style per-frame EMA: v = (v + new) / 2
    const smoothed = this._smoothedBuffer;
    const raw = this._frequencyBuffer;
    for (let i = 0; i < raw.length; i++) {
      smoothed[i] = (smoothed[i] + raw[i]) / 2;
      raw[i] = smoothed[i]; // write back for return
    }

    // Copy to cached array
    if (this._cachedFrequencyData) {
      this._cachedFrequencyData.set(this._frequencyBuffer);
    }

    this._lastUpdateTime = now;
    return this._frequencyBuffer;
  }

  /**
   * Get frequency data interpolated to a target size
   * Port of AMLL FFTPlayer's vec_interp for flexible output sizing.
   * @param targetSize Desired output length (default: 2048, matching AMLL result_buf)
   * @returns Float32Array of interpolated frequency values (0-255 range)
   */
  getInterpolatedFrequencyData(targetSize: number = 2048): Float32Array {
    const raw = this.getFrequencyData(true);
    if (raw.length === 0) {
      return this._interpolatedBuffer ?? new Float32Array(targetSize);
    }

    // Reallocate only when target size changes
    if (!this._interpolatedBuffer || this._interpolatedBuffer.length !== targetSize) {
      this._interpolatedBuffer = new Float32Array(targetSize);
    }

    vecInterp(raw, this._interpolatedBuffer);
    return this._interpolatedBuffer;
  }

  /**
   * Get time domain data (waveform)
   * @returns Uint8Array containing time domain data
   */
  getTimeDomainData(): Uint8Array<ArrayBuffer> {
    if (!this.analyserNode) return new Uint8Array(0);
    const buffer = new Uint8Array(this.analyserNode.frequencyBinCount);
    this.analyserNode.getByteTimeDomainData(buffer);
    return buffer;
  }

  /**
   * Get low frequency volume for background effects
   * Uses AMLL-style analysis with boost and floor thresholds.
   * Uses the smoothed frequency data from the last getFrequencyData() call.
   * @returns Low-frequency volume (typically 0-3 range, passed to renderer's setLowFreqVolume)
   */
  getLowFrequencyVolume(): number {
    if (!this._smoothedBuffer) return 0;

    // Use the smoothed buffer directly — already EMA-processed
    const buf = this._smoothedBuffer;
    for (let i = 0; i < buf.length; i++) {
      this._tempFrequencyArray[i] = buf[i];
    }

    return this.lowFreqAnalyzer.analyze(this._tempFrequencyArray);
  }

  /**
   * Get current FFT size
   */
  getFFTSize(): number {
    return this.analyserNode?.fftSize || 0;
  }

  /**
   * Get frequency bin count (half of FFT size)
   */
  getFrequencyBinCount(): number {
    return this.analyserNode?.frequencyBinCount || 0;
  }

  /**
   * Update FFT size dynamically
   * Note: This will reallocate buffers
   */
  setFFTSize(size: number): void {
    if (!this.analyserNode) return;

    // Validate FFT size (must be power of 2, 32-32768)
    const validSize = Math.min(32768, Math.max(32, Math.pow(2, Math.round(Math.log2(size)))));
    this.analyserNode.fftSize = validSize;
    this.options.fftSize = validSize;

    // Reallocate buffers
    const bufferSize = this.analyserNode.frequencyBinCount;
    this._frequencyBuffer = new Uint8Array(bufferSize);
    this._cachedFrequencyData = new Uint8Array(bufferSize);
    this._smoothedBuffer = new Float32Array(bufferSize);
    this._tempFrequencyArray = new Array(bufferSize).fill(0);
  }

  /**
   * Disconnect all nodes and cleanup
   */
  disconnect(): void {
    if (this.highpassFilter) {
      try {
        this.highpassFilter.disconnect();
      } catch (e) {
        // May already be disconnected
      }
      this.highpassFilter = null;
    }

    if (this.lowpassFilter) {
      try {
        this.lowpassFilter.disconnect();
      } catch (e) {
        // May already be disconnected
      }
      this.lowpassFilter = null;
    }

    if (this.analyserNode) {
      try {
        this.analyserNode.disconnect();
      } catch (e) {
        // May already be disconnected
      }
      this.analyserNode = null;
    }

    this._isConnected = false;
    this._frequencyBuffer = null;
    this._cachedFrequencyData = null;
    this._smoothedBuffer = null;
    this._interpolatedBuffer = null;
    this._tempFrequencyArray = [];
    this.lowFreqAnalyzer.reset();
  }

  /**
   * Reset analyzer state without disconnecting
   */
  reset(): void {
    this._lastUpdateTime = 0;
    this.lowFreqAnalyzer.reset();
    if (this._cachedFrequencyData) {
      this._cachedFrequencyData.fill(0);
    }
    if (this._smoothedBuffer) {
      this._smoothedBuffer.fill(0);
    }
  }
}
