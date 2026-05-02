/**
 * AudioAnalysisProcessor — WASM wrapper replacing WasmFFTManager + LowFreqVolumeAnalyzer
 *
 * Single WASM call per RAF frame: processFrame() runs FFT + dynamic peak normalization
 * + raw bin aggregation + amplitude-to-level + gradient + time-delta smoothing.
 *
 * Pipeline:
 *   AudioWorklet PCM → pushData() → [WASM ring buffer]
 *   RAF → ensureFresh() → processFrame(delta, outputBuf) → cached spectrum + lowFreq
 *
 * WASM loading: the module is pre-loaded at module evaluation time via dynamic import.
 * _loadWasm() fires immediately (before any AudioEffectManager is constructed). By the
 * time the AudioContext is ready and the AudioWorklet starts sending PCM, the WASM module
 * is typically already initialized. If it isn't, pushData/ensureFresh silently drop data
 * until WASM is ready — the AnalyserNode fallback handles the interim.
 */

import type { LowFreqVolumeOptions } from "./LowFreqVolumeAnalyzer";

// ── WASM module pre-loading ────────────────────────────────────────

interface WasmProcessor {
  pushPCM(sampleRate: number, samples: Float32Array): void;
  processFrame(deltaMs: number, spectrum: Float32Array): number;
  getRawBins(count: number): Float32Array;
  setFreqRange(min: number, max: number): void;
  setLFOptions(bc: number | null, ws: number | null, th: number | null, sf: number | null): void;
  isReady(): boolean;
  reset(): void;
  clear(): void;
  free(): void;
}

type WasmProcessorCtor = new (
  outputSize: number,
  freqMin: number,
  freqMax: number,
  binCount: number,
  windowSize: number,
  gradientThreshold: number,
  smoothingFactor: number,
) => WasmProcessor;

let _WasmCtor: WasmProcessorCtor | null = null;
let _wasmReadyPromise: Promise<void> | null = null;

/**
 * Ensure the WASM module is loaded and ready.
 * Must be awaited before creating AudioEffectManager instances so the
 * AudioWorklet connection path can use the WASM processor immediately.
 */
export async function ensureWasmReady(): Promise<void> {
  if (_WasmCtor) return;
  if (!_wasmReadyPromise) {
    _wasmReadyPromise = (async () => {
      try {
        const mod = await import("@player-helper/audio-analysis");
        _WasmCtor = mod.WasmAudioProcessor as unknown as WasmProcessorCtor;
      } catch (err) {
        console.error("AudioAnalysisProcessor: WASM module failed to load", err);
      }
    })();
  }
  await _wasmReadyPromise;
}

// ── Public API ──────────────────────────────────────────────────────

export class AudioAnalysisProcessor {
  private _proc: WasmProcessor | null = null;
  private _outputSize: number;
  private _outputBuf: Float32Array;
  private _rawBinCount: number;

  // Cached results from last processFrame
  private _cachedSpectrum: number[] = [];
  private _cachedLowFreq: number = 1;

  // Delta tracking
  private _lastTime: number = 0;

  // Dirty flag: set by pushData, consumed by ensureFresh
  private _dirty: boolean = false;

  private _lfOptions: Required<LowFreqVolumeOptions>;

  constructor(
    outputSize: number = 1024,
    freqMin: number = 80,
    freqMax: number = 2000,
    binCount: number = 2,
    windowSize: number = 10,
    gradientThreshold: number = 0.35,
    smoothingFactor: number = 0.003,
  ) {
    this._outputSize = outputSize;
    this._outputBuf = new Float32Array(outputSize);
    this._rawBinCount = binCount;
    this._lfOptions = { binCount, windowSize, gradientThreshold, smoothingFactor };
    this._cachedSpectrum = Array.from({ length: outputSize }, () => 0);

    this._tryCreateProc(outputSize, freqMin, freqMax, binCount, windowSize, gradientThreshold, smoothingFactor);
  }

  private _tryCreateProc(
    outputSize: number,
    freqMin: number,
    freqMax: number,
    binCount: number,
    windowSize: number,
    gradientThreshold: number,
    smoothingFactor: number,
  ): void {
    if (!_WasmCtor) return;
    try {
      this._proc = new _WasmCtor(
        outputSize, freqMin, freqMax,
        binCount, windowSize, gradientThreshold, smoothingFactor,
      );
    } catch (err) {
      console.error("AudioAnalysisProcessor: Failed to create processor", err);
      _WasmCtor = null; // Prevent future retries
    }
  }

  pushData(pcm: Float32Array, sampleRate: number, _channels: number): void {
    if (!this._proc) {
      // WASM not ready yet — try again (ctor may have completed since construction)
      this._tryCreateProc(
        this._outputSize, 80, 2000,  // freq defaults, will be overridden if setFreqRange was called
        this._rawBinCount,
        this._lfOptions.windowSize,
        this._lfOptions.gradientThreshold,
        this._lfOptions.smoothingFactor,
      );
      if (!this._proc) return; // Still not ready, drop this PCM block
    }
    this._proc.pushPCM(sampleRate, pcm);
    this._dirty = true;
  }

  ensureFresh(): void {
    if (!this._proc) return;

    const now = performance.now();
    const delta = this._lastTime > 0 ? now - this._lastTime : 16;
    this._lastTime = now;

    const lowFreq = this._proc.processFrame(delta, this._outputBuf);
    this._cachedLowFreq = lowFreq;

    if (this._dirty) {
      this._cachedSpectrum = Array.from(this._outputBuf);
      this._dirty = false;
    }
  }

  getSpectrum(): number[] {
    return this._cachedSpectrum;
  }

  getLowFrequencyVolume(): number {
    return this._cachedLowFreq;
  }

  getRawBins(count: number): number[] {
    if (!this._proc) return Array.from({ length: count }, () => 0);
    if (count !== this._rawBinCount) {
      this._rawBinCount = count;
    }
    return Array.from(this._proc.getRawBins(count));
  }

  isReady(): boolean {
    return this._proc !== null;
  }

  setFreqRange(min: number, max: number): void {
    this._proc?.setFreqRange(min, max);
  }

  setLFOptions(options: Partial<LowFreqVolumeOptions>): void {
    if (options.binCount !== undefined) this._lfOptions.binCount = options.binCount;
    if (options.windowSize !== undefined) this._lfOptions.windowSize = options.windowSize;
    if (options.gradientThreshold !== undefined)
      this._lfOptions.gradientThreshold = options.gradientThreshold;
    if (options.smoothingFactor !== undefined)
      this._lfOptions.smoothingFactor = options.smoothingFactor;

    this._proc?.setLFOptions(
      options.binCount ?? null,
      options.windowSize ?? null,
      options.gradientThreshold ?? null,
      options.smoothingFactor ?? null,
    );
  }

  getLFOptions(): Required<LowFreqVolumeOptions> {
    return { ...this._lfOptions };
  }

  reset(): void {
    this._lastTime = 0;
    this._dirty = false;
    this._cachedSpectrum.fill(0);
    this._cachedLowFreq = 0;
    this._proc?.reset();
  }

  clearQueue(): void {
    this._lastTime = 0;
    this._dirty = false;
    this._cachedSpectrum.fill(0);
    this._cachedLowFreq = 0;
    this._proc?.clear();
  }

  free(): void {
    this._proc?.free();
    this._proc = null;
  }
}
