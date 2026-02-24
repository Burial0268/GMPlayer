/**
 * AutoMix Module — Barrel export
 *
 * Re-exports all public symbols from the AutoMix sub-modules.
 */

// Service facade (singleton)
export { AutoMixService, getAutoMixEngine } from "./AutoMixService";

// Backward-compatible alias
export { AutoMixService as AutoMixEngine } from "./AutoMixService";

// Types
export type {
  AutoMixState,
  CachedAnalysis,
  CrossfadeCurve,
  CrossfadeParams,
  CrossfadeProfile,
  SpectralCrossfadeData,
  TransitionEffectConfig,
  CompatibilityScore,
  TransitionStrategy,
} from "./types";
export { getOutroTypeCrossfadeProfile } from "./types";

// Curves (pure math)
export {
  getCrossfadeValues,
  buildCurveArray,
  buildLinearCurve,
  buildBassSwapCurve,
  bassSwapValueAt,
} from "./curves";

// Sub-modules (for advanced usage)
export { CrossfadeScheduler } from "./CrossfadeScheduler";
export { SpectralEQ } from "./SpectralEQ";
export { PreBufferManager } from "./PreBufferManager";
export { TransitionStateMachine } from "./TransitionStateMachine";
export { VocalActivityGuard } from "./VocalActivityGuard";
export { CompatibilityScorer } from "./CompatibilityScorer";
export { TransitionEffects } from "./TransitionEffects";

// TrackAnalyzer and friends
export { analyzeTrack, spectralSimilarity, terminateAnalysisWorker } from "./TrackAnalyzer";
export type {
  TrackAnalysis,
  VolumeAnalysis,
  EnergyAnalysis,
  SpectralFingerprint,
  AnalyzeOptions,
  OutroType,
  OutroAnalysis,
  IntroAnalysis,
} from "./TrackAnalyzer";

// BPM
export { findNearestBeat } from "./BPMDetector";
export type { BPMResult } from "./BPMDetector";
