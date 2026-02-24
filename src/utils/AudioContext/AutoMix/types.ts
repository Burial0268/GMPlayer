/**
 * AutoMix Types — All AutoMix-specific type definitions
 *
 * Extracted from AutoMixEngine.ts, CrossfadeManager.ts, and TrackAnalyzer.ts
 * plus new types for transition effects, compatibility scoring, and vocal guard.
 */

import type { OutroType } from './TrackAnalyzer';

// ─── State machine types ──────────────────────────────────────────

export type AutoMixState = 'idle' | 'analyzing' | 'waiting' | 'crossfading' | 'finishing';

export interface CachedAnalysis {
  songId: number;
  analysis: import('./TrackAnalyzer').TrackAnalysis;
}

// ─── Crossfade curve types ────────────────────────────────────────

export type CrossfadeCurve = 'linear' | 'equalPower' | 'sCurve';

export interface CrossfadeParams {
  /** Crossfade duration in seconds */
  duration: number;
  /** Crossfade curve type */
  curve: CrossfadeCurve;
  /** Target gain for incoming track (typically userVolume) */
  incomingGain: number;
  /** Target gain for outgoing track (typically userVolume) */
  outgoingGain: number;
  /** If true, outgoing gain holds steady — for songs with natural fade-outs.
   *  The song's own audio content is already fading, so only the incoming ramps up. */
  fadeInOnly?: boolean;
  /** Classification of the outgoing track's ending. Used by getOutroTypeCrossfadeProfile()
   *  to select per-type curve profiles when autoMixSmartCurve is enabled. */
  outroType?: OutroType;
  /** Exponent for incoming volume curve. <1 = fast rise, >1 = slow rise. Default 1.0 */
  inShape?: number;
  /** Exponent for outgoing volume curve. <1 = fast drop, >1 = slow drop. Default 1.0 */
  outShape?: number;
  /** Persistent gain adjustment for the incoming track (1.0 = no adjustment).
   *  Derived from LUFS normalization: targets -14 LUFS across tracks.
   *  Applied as a flat multiplier on the incoming gain throughout the crossfade
   *  and persisted after completion — no mid-crossfade bumps. */
  incomingGainAdjustment?: number;
  /** Data-driven spectral EQ crossfade. false or undefined to disable. */
  spectralCrossfade?: SpectralCrossfadeData | false;
}

export interface SpectralCrossfadeData {
  /** Target EQ dB adjustments for outgoing track at crossfade end [low, mid, high] */
  outTargetDb: [number, number, number];
  /** Initial EQ dB adjustments for incoming track at crossfade start [low, mid, high] */
  inInitialDb: [number, number, number];
  /** If true, low band uses bass-swap style (midpoint handoff) instead of linear ramp.
   *  Prevents low-end muddiness by holding bass steady until a quick handoff at the midpoint. */
  bassSwapLow?: boolean;
}

export interface CrossfadeProfile {
  curve: CrossfadeCurve;
  fadeInOnly: boolean;
  durationRange?: [number, number];
  /** Exponent for incoming volume curve. <1 = fast rise, >1 = slow rise. Default 1.0 */
  inShape?: number;
  /** Exponent for outgoing volume curve. <1 = fast drop, >1 = slow drop. Default 1.0 */
  outShape?: number;
}

// ─── Transition effects types ─────────────────────────────────────

export interface TransitionEffectConfig {
  /** Enable reverb tail on outgoing track */
  reverbTail: boolean;
  /** Enable noise riser before incoming track */
  noiseRiser: boolean;
  /** Reverb decay time in seconds (1.5-3.0) */
  reverbDecay?: number;
  /** Noise riser duration in seconds (1.0-2.0) */
  riserDuration?: number;
}

// ─── Compatibility scoring types ──────────────────────────────────

export interface CompatibilityScore {
  /** Overall compatibility (0-1, 1 = perfect match) */
  overall: number;
  /** BPM compatibility (0-1), accounts for harmonic ratios */
  bpm: number;
  /** Intensity compatibility (0-1) based on multiband energy */
  intensity: number;
  /** Loudness compatibility (0-1) based on LUFS */
  loudness: number;
  /** Spectral compatibility (0-1) based on fingerprint similarity */
  spectral: number;
}

export interface TransitionStrategy {
  /** Recommended crossfade duration multiplier */
  durationMultiplier: number;
  /** Whether to use transition effects (reverb/riser/filter sweep) */
  useEffects: boolean;
  /** Whether reverb tail is recommended */
  useReverbTail: boolean;
  /** Whether noise riser is recommended */
  useNoiseRiser: boolean;
  /** Enable LPF/HPF frequency sweep for style-different tracks */
  useFilterSweep: boolean;
  /** 0-1, controls cutoff aggressiveness (higher = more aggressive sweep) */
  filterSweepIntensity: number;
  /** Override curve for low compatibility transitions (null = use default) */
  recommendedCurve: CrossfadeCurve | null;
  /** Override shapes for low compatibility transitions (null = use default) */
  shapeOverride: { inShape: number; outShape: number } | null;
}

// ─── Pure lookup function ─────────────────────────────────────────

/**
 * Returns recommended crossfade profile for a given outro type.
 * Used when autoMixSmartCurve is enabled and outroConfidence >= 0.7.
 */
export function getOutroTypeCrossfadeProfile(outroType: OutroType): CrossfadeProfile {
  switch (outroType) {
    case 'hard':
      return { curve: 'equalPower', fadeInOnly: false, durationRange: [2, 3], inShape: 0.85, outShape: 1.2 };
    case 'fadeOut':
      return { curve: 'equalPower', fadeInOnly: true, inShape: 1.15 };
    case 'reverbTail':
      return { curve: 'sCurve', fadeInOnly: false, inShape: 1.2, outShape: 0.9 };
    case 'silence':
      return { curve: 'equalPower', fadeInOnly: false, durationRange: [2, 4], inShape: 0.9 };
    case 'noiseEnd':
      return { curve: 'equalPower', fadeInOnly: false, inShape: 0.9, outShape: 1.15 };
    case 'slowDown':
      return { curve: 'sCurve', fadeInOnly: false, inShape: 1.1 };
    case 'sustained':
      return { curve: 'sCurve', fadeInOnly: false, inShape: 1.15, outShape: 0.95 };
    case 'musicalOutro':
      return { curve: 'equalPower', fadeInOnly: false };
    case 'loopFade':
      return { curve: 'equalPower', fadeInOnly: true };
    default:
      return { curve: 'equalPower', fadeInOnly: false };
  }
}
