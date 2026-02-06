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
  /** Smoothing time constant (0-1). Default: 0.85 */
  smoothingTimeConstant?: number;
  /** Min update interval in ms for getFrequencyData. Default: 16 (~60fps) */
  minUpdateInterval?: number;
}

const DEFAULT_OPTIONS: Required<EffectManagerOptions> = {
  fftSize: 2048,
  smoothingTimeConstant: 0.85,
  minUpdateInterval: 16,
};

const MOBILE_OPTIONS: Required<EffectManagerOptions> = {
  fftSize: 1024,  // Smaller FFT for better performance
  smoothingTimeConstant: 0.85,  // Match AMLL smoothing
  minUpdateInterval: 33,  // ~30fps on mobile
};

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
      this.analyserNode.smoothingTimeConstant = this.options.smoothingTimeConstant;

      // Create bandpass filter for spectrum analysis range (80-2000Hz)
      // Highpass filter: removes frequencies below 20Hz
      this.highpassFilter = this.audioCtx.createBiquadFilter();
      this.highpassFilter.type = 'highpass';
      this.highpassFilter.frequency.value = AudioEffectManager.FREQ_MIN;
      this.highpassFilter.Q.value = 0.7071; // Butterworth response (flat passband)

      // Lowpass filter: removes frequencies above 20000Hz
      this.lowpassFilter = this.audioCtx.createBiquadFilter();
      this.lowpassFilter.type = 'lowpass';
      this.lowpassFilter.frequency.value = AudioEffectManager.FREQ_MAX;
      this.lowpassFilter.Q.value = 0.7071; // Butterworth response

      // Pre-allocate buffers
      const bufferSize = this.analyserNode.frequencyBinCount;
      this._frequencyBuffer = new Uint8Array(bufferSize);
      this._cachedFrequencyData = new Uint8Array(bufferSize);
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
   * Get frequency data for spectrum visualization
   * Uses throttling and buffer reuse for performance
   * @param force Skip throttling check
   * @returns Uint8Array containing frequency data
   */
  getFrequencyData(force: boolean = false): Uint8Array<ArrayBuffer> {
    if (!this.analyserNode || !this._frequencyBuffer) {
      return this._cachedFrequencyData || new Uint8Array(0);
    }

    const now = performance.now();
    const elapsed = now - this._lastUpdateTime;

    // Return cached data if within throttle window (unless forced)
    if (!force && elapsed < this.options.minUpdateInterval && this._cachedFrequencyData) {
      return this._cachedFrequencyData;
    }

    // Update frequency data using reusable buffer
    this.analyserNode.getByteFrequencyData(this._frequencyBuffer);

    // Copy to cached array
    if (this._cachedFrequencyData) {
      this._cachedFrequencyData.set(this._frequencyBuffer);
    }

    this._lastUpdateTime = now;
    return this._frequencyBuffer;
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
   * Uses AMLL-style analysis with boost and floor thresholds
   * @returns Low-frequency volume (typically 0-3 range, passed to renderer's setLowFreqVolume)
   */
  getLowFrequencyVolume(): number {
    if (!this.analyserNode || !this._frequencyBuffer) return 0;

    // Use the already-fetched frequency data to avoid double computation
    this.analyserNode.getByteFrequencyData(this._frequencyBuffer);

    // Convert to number array for analyzer (reuse temp array)
    for (let i = 0; i < this._frequencyBuffer.length; i++) {
      this._tempFrequencyArray[i] = this._frequencyBuffer[i];
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
  }
}
