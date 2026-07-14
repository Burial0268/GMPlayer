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
} from "./NativeQueuePrefill";

// Export types
export type {
  SoundOptions,
  SoundEventType,
  SoundEventCallback,
  ISound,
  PlaySongTime,
} from "./types";

export type { EffectManagerOptions } from "./AudioEffectManager";
export type { LowFreqVolumeOptions } from "./LowFreqVolumeAnalyzer";

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
export { NativeSound } from "./NativeSound";
export { BufferedSound } from "./BufferedSound";
export { SoundManager } from "./SoundManager";
export { AudioEffectManager } from "./AudioEffectManager";
export { AudioAnalysisProcessor } from "./AudioAnalysisProcessor";
export { LowFreqVolumeAnalyzer } from "./LowFreqVolumeAnalyzer";
export { AudioContextManager } from "./AudioContextManager";
export {
  clearSpectrumFrame,
  getSpectrumFrame,
  getSpectrumScale,
  getSpectrumVersion,
  setSpectrumFrame,
} from "./SpectrumFrame";
export type { SpectrumFrame } from "./SpectrumFrame";

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
