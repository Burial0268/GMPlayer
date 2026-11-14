/**
 * AudioContext Module - Public API
 *
 * This module provides audio playback functionality with Web Audio API integration,
 * spectrum analysis, low-frequency volume detection for visual effects,
 * and AutoMix crossfade transitions.
 */

// Export player functions (main public API)
export {
  createSound,
  setVolume,
  setSeek,
  fadePlayOrPause,
  soundStop,
  seekActiveSound,
  stopActiveSound,
  processSpectrum,
  adoptIncomingSound,
  setPageVisible,
  ensureSpectrumUpdate,
  isNativeAdvanceHoldActiveFor,
} from "./PlayerFunctions";

// Native queue-window prefill (Tauri background advance)
export {
  prefillNativeQueue,
  cancelNativeQueuePrefill,
  getNativeQueueRegistryEntry,
} from "./native/NativeQueuePrefill";

// Native manifest planner (Tauri unbounded background advance)
export {
  publishNativeManifest,
  clearNativeManifest,
  requestNativeReshuffle,
} from "./native/NativeManifestPublisher";
export {
  syncNativeResolverConfig,
  invalidateNativeResolverConfig,
} from "./native/NativeResolverConfigSync";

// Session controls (play mode / favourite) shared with the OS media session
export {
  publishSessionControls,
  invalidateSessionControls,
  installSessionControlsSubscriber,
  isAdoptingSessionControls,
  requestNextPlayMode,
  requestToggleFavourite,
} from "./native/NativeSessionControlsSync";

// Boot-time reconciliation with a backend that outlived the WebView
export { adoptNativeBackendSession } from "./native/NativeSessionAdopt";
export type { AdoptedBackendSession } from "./native/NativeSessionAdopt";

// Listen-together keepalive hand-off (Tauri only)
export { setNativeListenTogetherRoom } from "./native/NativeListenTogetherSync";

// Export types
export type {
  SoundOptions,
  SoundEventType,
  SoundEventCallback,
  SoundEventMap,
  ISound,
  BackendSound,
} from "./sound/types";
export { isBackendSound } from "./sound/types";

export type { EffectManagerOptions } from "./analysis/AudioEffectManager";
export type { LowFreqVolumeOptions } from "./analysis/LowFreqVolumeAnalyzer";

// AutoMix types (from new AutoMix sub-module)
export type { CrossfadeCurve, CrossfadeParams } from "./AutoMix";
export type {
  TrackAnalysis,
  VolumeAnalysis,
  EnergyAnalysis,
  SpectralFingerprint,
  AnalyzeOptions,
  OutroType,
  OutroAnalysis,
} from "./AutoMix";
export type { BPMResult } from "./AutoMix";
export type { AutoMixState } from "./AutoMix";

// Export classes for advanced usage
export { NativeSound } from "./sound/NativeSound";
export { BufferedSound } from "./sound/BufferedSound";
export { SoundManager } from "./sound/SoundManager";
export { AudioEffectManager } from "./analysis/AudioEffectManager";
export { AudioAnalysisProcessor } from "./analysis/AudioAnalysisProcessor";
export { LowFreqVolumeAnalyzer } from "./analysis/LowFreqVolumeAnalyzer";
export { AudioContextManager } from "./AudioContextManager";
export {
  clearSpectrumFrame,
  getSpectrumFrame,
  getSpectrumVersion,
  setSpectrumFrame,
} from "./analysis/SpectrumFrame";
export type { SpectrumFrame } from "./analysis/SpectrumFrame";

// AudioPreloader
export { AudioPreloader, getAudioPreloader } from "./AudioPreloader";

// Unified song URL resolution
export { resolveSongUrl } from "./resolveSongUrl";
export type { SongUrlInput, ResolveSongUrlResult, ResolveSongUrlOptions } from "./resolveSongUrl";

// AutoMix exports (backward-compatible aliases)
export { CrossfadeScheduler as CrossfadeManager } from "./AutoMix";
export { AutoMixEngine, getAutoMixEngine } from "./AutoMix";
export { analyzeTrack, spectralSimilarity, terminateAnalysisWorker } from "./AutoMix";
export { findNearestBeat } from "./AutoMix";
export { activeSound } from "./sound/SoundManager";
