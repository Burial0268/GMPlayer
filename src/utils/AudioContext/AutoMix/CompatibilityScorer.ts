/**
 * CompatibilityScorer — Unified track compatibility scoring
 *
 * Replaces scattered individual checks in AutoMixEngine._finalizeCrossfadeParams()
 * with a unified scoring system. Scores BPM, intensity, loudness, and spectral
 * similarity to produce an overall compatibility score.
 *
 * Used to determine transition strategy (effects, duration adjustments).
 */

import { spectralSimilarity, type TrackAnalysis, type SpectralFingerprint } from "./TrackAnalyzer";
import type { OutroType } from "./TrackAnalyzer";
import type { CompatibilityScore, CrossfadeCurve, TransitionStrategy } from "./types";

const IS_DEV = import.meta.env?.DEV ?? false;

export class CompatibilityScorer {
  /**
   * Score BPM compatibility between two tracks.
   * Handles harmonic ratios (2:1, 3:2) for compatible tempos.
   *
   * @returns 0-1 score (1 = identical or harmonically matched BPM)
   */
  scoreBPM(bpm1: number | null, bpm2: number | null): number {
    if (!bpm1 || !bpm2) return 0.5; // unknown → neutral

    // Check exact and harmonic ratios
    const ratios = [1, 2, 0.5, 1.5, 2 / 3];
    let bestDiff = Infinity;

    for (const ratio of ratios) {
      const adjusted = bpm1 * ratio;
      const diff = Math.abs(adjusted - bpm2) / bpm2;
      if (diff < bestDiff) bestDiff = diff;
    }

    // <5% diff = perfect, >20% diff = poor
    if (bestDiff < 0.05) return 1;
    if (bestDiff > 0.2) return 0;
    return 1 - (bestDiff - 0.05) / 0.15;
  }

  /**
   * Score intensity compatibility based on multiband energy.
   *
   * @returns 0-1 score (1 = similar energy levels)
   */
  scoreIntensity(
    outroMB: { low: number[]; mid: number[]; high: number[] } | undefined,
    introMB: { low: number[]; mid: number[]; high: number[] } | null | undefined,
  ): number {
    if (!outroMB || !introMB) return 0.5; // unknown → neutral

    const outroLen = outroMB.low.length;
    const introLen = introMB.low.length;
    const outroWindows = Math.min(8, outroLen);
    const introWindows = Math.min(8, introLen);
    if (outroWindows < 2 || introWindows < 2) return 0.5;

    let outroE = 0;
    for (let i = outroLen - outroWindows; i < outroLen; i++) {
      outroE += outroMB.low[i] + outroMB.mid[i] + outroMB.high[i];
    }
    outroE /= outroWindows;

    let introE = 0;
    for (let i = 0; i < introWindows; i++) {
      introE += introMB.low[i] + introMB.mid[i] + introMB.high[i];
    }
    introE /= introWindows;

    if (outroE < 0.0001 || introE < 0.0001) return 0.5;

    // Ratio: 1 = identical, diverges as ratio increases
    const ratio = introE / outroE;
    const logRatio = Math.abs(Math.log2(ratio));

    // <0.5 octave = great, >2 octaves = poor
    if (logRatio < 0.5) return 1;
    if (logRatio > 2) return 0;
    return 1 - (logRatio - 0.5) / 1.5;
  }

  /**
   * Score loudness compatibility based on LUFS.
   *
   * @returns 0-1 score (1 = similar loudness)
   */
  scoreLoudness(lufs1: number | undefined, lufs2: number | undefined): number {
    if (lufs1 == null || lufs2 == null) return 0.5;

    const diff = Math.abs(lufs1 - lufs2);
    // <2dB = great, >8dB = poor
    if (diff < 2) return 1;
    if (diff > 8) return 0;
    return 1 - (diff - 2) / 6;
  }

  /**
   * Score spectral compatibility using existing spectralSimilarity function.
   *
   * @returns 0-1 score (1 = identical spectral balance)
   */
  scoreSpectral(
    fp1: SpectralFingerprint | undefined,
    fp2: SpectralFingerprint | undefined,
  ): number {
    if (!fp1 || !fp2) return 0.5;
    return spectralSimilarity(fp1, fp2);
  }

  /**
   * Compute overall compatibility score between current and next track.
   *
   * @returns CompatibilityScore with overall and per-dimension scores
   */
  computeOverall(current: TrackAnalysis | null, next: TrackAnalysis | null): CompatibilityScore {
    if (!current || !next) {
      return { overall: 0.5, bpm: 0.5, intensity: 0.5, loudness: 0.5, spectral: 0.5 };
    }

    const bpm = this.scoreBPM(current.bpm?.bpm ?? null, next.bpm?.bpm ?? null);

    const intensity = this.scoreIntensity(
      current.outro?.multibandEnergy,
      next.intro?.multibandEnergy,
    );

    const loudness = this.scoreLoudness(current.volume.estimatedLUFS, next.volume.estimatedLUFS);

    const spectral = this.scoreSpectral(current.fingerprint, next.fingerprint);

    // Weighted average: spectral and intensity matter most for crossfade quality
    const overall = bpm * 0.15 + intensity * 0.3 + loudness * 0.2 + spectral * 0.35;

    const score = { overall, bpm, intensity, loudness, spectral };

    if (IS_DEV) {
      console.log(
        `CompatibilityScorer: overall=${overall.toFixed(2)} ` +
          `(bpm=${bpm.toFixed(2)}, intensity=${intensity.toFixed(2)}, ` +
          `loudness=${loudness.toFixed(2)}, spectral=${spectral.toFixed(2)})`,
      );
    }

    return score;
  }

  /**
   * Compute transition strategy based on compatibility score and outro type.
   *
   * @returns TransitionStrategy with recommendations for effects and duration
   */
  computeTransitionStrategy(
    score: CompatibilityScore,
    outroType: OutroType | null,
  ): TransitionStrategy {
    const strategy: TransitionStrategy = {
      durationMultiplier: 1,
      useEffects: false,
      useReverbTail: false,
      useNoiseRiser: false,
      useFilterSweep: false,
      filterSweepIntensity: 0,
      recommendedCurve: null,
      shapeOverride: null,
    };

    // Duration adjustment based on compatibility
    // Similar tracks → shorter crossfade (0.85x), different → longer (1.3x)
    strategy.durationMultiplier = 0.85 + (1 - score.overall) * 0.45;

    // Reverb tail for hard endings, sustained, musical outro with sufficient energy
    if (outroType === "hard" || outroType === "musicalOutro" || outroType === "sustained") {
      strategy.useReverbTail = true;
      strategy.useEffects = true;
    }

    // Noise riser for low-compatibility transitions to mask the jarring change
    if (score.overall < 0.4) {
      strategy.useNoiseRiser = true;
      strategy.useEffects = true;
    }

    // Filter sweep for spectrally or overall incompatible tracks
    if (score.spectral < 0.35 || score.overall < 0.3) {
      strategy.useFilterSweep = true;
      strategy.useEffects = true;
      // Intensity: lower compat → more aggressive sweep
      strategy.filterSweepIntensity = Math.min(1, Math.max(0, 1 - score.spectral * 2));
      // Filter sweep benefits from reverb tail (full-spectrum reverb creates "receding" effect)
      strategy.useReverbTail = true;
    }

    // Curve override for very low compatibility (sCurve is smoother)
    if (score.overall < 0.3) {
      strategy.recommendedCurve = "sCurve" as CrossfadeCurve;
      strategy.shapeOverride = { inShape: 1.15, outShape: 0.95 };
    }

    if (IS_DEV && strategy.useEffects) {
      console.log(
        `CompatibilityScorer: Strategy — durationMul=${strategy.durationMultiplier.toFixed(2)}, ` +
          `reverbTail=${strategy.useReverbTail}, noiseRiser=${strategy.useNoiseRiser}` +
          (strategy.useFilterSweep
            ? `, filterSweep=true, intensity=${strategy.filterSweepIntensity.toFixed(2)}`
            : "") +
          (strategy.recommendedCurve ? `, curve=${strategy.recommendedCurve}` : ""),
      );
    }

    return strategy;
  }
}
