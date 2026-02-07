/**
 * AudioEffectManager - Hybrid analysis: AnalyserNode + WASM FFTPlayer
 *
 * Architecture:
 *   inputNode → AnalyserNode (Blackman window)    → spectrum display (getFrequencyData)
 *   inputNode → AudioWorkletNode (PCM capture)    → WASM FFTPlayer (Hamming window)
 *                                                    → lowFreqVolume (getLowFrequencyVolume)
 *                                                    → detailed FFT (getFFTData)
 *
 * Why hybrid:
 *   - Spectrum bars: AnalyserNode Blackman window gives sharper peaks, better visual contrast
 *   - lowFreqVolume: WASM FFTPlayer Hamming window matches AMLL's native FFTPlayer behavior
 *     (Blackman window cannot reproduce AMLL's official implementation)
 */

import { LowFreqVolumeAnalyzer } from './LowFreqVolumeAnalyzer';
import { AudioContextManager } from './AudioContextManager';
import { WasmFFTManager } from './WasmFFTManager';
import { isPCMWorkletRegistered } from './pcm-capture-worklet';

export interface EffectManagerOptions {
  /** FFT size for AnalyserNode (spectrum display). Default: 2048 */
  fftSize?: number;
  /** AnalyserNode smoothing (0-1). Default: 0.85 */
  smoothingTimeConstant?: number;
  /** Min update interval in ms. Default: 16 (~60fps) */
  minUpdateInterval?: number;
  /** Output size for WASM FFT. Default: 1024 desktop, 512 mobile */
  fftOutputSize?: number;
}

const DEFAULT_OPTIONS: Required<EffectManagerOptions> = {
  fftSize: 2048,
  smoothingTimeConstant: 0.85,
  minUpdateInterval: 16,
  fftOutputSize: 2048,
};

const MOBILE_OPTIONS: Required<EffectManagerOptions> = {
  fftSize: 2048,
  smoothingTimeConstant: 0.85,
  minUpdateInterval: 33,
  fftOutputSize: 1024,
};

// WASM FFTPlayer frequency range (matching AMLL default)
const FREQ_MIN = 80;
const FREQ_MAX = 2000;

/**
 * AudioEffectManager - Hybrid analysis engine
 */
export class AudioEffectManager {
  private audioCtx: AudioContext;
  private analyserNode: AnalyserNode | null = null;
  private lowFreqAnalyzer: LowFreqVolumeAnalyzer;
  private options: Required<EffectManagerOptions>;

  // WASM FFT (for lowFreqVolume + getFFTData)
  private _wasmFFT: WasmFFTManager | null = null;
  private _workletNode: AudioWorkletNode | null = null;

  // AnalyserNode buffers (for spectrum display)
  private _frequencyBuffer: Uint8Array<ArrayBuffer> | null = null;

  // Average amplitude (computed during getFrequencyData)
  private _lastAverage: number = 0;

  // WASM FFT cached spectrum (shared between getFFTData and getLowFrequencyVolume)
  private _cachedWasmSpectrum: number[] = [];
  private _wasmDirty: boolean = false;

  // Throttling for AnalyserNode
  private _lastUpdateTime: number = 0;

  // Connection state
  private _isConnected: boolean = false;

  constructor(audioCtx: AudioContext, options?: EffectManagerOptions) {
    this.audioCtx = audioCtx;

    const baseOptions = AudioContextManager.isMobile() ? MOBILE_OPTIONS : DEFAULT_OPTIONS;
    this.options = { ...baseOptions, ...options };

    this.lowFreqAnalyzer = new LowFreqVolumeAnalyzer();

    this._initNodes();
  }

  private _initNodes(): void {
    try {
      // AnalyserNode for spectrum display (Blackman window — sharp peaks)
      this.analyserNode = this.audioCtx.createAnalyser();
      this.analyserNode.fftSize = this.options.fftSize;
      this.analyserNode.smoothingTimeConstant = this.options.smoothingTimeConstant;

      const bufferSize = this.analyserNode.frequencyBinCount;
      this._frequencyBuffer = new Uint8Array(bufferSize);

      // WASM FFTPlayer for lowFreqVolume + detailed analysis (Hamming window — AMLL native)
      this._wasmFFT = new WasmFFTManager(this.options.fftOutputSize);
      if (!this._wasmFFT.isReady()) {
        console.warn('AudioEffectManager: WasmFFTManager failed to initialize');
      }

      this._cachedWasmSpectrum = new Array(this.options.fftOutputSize).fill(0);
    } catch (err) {
      console.error('AudioEffectManager: Failed to initialize nodes', err);
    }
  }

  /**
   * Connect input node to analysis chains.
   *
   * Audio graph:
   *   inputNode → analyserNode (Blackman, spectrum display)
   *   inputNode → workletNode  (PCM capture → WASM FFTPlayer, lowFreqVolume)
   *
   * @param inputNode The source audio node
   * @returns The same inputNode for chaining
   */
  connect(inputNode: AudioNode): AudioNode {
    if (!this.analyserNode) {
      console.warn('AudioEffectManager: Nodes not initialized');
      return inputNode;
    }

    try {
      // AnalyserNode for spectrum display
      inputNode.connect(this.analyserNode);

      // AudioWorklet for PCM capture → WASM FFT (lowFreqVolume + getFFTData)
      if (isPCMWorkletRegistered() && this._wasmFFT?.isReady()) {
        try {
          this._workletNode = new AudioWorkletNode(this.audioCtx, 'pcm-capture-processor');

          this._workletNode.port.onmessage = (e: MessageEvent<Float32Array>) => {
            if (this._wasmFFT) {
              this._wasmFFT.pushData(e.data, this.audioCtx.sampleRate, 1);
              this._wasmDirty = true;
            }
          };

          inputNode.connect(this._workletNode);
        } catch (err) {
          console.warn('AudioEffectManager: Failed to create AudioWorkletNode', err);
        }
      }

      this._isConnected = true;
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
   * Read WASM FFT spectrum once per frame when new data is available.
   * Shared by getFFTData() and getLowFrequencyVolume().
   */
  private _ensureWasmSpectrumFresh(): void {
    if (!this._wasmDirty) return;
    this._wasmDirty = false;

    if (this._wasmFFT?.isReady()) {
      this._cachedWasmSpectrum = this._wasmFFT.readSpectrum();
    }
  }

  /**
   * Get frequency data from AnalyserNode for spectrum bar display.
   * Uses Blackman window — sharper peaks, better visual contrast for bar visualization.
   * Also computes average amplitude in the same pass.
   * @param force Skip throttling check
   * @returns Uint8Array containing byte frequency data (0-255)
   */
  getFrequencyData(force: boolean = false): Uint8Array<ArrayBuffer> {
    if (!this.analyserNode || !this._frequencyBuffer) {
      return this._frequencyBuffer || new Uint8Array(0);
    }

    const now = performance.now();
    if (!force && now - this._lastUpdateTime < this.options.minUpdateInterval) {
      return this._frequencyBuffer;
    }

    this.analyserNode.getByteFrequencyData(this._frequencyBuffer);

    // Compute average in same pass
    let sum = 0;
    for (let i = 0; i < this._frequencyBuffer.length; i++) {
      sum += this._frequencyBuffer[i];
    }
    this._lastAverage = sum / this._frequencyBuffer.length;

    this._lastUpdateTime = now;
    return this._frequencyBuffer;
  }

  /**
   * Get the average amplitude computed during the last getFrequencyData() call.
   * @returns Average amplitude value (0-255 range)
   */
  getAverageAmplitude(): number {
    return this._lastAverage;
  }

  /**
   * Get FFT data from WASM FFTPlayer (Hamming window, normalized 0-255).
   * Higher precision, with dynamic peak normalization and configurable freq range.
   * @returns number[] of spectrum values (0-255 range)
   */
  getFFTData(): number[] {
    this._ensureWasmSpectrumFresh();
    return this._cachedWasmSpectrum;
  }

  /**
   * Get low frequency volume for background effects.
   * Calculated from WASM FFTPlayer raw magnitudes (Hamming window — matches AMLL native).
   * Uses AMLL FFTToLowPassContext algorithm: log10 amplitude, gradient sliding window,
   * time-delta smoothing.
   *
   * Raw magnitudes are passed directly (not 0-255 normalized) to match AMLL's native
   * FFTPlayer output format, producing values in the 0.x-1.0 range.
   *
   * @returns Smoothed low-frequency volume
   */
  getLowFrequencyVolume(): number {
    this._ensureWasmSpectrumFresh();
    // Pass raw (un-normalized) FFT bins to match AMLL's native FFTPlayer output
    const rawBins = this._wasmFFT?.getRawBins(2);
    if (rawBins) {
      return this.lowFreqAnalyzer.analyze(rawBins);
    }
    return this.lowFreqAnalyzer.analyze(this._cachedWasmSpectrum);
  }

  /**
   * Get current FFT size (AnalyserNode)
   */
  getFFTSize(): number {
    return this.analyserNode?.fftSize || 2048;
  }

  /**
   * Get frequency bin count (AnalyserNode)
   */
  getFrequencyBinCount(): number {
    return this.analyserNode?.frequencyBinCount || 0;
  }

  /**
   * Update FFT size dynamically (affects AnalyserNode spectrum display only)
   */
  setFFTSize(size: number): void {
    if (!this.analyserNode) return;

    const validSize = Math.min(32768, Math.max(32, Math.pow(2, Math.round(Math.log2(size)))));
    this.analyserNode.fftSize = validSize;
    this.options.fftSize = validSize;

    const bufferSize = this.analyserNode.frequencyBinCount;
    this._frequencyBuffer = new Uint8Array(bufferSize);
  }

  /**
   * Disconnect all nodes and cleanup
   */
  disconnect(): void {
    if (this._workletNode) {
      try {
        this._workletNode.port.onmessage = null;
        this._workletNode.disconnect();
      } catch (e) {
        // May already be disconnected
      }
      this._workletNode = null;
    }

    if (this.analyserNode) {
      try {
        this.analyserNode.disconnect();
      } catch (e) {
        // May already be disconnected
      }
      this.analyserNode = null;
    }

    if (this._wasmFFT) {
      this._wasmFFT.free();
      this._wasmFFT = null;
    }

    this._isConnected = false;
    this._frequencyBuffer = null;
    this._cachedWasmSpectrum = [];
    this._lastAverage = 0;
    this.lowFreqAnalyzer.reset();
  }

  /**
   * Reset analyzer state without disconnecting
   */
  reset(): void {
    this._lastUpdateTime = 0;
    this._wasmDirty = false;
    this._lastAverage = 0;
    this.lowFreqAnalyzer.reset();
    if (this._frequencyBuffer) {
      this._frequencyBuffer.fill(0);
    }
    this._cachedWasmSpectrum.fill(0);
    if (this._wasmFFT) {
      this._wasmFFT.reset();
    }
  }
}
