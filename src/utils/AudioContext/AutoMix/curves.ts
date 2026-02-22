/**
 * Crossfade Curves — Pure math functions for crossfade gain computation
 *
 * Zero dependencies. Extracted from CrossfadeManager.ts.
 * All functions are stateless and can be called from any context.
 */

import type { CrossfadeCurve } from './types';

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
  outShape: number = 1
): [number, number] {
  const t = Math.max(0, Math.min(1, progress));

  let outVol: number;
  let inVol: number;

  switch (curve) {
    case 'linear':
      outVol = 1 - t;
      inVol = t;
      break;

    case 'equalPower': {
      // Equal-power: constant perceived loudness during crossfade
      outVol = Math.cos(t * Math.PI * 0.5);
      inVol = Math.sin(t * Math.PI * 0.5);
      break;
    }

    case 'sCurve': {
      // Smootherstep (6th-order, C2-continuous) → equal-power angle.
      // C2 continuity eliminates the velocity "kink" at t=0/1 that
      // smoothstep has, which matters when automation lanes are dense.
      const s = t * t * t * (t * (t * 6 - 15) + 10);
      const angle = s * Math.PI * 0.5;
      outVol = Math.cos(angle);
      inVol  = Math.sin(angle);
      break;
    }

    default:
      outVol = 1 - t;
      inVol = t;
      break;
  }

  // Apply shape exponents (preserves 0/1 endpoints)
  if (outShape !== 1) outVol = Math.pow(outVol, outShape);
  if (inShape !== 1) inVol = Math.pow(inVol, inShape);

  // Power normalization for constant-power curves (equalPower, sCurve).
  // Shape exponents break cos²+sin²=1, causing volume dips at midpoint.
  // Re-normalize so the total power remains constant throughout.
  if ((outShape !== 1 || inShape !== 1) && (curve === 'equalPower' || curve === 'sCurve')) {
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
  channel: 'outgoing' | 'incoming'
): Float32Array {
  const arr = new Float32Array(resolution);
  const range = endProgress - startProgress;
  for (let i = 0; i < resolution; i++) {
    const progress = startProgress + (i / (resolution - 1)) * range;
    const [outVol, inVol] = getCrossfadeValues(progress, curve, inShape, outShape);
    arr[i] = (channel === 'outgoing' ? outVol : inVol) * targetGain;
  }
  return arr;
}

/**
 * Build a Float32Array for a linear interpolation between two values.
 * Used for EQ gain automation (dB ramps).
 */
export function buildLinearCurve(
  resolution: number,
  startValue: number,
  endValue: number
): Float32Array {
  const arr = new Float32Array(resolution);
  for (let i = 0; i < resolution; i++) {
    arr[i] = startValue + (i / (resolution - 1)) * (endValue - startValue);
  }
  return arr;
}

/**
 * Build a Float32Array for a bass-swap curve: hold start value for 40%,
 * ramp during 40%-60% (midpoint handoff), hold end value for remaining 40%.
 * Used for low-band EQ to prevent bass muddiness during crossfade overlap.
 *
 * Shape:  ▄▄▄▄▄▄▄▅▆▇█████████  (clean handoff at midpoint)
 * vs linear: ─────────────────  (gradual, muddy overlap)
 */
export function buildBassSwapCurve(
  resolution: number,
  startValue: number,
  endValue: number
): Float32Array {
  const arr = new Float32Array(resolution);
  for (let i = 0; i < resolution; i++) {
    const t = i / (resolution - 1);
    if (t < 0.4) {
      arr[i] = startValue;
    } else if (t > 0.6) {
      arr[i] = endValue;
    } else {
      // Linear ramp in the 0.4–0.6 range
      const rampProgress = (t - 0.4) / 0.2;
      arr[i] = startValue + rampProgress * (endValue - startValue);
    }
  }
  return arr;
}

/**
 * Compute the bass-swap curve value at a given progress point.
 * Used by pause/resume to compute intermediate EQ gain for the low band.
 */
export function bassSwapValueAt(progress: number, startValue: number, endValue: number): number {
  if (progress < 0.4) return startValue;
  if (progress > 0.6) return endValue;
  return startValue + ((progress - 0.4) / 0.2) * (endValue - startValue);
}
