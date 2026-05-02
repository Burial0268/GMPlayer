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

import { AudioAnalysisProcessor } from "./AudioAnalysisProcessor";
import { LowFreqVolumeAnalyzer } from "./LowFreqVolumeAnalyzer";
import type { LowFreqVolumeOptions } from "./LowFreqVolumeAnalyzer";
import { AudioContextManager } from "./AudioContextManager";
import { isPCMWorkletRegisteredFor } from "./pcm-capture-worklet";

export interface EffectManagerOptions {
  /** FFT size for AnalyserNode (spectrum display). Default: 2048 */
  fftSize?: number;
  /** AnalyserNode smoothing (0-1). Default: 0.85 */
  smoothingTimeConstant?: number;
  /** Min update interval in ms. Default: 16 (~60fps) */
  minUpdateInterval?: number;
  /** Output size for WASM FFT. Default: 1024 desktop, 512 mobile */
  fftOutputSize?: number;
  /** WASM FFT min frequency (Hz). Default: 80 */
  freqMin?: number;
  /** WASM FFT max frequency (Hz). Default: 2500 */
  freqMax?: number;
  /** Number of raw bins to aggregate for lowFreqVolume. Default: 2 */
  lowFreqBinCount?: number;
  /** LowFreqVolumeAnalyzer options */
  lowFreqOptions?: LowFreqVolumeOptions;
}

const DEFAULT_OPTIONS: Required<EffectManagerOptions> = {
  fftSize: 2048,
  smoothingTimeConstant: 0.85,
  minUpdateInterval: 16,
  fftOutputSize: 2048,
  // Lower freqMin so the AMLL FrequencyLimit::Range includes the actual
  // sub-bass region. With 2048-pt FFT @ 44100 Hz, bin 2 (43 Hz) is the
  // first bin above 40 Hz. 4 raw_bins × ~18 Hz/bin = 43–115 Hz.
  freqMin: 40,
  freqMax: 2400,
  lowFreqBinCount: 4,
  lowFreqOptions: {},
};

const MOBILE_OPTIONS: Required<EffectManagerOptions> = {
  fftSize: 2048,
  smoothingTimeConstant: 0.85,
  minUpdateInterval: 33,
  fftOutputSize: 1024,
  freqMin: 76,
  freqMax: 2400,
  lowFreqBinCount: 4,
  lowFreqOptions: {
    // Lower threshold for AnalyserNode fallback: byte frequency data is dB-scaled,
    // so amplitudeToLevel (another log) compresses the dynamic range.
    // Default 0.35 almost never triggers; 0.1 restores punchy bass detection.
    gradientThreshold: 0.1,
  },
};

const EMPTY_U8 = new Uint8Array(0);

/**
 * AudioEffectManager - Hybrid analysis engine
 */
export class AudioEffectManager {
  private audioCtx: AudioContext;
  private analyserNode: AnalyserNode | null = null;
  private options: Required<EffectManagerOptions>;

  // WASM audio analysis: FFT + peak normalization + lowFreq (replaces WasmFFTManager)
  private _analysisProc: AudioAnalysisProcessor | null = null;
  private _workletNode: AudioWorkletNode | null = null;

  // AnalyserNode buffers (for spectrum display)
  private _frequencyBuffer: Uint8Array<ArrayBuffer> | null = null;

  // Average amplitude (computed during getFrequencyData)
  private _lastAverage: number = 0;

  // AnalyserNode fallback for mobile lowFreqVolume (avoids WASM overhead)
  private _analyserLowFreqBins: number[] = [0, 0, 0, 0];
  private _fallbackAnalyzer: LowFreqVolumeAnalyzer;

  // Throttling for AnalyserNode
  private _lastUpdateTime: number = 0;

  // Seek guard: blocks stale PCM data from AudioWorklet during seek transition
  private _seekPending: boolean = false;

  // Connection state
  private _isConnected: boolean = false;

  constructor(audioCtx: AudioContext, options?: EffectManagerOptions) {
    this.audioCtx = audioCtx;

    const baseOptions = AudioContextManager.isMobile() ? MOBILE_OPTIONS : DEFAULT_OPTIONS;
    this.options = { ...baseOptions, ...options };

    this._fallbackAnalyzer = new LowFreqVolumeAnalyzer({
      binCount: this.options.lowFreqBinCount,
      ...this.options.lowFreqOptions,
    });

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

      // WASM audio analysis for lowFreqVolume + detailed FFT (Hamming window)
      // Skip on mobile: AudioWorklet + WASM causes audio glitches.
      // Mobile uses AnalyserNode fallback for lowFreqVolume instead.
      if (!AudioContextManager.isMobile()) {
        this._analysisProc = new AudioAnalysisProcessor(
          this.options.fftOutputSize,
          this.options.freqMin,
          this.options.freqMax,
          this.options.lowFreqBinCount,
          this.options.lowFreqOptions?.windowSize,
          this.options.lowFreqOptions?.gradientThreshold,
          this.options.lowFreqOptions?.smoothingFactor,
        );
      }
    } catch (err) {
      console.error("AudioEffectManager: Failed to initialize nodes", err);
    }
  }

  /**
   * Connect input node to analysis chains.
   *
   * Audio graph:
   *   inputNode → analyserNode (Blackman, spectrum display)
   *   inputNode → workletNode  (PCM capture → WASM FFTPlayer, lowFreqVolume)
   */
  connect(inputNode: AudioNode): AudioNode {
    if (!this.analyserNode) {
      console.warn("AudioEffectManager: Nodes not initialized");
      return inputNode;
    }

    try {
      // AnalyserNode for spectrum display
      inputNode.connect(this.analyserNode);

      // AudioWorklet for PCM capture → WASM FFT (lowFreqVolume + getFFTData)
      if (
        !AudioContextManager.isMobile() &&
        isPCMWorkletRegisteredFor(this.audioCtx) &&
        this._analysisProc?.isReady()
      ) {
        try {
          this._workletNode = new AudioWorkletNode(this.audioCtx, "pcm-capture-processor");

          this._workletNode.port.onmessage = (e: MessageEvent<Float32Array>) => {
            if (this._seekPending) return;
            if (this._analysisProc) {
              this._analysisProc.pushData(e.data, this.audioCtx.sampleRate, 1);
            }
          };

          inputNode.connect(this._workletNode);
        } catch (err) {
          console.warn("AudioEffectManager: Failed to create AudioWorkletNode", err);
        }
      }

      this._isConnected = true;
      return inputNode;
    } catch (err) {
      console.error("AudioEffectManager: Failed to connect", err);
      return inputNode;
    }
  }

  isConnected(): boolean {
    return this._isConnected;
  }

  /**
   * Process audio frame via WASM (FFT + lowFreq analysis).
   * Called once per RAF frame — time-delta smoothing runs continuously.
   */
  private _ensureFresh(): void {
    this._analysisProc?.ensureFresh();
  }

  /**
   * Get frequency data from AnalyserNode for spectrum bar display.
   */
  getFrequencyData(force: boolean = false): Uint8Array<ArrayBuffer> {
    const buffer = this._frequencyBuffer;
    const analyser = this.analyserNode;
    if (!analyser || !buffer) return EMPTY_U8 as Uint8Array<ArrayBuffer>;

    const now = performance.now();
    if (!force && now - this._lastUpdateTime < this.options.minUpdateInterval) {
      return buffer;
    }

    analyser.getByteFrequencyData(buffer);

    // Compute average in same pass
    let sum = 0;
    const len = buffer.length;
    for (let i = 0; i < len; i++) {
      sum += buffer[i];
    }
    this._lastAverage = sum / len;

    this._lastUpdateTime = now;
    return buffer;
  }

  getAverageAmplitude(): number {
    return this._lastAverage;
  }

  /**
   * Get FFT data from WASM audio analysis (Hamming window, normalized 0-255).
   */
  getFFTData(): number[] {
    this._ensureFresh();
    return this._analysisProc?.getSpectrum() ?? [];
  }

  /**
   * Get low frequency volume for background effects.
   *
   * Desktop (WASM): FFT + lowFreq analysis runs in Rust via processFrame.
   * Mobile (fallback): AnalyserNode byte frequency data via LowFreqVolumeAnalyzer.
   */
  getLowFrequencyVolume(): number {
    // Mobile or no worklet: use AnalyserNode byte frequency fallback
    if (!this._workletNode) {
      return this._getLowFreqFromAnalyser();
    }

    this._ensureFresh();
    return this._analysisProc?.getLowFrequencyVolume() ?? 0;
  }

  /**
   * Fallback: compute low-frequency volume from AnalyserNode byte frequency data.
   * Used on mobile to avoid AudioWorklet + WASM FFT overhead.
   */
  private _getLowFreqFromAnalyser(): number {
    if (!this._frequencyBuffer || this._frequencyBuffer.length === 0) {
      return this._fallbackAnalyzer.analyze(new Uint8Array(0));
    }

    const binCount = Math.min(this.options.lowFreqBinCount, this._frequencyBuffer.length);
    if (this._analyserLowFreqBins.length !== binCount) {
      this._analyserLowFreqBins = Array.from({ length: binCount }, () => 0);
    }
    for (let i = 0; i < binCount; i++) {
      this._analyserLowFreqBins[i] = this._frequencyBuffer[i] * 50;
    }

    return this._fallbackAnalyzer.analyze(this._analyserLowFreqBins);
  }

  getFFTSize(): number {
    return this.analyserNode?.fftSize || 2048;
  }

  getFrequencyBinCount(): number {
    return this.analyserNode?.frequencyBinCount || 0;
  }

  setFFTSize(size: number): void {
    if (!this.analyserNode) return;
    const validSize = Math.min(32768, Math.max(32, Math.pow(2, Math.round(Math.log2(size)))));
    this.analyserNode.fftSize = validSize;
    this.options.fftSize = validSize;
    const bufferSize = this.analyserNode.frequencyBinCount;
    this._frequencyBuffer = new Uint8Array(bufferSize);
  }

  setFreqRange(min: number, max: number): void {
    this.options.freqMin = min;
    this.options.freqMax = max;
    this._analysisProc?.setFreqRange(min, max);
  }

  getFreqRange(): { min: number; max: number } {
    return { min: this.options.freqMin, max: this.options.freqMax };
  }

  setLowFreqOptions(options: Partial<LowFreqVolumeOptions>): void {
    if (options.binCount !== undefined) this.options.lowFreqBinCount = options.binCount;
    this._fallbackAnalyzer.setOptions(options);
    this._analysisProc?.setLFOptions(options);
  }

  getLowFreqOptions(): Required<LowFreqVolumeOptions> {
    return this._fallbackAnalyzer.getOptions();
  }

  disconnect(): void {
    if (this._workletNode) {
      try {
        this._workletNode.port.onmessage = null;
        this._workletNode.disconnect();
      } catch { /* already disconnected */ }
      this._workletNode = null;
    }

    if (this.analyserNode) {
      try { this.analyserNode.disconnect(); } catch { /* already disconnected */ }
      this.analyserNode = null;
    }

    if (this._analysisProc) {
      this._analysisProc.free();
      this._analysisProc = null;
    }

    this._isConnected = false;
    this._frequencyBuffer = null;
    this._lastAverage = 0;
    this._fallbackAnalyzer.reset();
  }

  reset(): void {
    this._lastUpdateTime = 0;
    this._lastAverage = 0;
    this._fallbackAnalyzer.reset();
    if (this._frequencyBuffer) {
      this._frequencyBuffer.fill(0);
    }
    this._analysisProc?.reset();
  }

  clearFFTState(): void {
    this._seekPending = true;
    this._lastAverage = 0;
    this._fallbackAnalyzer.reset();
    this._analysisProc?.clearQueue();
  }

  onSeeked(): void {
    if (!this._seekPending) return;
    this._analysisProc?.clearQueue();
    this._seekPending = false;
  }
}
