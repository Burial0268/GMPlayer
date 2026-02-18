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
} from './PlayerFunctions';

// Export types
export type {
  SoundOptions,
  SoundEventType,
  SoundEventCallback,
  ISound,
  PlaySongTime,
} from './types';

export type { EffectManagerOptions } from './AudioEffectManager';
export type { LowFreqVolumeOptions } from './LowFreqVolumeAnalyzer';
export type { CrossfadeCurve, CrossfadeParams } from './CrossfadeManager';
export type { TrackAnalysis, VolumeAnalysis, EnergyAnalysis, SpectralFingerprint, AnalyzeOptions, OutroType, OutroAnalysis } from './TrackAnalyzer';
export type { BPMResult } from './BPMDetector';
export type { AutoMixState } from './AutoMixEngine';

// Export classes for advanced usage
export { NativeSound } from './NativeSound';
export { BufferedSound } from './BufferedSound';
export { SoundManager } from './SoundManager';
export { AudioEffectManager } from './AudioEffectManager';
export { LowFreqVolumeAnalyzer } from './LowFreqVolumeAnalyzer';
export { AudioContextManager } from './AudioContextManager';
export { WasmFFTManager } from './WasmFFTManager';

// AutoMix exports
export { CrossfadeManager } from './CrossfadeManager';
export { AutoMixEngine, getAutoMixEngine } from './AutoMixEngine';
export { analyzeTrack, spectralSimilarity, terminateAnalysisWorker } from './TrackAnalyzer';
export { findNearestBeat } from './BPMDetector';
