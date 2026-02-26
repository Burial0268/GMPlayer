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
} from "./PlayerFunctions";

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
export { LowFreqVolumeAnalyzer } from "./LowFreqVolumeAnalyzer";
export { AudioContextManager } from "./AudioContextManager";
export { WasmFFTManager } from "./WasmFFTManager";

// AudioPreloader
export { AudioPreloader, getAudioPreloader } from "./AudioPreloader";

// AutoMix exports (backward-compatible aliases)
export { CrossfadeScheduler as CrossfadeManager } from "./AutoMix";
export { AutoMixEngine, getAutoMixEngine } from "./AutoMix";
export { analyzeTrack, spectralSimilarity, terminateAnalysisWorker } from "./AutoMix";
export { findNearestBeat } from "./AutoMix";
