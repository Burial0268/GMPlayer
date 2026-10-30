import { isTauri } from "../core/runtime";

declare const __GMPLAYER_TAURI_BUILD__: boolean;

/**
 * Which implementation of the audio-backend protocol is driving playback.
 * Every "is the backend in charge?" question in the player goes through here,
 * so the web-era JS paths (BufferedSound preload, JS analysis, JS AutoMix) are
 * gated by one definition instead of one per file.
 */

/** Rust backend over Tauri IPC. */
export function isNativeAudioBackendAvailable(): boolean {
  return isTauri();
}

/**
 * Same protocol served by WASM + an `<audio>` element. The build flag is read
 * directly so a Tauri bundle drops the WASM transport instead of shipping it.
 */
export function isWasmAudioBackendAvailable(): boolean {
  if (__GMPLAYER_TAURI_BUILD__ || isTauri()) return false;
  return (
    typeof window !== "undefined" && typeof Audio !== "undefined" && typeof Worker !== "undefined"
  );
}

/** Native or WASM: either way a `NativeRustSound` owns the pipeline. */
export function isAudioBackendRuntimeAvailable(): boolean {
  return isNativeAudioBackendAvailable() || isWasmAudioBackendAvailable();
}
