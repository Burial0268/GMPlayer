/**
 * `@player-helper/audio-analysis` 在 Tauri 构建下的占位实现。
 *
 * 与 backend 占位共用同一套类与错误信息——两者是同一个 WASM 工具链产出的，
 * 行为约定也相同（构造即抛，调用方回落）。见 `disabledAudioBackendWasm`。
 */
export { LFOptionsJs, WasmAudioProcessor, default } from "./disabledAudioBackendWasm";
