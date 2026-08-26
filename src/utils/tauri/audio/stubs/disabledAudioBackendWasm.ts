/**
 * Tauri 构建下 WASM 音频模块的占位实现。
 *
 * `vite.config.ts` 在 `TAURI_ENV_PLATFORM` 存在时把 `@player-helper/*` 的两个
 * WASM 包别名到这里：Tauri 里播放与分析都由 native audio-backend 提供，把
 * 几 MB 的 WASM 打进产物没有意义。
 *
 * 构造即抛是**故意**的，调用方据此判定「本构建没有 WASM」并回落：
 * `AudioAnalysisProcessor` 捕获后置空 ctor 不再重试，`AudioEffectManager`
 * 转走 AnalyserNode 路径。所以这个异常出现在日志里不代表功能坏了。
 *
 * 但如果你在**浏览器**里看到它，那就说明部署的 `dist/` 是被一次 Tauri 构建
 * 覆盖过的（`tauri.conf.json` 的 beforeBuildCommand 和 web 共用 `../dist`），
 * 该重新跑一次不带 `TAURI_ENV_PLATFORM` 的 `pnpm build`。错误信息里保留了
 * 这句提示，因为生产构建会剥掉 console，这行文本往往是唯一的线索。
 */
const DISABLED_WASM_ERROR =
  "WASM audio modules are excluded from this build (Tauri uses the native audio-backend). " +
  "Callers fall back to the AnalyserNode path. " +
  "If you are seeing this in a browser, the deployed dist/ was produced by a Tauri build — " +
  "rebuild with a plain `pnpm build`.";

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
