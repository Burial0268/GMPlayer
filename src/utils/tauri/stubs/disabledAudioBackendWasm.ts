const DISABLED_WASM_ERROR =
  "WASM audio backend is disabled in Tauri builds; native audio-backend is required.";

function disabledWasm(): never {
  throw new Error(DISABLED_WASM_ERROR);
}

export class DecodedAudioJs {
  constructor() {
    disabledWasm();
  }

  free(): void {}

  [Symbol.dispose](): void {}

  channels(): number {
    disabledWasm();
  }

  duration(): number {
    disabledWasm();
  }

  sampleRate(): number {
    disabledWasm();
  }

  samples(): Float32Array {
    disabledWasm();
  }
}

export class LFOptionsJs {
  bin_count = 0;
  gradient_threshold = 0;
  smoothing_factor = 0;
  window_size = 0;

  constructor() {
    disabledWasm();
  }

  free(): void {}

  [Symbol.dispose](): void {}
}

export class WasmAudioBackend {
  constructor() {
    disabledWasm();
  }

  free(): void {}

  [Symbol.dispose](): void {}

  applyLoadError(): string {
    disabledWasm();
  }

  applyLoadedTrack(): string {
    disabledWasm();
  }

  applyPlayError(): string {
    disabledWasm();
  }

  applyPlayPosition(): string {
    disabledWasm();
  }

  applyPlaybackFinished(): string {
    disabledWasm();
  }

  applyPlaybackState(): string {
    disabledWasm();
  }

  applyVolume(): string {
    disabledWasm();
  }

  decodeAudioBytes(): DecodedAudioJs {
    disabledWasm();
  }

  loadAnalysisBytes(): string {
    disabledWasm();
  }

  processAnalysisFrame(): string {
    disabledWasm();
  }

  sendMessageJson(): string {
    disabledWasm();
  }

  stateJson(): string {
    disabledWasm();
  }

  syncStatusJson(): string {
    disabledWasm();
  }
}

export class WasmAudioProcessor {
  constructor() {
    disabledWasm();
  }

  free(): void {}

  [Symbol.dispose](): void {}

  clear(): void {
    disabledWasm();
  }

  getLFOptions(): LFOptionsJs {
    disabledWasm();
  }

  getLowFreq(): number {
    disabledWasm();
  }

  getRawBins(): Float32Array {
    disabledWasm();
  }

  getSpectrum(): Float32Array {
    disabledWasm();
  }

  isReady(): boolean {
    disabledWasm();
  }

  processFrame(): number {
    disabledWasm();
  }

  pushPCM(): void {
    disabledWasm();
  }

  reset(): void {
    disabledWasm();
  }

  setFreqRange(): void {
    disabledWasm();
  }

  setLFOptions(): void {
    disabledWasm();
  }
}

export default async function init(): Promise<never> {
  disabledWasm();
}
