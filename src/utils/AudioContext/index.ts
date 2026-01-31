/**
 * AudioContext Module - Public API
 *
 * This module provides audio playback functionality with Web Audio API integration,
 * spectrum analysis, and low-frequency volume detection for visual effects.
 */

// Export player functions (main public API)
export {
  createSound,
  setVolume,
  setSeek,
  fadePlayOrPause,
  soundStop,
  processSpectrum,
} from './PlayerFunctions';

// Export types
export type {
  SoundOptions,
  SoundEventType,
  SoundEventCallback,
  ISound,
  LowFreqAnalyzerOptions,
  PlaySongTime,
} from './types';

// Export classes for advanced usage
export { NativeSound } from './NativeSound';
export { BufferedSound } from './BufferedSound';
export { SoundManager } from './SoundManager';
export { AudioEffectManager } from './AudioEffectManager';
export { LowFreqVolumeAnalyzer } from './LowFreqVolumeAnalyzer';
export { AudioContextManager } from './AudioContextManager';
