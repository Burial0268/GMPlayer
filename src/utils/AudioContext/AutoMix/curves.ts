/**
 * Crossfade Curves — Pure math functions for crossfade gain computation
 *
 * Zero dependencies. Extracted from CrossfadeManager.ts.
 * All functions are stateless and can be called from any context.
 */

import type { CrossfadeCurve } from "./types";

/** Share of a fade, at each end, over which its progress eases in and out. */
const FADE_EDGE = 0.06;

/**
 * A fade's progress warped so every curve built on it starts and ends at rest: linear through the
 * middle, with raised-cosine velocity ramps over the first and last `FADE_EDGE`. Monotone, exact
 * at 0 and 1 (the tail mirrors the head). Mirrors `eased_progress` in
 * `audio-backend/src/player/mixer.rs`; keep the two in step.
 */
export function easedProgress(progress: number): number {
  const t = Math.max(0, Math.min(1, progress));
  if (t > 0.5) return 1 - easedProgress(1 - t);
  const speed = 1 / (1 - FADE_EDGE);
  if (t < FADE_EDGE) {
    return speed * (0.5 * t - (FADE_EDGE / (2 * Math.PI)) * Math.sin((Math.PI * t) / FADE_EDGE));
  }
  return speed * (t - 0.5 * FADE_EDGE);
}

/** Smootherstep (C2): at rest at both ends. */
export function smootherstep(progress: number): number {
  const t = Math.max(0, Math.min(1, progress));
  return t * t * t * (t * (t * 6 - 15) + 10);
}

/** `min(x, 1.1)` without the corner: unchanged up to 1, then easing into 1.1 with matching slope. */
function softPowerCap(x: number): number {
  return x <= 1 ? x : 1 + 0.1 * (1 - Math.exp(-(x - 1) / 0.1));
}

/**
 * Generate crossfade curve values at a given progress point (0-1).
 * Returns [outgoingVolume, incomingVolume].
 *
 * inShape/outShape apply a power exponent after the base curve:
 *   <1 = faster transition, >1 = slower transition.
 *   Endpoints (0 and 1) are preserved since 0^n=0 and 1^n=1.
 *
 * For equal-power and S-curve, power normalization is applied after
 * shape exponents to restore the constant-power property (out²+in²=1)
 * that shapes would otherwise break. This prevents volume dips/bumps
 * at the crossfade midpoint while preserving asymmetric ramp feel.
 */
export function getCrossfadeValues(
  progress: number,
  curve: CrossfadeCurve,
  inShape: number = 1,
  outShape: number = 1,
): [number, number] {
  const t = Math.max(0, Math.min(1, progress));

  let outVol: number;
  let inVol: number;

  // Each side from its own end (`f(1 - t)`, not `1 - f(t)`): both warps are symmetric, and this
  // keeps the outgoing side precise near zero, where a shape exponent below 1 amplifies rounding.
  // `linear` and `equalPower` run on the eased progress so they leave and arrive at rest;
  // `sCurve` is smootherstep, already at rest at both ends.
  switch (curve) {
    case "equalPower":
      outVol = Math.sin(easedProgress(1 - t) * Math.PI * 0.5);
      inVol = Math.sin(easedProgress(t) * Math.PI * 0.5);
      break;

    case "sCurve":
      outVol = Math.sin(smootherstep(1 - t) * Math.PI * 0.5);
      inVol = Math.sin(smootherstep(t) * Math.PI * 0.5);
      break;

    case "linear":
    default:
      outVol = easedProgress(1 - t);
      inVol = easedProgress(t);
      break;
  }

  // Apply shape exponents (preserves 0/1 endpoints)
  if (outShape !== 1) outVol = Math.pow(outVol, outShape);
  if (inShape !== 1) inVol = Math.pow(inVol, inShape);

  // Power normalization for constant-power curves (equalPower, sCurve).
  // Shape exponents break cos²+sin²=1, causing volume dips at midpoint.
  // Re-normalize so the total power remains constant throughout.
  if ((outShape !== 1 || inShape !== 1) && (curve === "equalPower" || curve === "sCurve")) {
    const power = outVol * outVol + inVol * inVol;
    if (power > 1e-8) {
      const scale = 1 / Math.sqrt(power);
      outVol *= scale;
      inVol *= scale;
    }
  }

  return [outVol, inVol];
}

/**
 * Convert the normalized crossfade shape into actual GainNode values while
 * keeping overlap power controlled when incoming/outgoing target gains differ.
 * The power target moves on the eased progress, the headroom dip is shaped
 * `sin^2(pi t)` and the lift is soft-capped, so every gain leaves and arrives at
 * rest. Mirrors `balance` in `audio-backend/src/player/mixer.rs`.
 */
export function getBalancedCrossfadeGains(
  progress: number,
  curve: CrossfadeCurve,
  inShape: number,
  outShape: number,
  outgoingTargetGain: number,
  incomingTargetGain: number,
  overlapHeadroomDb: number = 0,
): [number, number] {
  const t = Math.max(0, Math.min(1, progress));
  const [outVol, inVol] = getCrossfadeValues(t, curve, inShape, outShape);

  let outGain = outVol * outgoingTargetGain;
  let inGain = inVol * incomingTargetGain;

  const currentPower = outGain * outGain + inGain * inGain;
  if (currentPower > 1e-8) {
    const w = easedProgress(t);
    const targetPower =
      outgoingTargetGain * outgoingTargetGain * (1 - w) +
      incomingTargetGain * incomingTargetGain * w;
    const powerScale = Math.sqrt(Math.max(0, targetPower) / currentPower);
    const overlapShape = Math.sin(t * Math.PI) ** 2;
    const headroomScale = Math.pow(10, (overlapHeadroomDb * overlapShape) / 20);
    const scale = softPowerCap(powerScale) * headroomScale;

    outGain *= scale;
    inGain *= scale;
  }

  return [outGain, inGain];
}

/**
 * Build a Float32Array representing the gain curve from startProgress to endProgress.
 * Each sample is the gain value at that point in the crossfade.
 */
export function buildCurveArray(
  resolution: number,
  startProgress: number,
  endProgress: number,
  curve: CrossfadeCurve,
  inShape: number,
  outShape: number,
  targetGain: number,
  channel: "outgoing" | "incoming",
): Float32Array {
  const arr = new Float32Array(resolution);
  const range = endProgress - startProgress;
  for (let i = 0; i < resolution; i++) {
    const progress = startProgress + (i / (resolution - 1)) * range;
    const [outVol, inVol] = getCrossfadeValues(progress, curve, inShape, outShape);
    arr[i] = (channel === "outgoing" ? outVol : inVol) * targetGain;
  }
  return arr;
}

/**
 * Build a Float32Array of actual gain values with overlap loudness compensation.
 */
export function buildBalancedCurveArray(
  resolution: number,
  startProgress: number,
  endProgress: number,
  curve: CrossfadeCurve,
  inShape: number,
  outShape: number,
  outgoingTargetGain: number,
  incomingTargetGain: number,
  overlapHeadroomDb: number,
  channel: "outgoing" | "incoming",
): Float32Array {
  const arr = new Float32Array(resolution);
  const range = endProgress - startProgress;
  for (let i = 0; i < resolution; i++) {
    const progress = startProgress + (i / (resolution - 1)) * range;
    const [outGain, inGain] = getBalancedCrossfadeGains(
      progress,
      curve,
      inShape,
      outShape,
      outgoingTargetGain,
      incomingTargetGain,
      overlapHeadroomDb,
    );
    arr[i] = channel === "outgoing" ? outGain : inGain;
  }
  return arr;
}

/** EQ gain in dB at `progress` for a band ramping `fromDb` -> `toDb` on the eased progress. */
export function eqLeanAt(progress: number, fromDb: number, toDb: number): number {
  return fromDb + (toDb - fromDb) * easedProgress(progress);
}

/**
 * Bass-swap EQ gain at `progress`: holds `fromDb` until 40% of the fade, moves to `toDb` by 60% on
 * smootherstep, then holds. A clean handoff at the midpoint instead of two basslines stacking,
 * without the corners a linear ramp has where it starts and stops.
 */
export function bassSwapValueAt(progress: number, fromDb: number, toDb: number): number {
  return fromDb + (toDb - fromDb) * smootherstep((progress - 0.4) / 0.2);
}

/**
 * Sample an EQ gain function over `[startProgress, endProgress]` of the fade, for
 * `setValueCurveAtTime`. Pause and resume take a sub-range of the same function, so a resumed
 * fade continues its curve rather than restarting a new one over the remaining time.
 */
export function buildEqCurve(
  resolution: number,
  startProgress: number,
  endProgress: number,
  valueAt: (progress: number) => number,
): Float32Array {
  const arr = new Float32Array(resolution);
  const range = endProgress - startProgress;
  for (let i = 0; i < resolution; i++) {
    arr[i] = valueAt(startProgress + (i / (resolution - 1)) * range);
  }
  return arr;
}

/** Share of an overlap's vocal windows that makes a side "singing" / "clear". */
const VOCAL_BUSY = 0.5;
const VOCAL_CLEAR = 0.25;

/**
 * Auto-curve exponent scales `[inScale, outScale]` from what overlaps: two
 * vocals hand over quickly, a singer arriving clears the old song first, a
 * quiet opening comes in early and a loud one is held back.
 * Mirrors `pair_shape_scale` in `audio-backend/src/player/automix/plan.rs`;
 * keep the thresholds in step.
 */
export function pairShapeScale(
  vocalOut: number | null,
  vocalIn: number | null,
  energyOut: number | null,
  energyIn: number | null,
): [number, number] {
  let inScale = 1;
  let outScale = 1;
  if (vocalOut !== null && vocalIn !== null) {
    if (vocalOut >= VOCAL_BUSY && vocalIn >= VOCAL_BUSY) {
      inScale *= 1.2;
      outScale *= 1.2;
    } else if (vocalIn >= VOCAL_BUSY && vocalOut < VOCAL_CLEAR) {
      outScale *= 1.2;
    } else if (vocalOut >= VOCAL_BUSY && vocalIn < VOCAL_CLEAR) {
      inScale *= 0.85;
    }
  }
  if (energyOut !== null && energyIn !== null) {
    const ratio = energyIn / Math.max(energyOut, 1e-3);
    if (ratio < 0.5) inScale *= 0.85;
    else if (ratio > 2) inScale *= 1.15;
  }
  return [inScale, outScale];
}
