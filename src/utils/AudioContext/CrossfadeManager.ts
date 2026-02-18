/**
 * CrossfadeManager - GainNode crossfade curves and scheduling
 *
 * Manages the volume crossfade between outgoing and incoming sounds.
 * Supports equal-power, linear, and S-curve crossfade profiles.
 */

import { AudioContextManager } from './AudioContextManager';
import type { OutroType } from './TrackAnalyzer';

const IS_DEV = import.meta.env?.DEV ?? false;

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
  /** Loudness compensation factor for mid-crossfade balance (1.0 = no compensation).
   *  Applied as bell-shaped envelope: comp(t) = 1 + (factor-1) × sin(πt),
   *  peaks at midpoint, 1.0 at both ends — no boundary discontinuity. */
  loudnessCompensation?: number;
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

/**
 * Returns recommended crossfade profile for a given outro type.
 * Used when autoMixSmartCurve is enabled and outroConfidence >= 0.7.
 */
export function getOutroTypeCrossfadeProfile(outroType: OutroType): CrossfadeProfile {
  switch (outroType) {
    case 'hard':
      return { curve: 'equalPower', fadeInOnly: false, durationRange: [2, 3], inShape: 0.7, outShape: 1.5 };
    case 'fadeOut':
      return { curve: 'equalPower', fadeInOnly: true, inShape: 1.3 };
    case 'reverbTail':
      return { curve: 'sCurve', fadeInOnly: false, inShape: 1.5, outShape: 0.8 };
    case 'silence':
      return { curve: 'equalPower', fadeInOnly: false, durationRange: [2, 4], inShape: 0.8 };
    case 'noiseEnd':
      return { curve: 'equalPower', fadeInOnly: false, inShape: 0.8, outShape: 1.3 };
    case 'slowDown':
      return { curve: 'sCurve', fadeInOnly: false, inShape: 1.2 };
    case 'sustained':
      return { curve: 'sCurve', fadeInOnly: false, inShape: 1.3, outShape: 0.9 };
    case 'musicalOutro':
      return { curve: 'equalPower', fadeInOnly: false };
    case 'loopFade':
      return { curve: 'equalPower', fadeInOnly: true };
    default:
      return { curve: 'equalPower', fadeInOnly: false };
  }
}

/**
 * Generate crossfade curve values at a given progress point (0-1).
 * Returns [outgoingVolume, incomingVolume].
 *
 * inShape/outShape apply a power exponent after the base curve:
 *   <1 = faster transition, >1 = slower transition.
 *   Endpoints (0 and 1) are preserved since 0^n=0 and 1^n=1.
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

  return [outVol, inVol];
}

/**
 * CrossfadeManager - Schedules and manages GainNode crossfades
 */
export class CrossfadeManager {
  private _outgoingGain: GainNode | null = null;
  private _incomingGain: GainNode | null = null;
  private _isActive: boolean = false;
  private _startTime: number = 0;
  private _duration: number = 0;
  private _curve: CrossfadeCurve = 'equalPower';
  private _incomingTargetGain: number = 1;
  private _outgoingTargetGain: number = 1;
  private _rafId: number | null = null;
  private _onComplete: (() => void) | null = null;
  private _fadeInOnly: boolean = false;
  private _inShape: number = 1;
  private _outShape: number = 1;
  private _loudnessCompensation: number = 1;
  private _isPaused: boolean = false;
  private _pausedAt: number = 0;

  /**
   * Schedule a full crossfade between outgoing and incoming GainNodes.
   *
   * For linear curves we use Web Audio API's built-in linearRampToValueAtTime
   * for sample-accurate transitions. For equal-power and S-curve we use
   * requestAnimationFrame to apply the non-linear curve.
   */
  scheduleFullCrossfade(
    outgoingGain: GainNode,
    incomingGain: GainNode,
    params: CrossfadeParams,
    onComplete?: () => void
  ): void {
    this.cancel();

    this._outgoingGain = outgoingGain;
    this._incomingGain = incomingGain;
    this._duration = params.duration;
    this._curve = params.curve;
    this._incomingTargetGain = params.incomingGain;
    this._onComplete = onComplete ?? null;
    this._fadeInOnly = params.fadeInOnly ?? false;
    this._inShape = params.inShape ?? 1;
    this._outShape = params.outShape ?? 1;
    this._loudnessCompensation = params.loudnessCompensation ?? 1;
    this._isActive = true;

    const audioCtx = AudioContextManager.getContext();
    if (!audioCtx) {
      console.warn('CrossfadeManager: No AudioContext available');
      this._isActive = false;
      return;
    }

    const now = audioCtx.currentTime;

    // Set initial values — start outgoing from its current gain (no pre-ramp, no sudden level change).
    // The crossfade curve handles the transition from this level to zero.
    outgoingGain.gain.cancelScheduledValues(now);
    incomingGain.gain.cancelScheduledValues(now);

    const currentOutGain = outgoingGain.gain.value;
    outgoingGain.gain.setValueAtTime(currentOutGain, now);
    this._outgoingTargetGain = currentOutGain;
    this._startTime = now;

    incomingGain.gain.setValueAtTime(0, now);

    if (params.curve === 'linear') {
      // Use Web Audio API scheduling for sample-accurate linear fade
      if (!this._fadeInOnly) {
        outgoingGain.gain.linearRampToValueAtTime(0, this._startTime + params.duration);
      }
      incomingGain.gain.linearRampToValueAtTime(params.incomingGain, this._startTime + params.duration);

      // Schedule completion callback
      const checkComplete = (): void => {
        if (!this._isActive || this._isPaused) return;
        const ctx = AudioContextManager.getContext();
        if (!ctx) return;
        if (ctx.currentTime >= this._startTime + this._duration) {
          this._finish();
        } else {
          this._rafId = requestAnimationFrame(checkComplete);
        }
      };
      this._rafId = requestAnimationFrame(checkComplete);
    } else {
      // Use RAF for non-linear curves (equalPower, sCurve)
      this._startRAFLoop();
    }

    if (IS_DEV) {
      console.log(`CrossfadeManager: Started ${params.curve} crossfade, duration=${params.duration}s`);
    }
  }

  /**
   * RAF loop for non-linear crossfade curves
   */
  private _startRAFLoop(): void {
    const loop = (): void => {
      if (!this._isActive || this._isPaused) return;

      const audioCtx = AudioContextManager.getContext();
      if (!audioCtx) {
        this._finish();
        return;
      }

      const elapsed = audioCtx.currentTime - this._startTime;
      const progress = Math.min(elapsed / this._duration, 1);
      const [outVol, inVol] = getCrossfadeValues(progress, this._curve, this._inShape, this._outShape);

      // For fade-out songs, hold outgoing gain steady — the song's natural fade handles it.
      // Only ramp the incoming sound.
      if (this._outgoingGain && !this._fadeInOnly) {
        this._outgoingGain.gain.value = outVol * this._outgoingTargetGain;
      }

      // Loudness compensation: bell-shaped envelope peaking at midpoint.
      // comp(t) = 1 + (factor-1) * sin(πt): full compensation at t=0.5, 1.0 at boundaries.
      const comp = this._loudnessCompensation !== 1
        ? 1 + (this._loudnessCompensation - 1) * Math.sin(Math.PI * progress)
        : 1;

      if (this._incomingGain) {
        this._incomingGain.gain.value = inVol * this._incomingTargetGain * comp;
      }

      if (progress >= 1) {
        this._finish();
      } else {
        this._rafId = requestAnimationFrame(loop);
      }
    };

    this._rafId = requestAnimationFrame(loop);
  }

  /**
   * Complete the crossfade
   */
  private _finish(): void {
    if (!this._isActive) return;
    this._isActive = false;
    this._isPaused = false;

    if (this._rafId !== null) {
      cancelAnimationFrame(this._rafId);
      this._rafId = null;
    }

    // Ensure final values are set
    if (this._outgoingGain) {
      this._outgoingGain.gain.cancelScheduledValues(0);
      // For fadeInOnly, don't force to 0 — the song's natural fade is still in progress.
      // The outgoing sound will be stopped/unloaded by AutoMixEngine._onCrossfadeComplete.
      if (!this._fadeInOnly) {
        this._outgoingGain.gain.value = 0;
      }
    }
    if (this._incomingGain) {
      this._incomingGain.gain.cancelScheduledValues(0);
      this._incomingGain.gain.value = this._incomingTargetGain;
    }

    if (IS_DEV) {
      console.log('CrossfadeManager: Crossfade complete');
    }

    const cb = this._onComplete;
    this._onComplete = null;
    cb?.();
  }

  /**
   * Pause the active crossfade: freeze gain scheduling and stop the RAF loop.
   * The caller is responsible for pausing the actual sound playback.
   */
  pauseCrossfade(): void {
    if (!this._isActive || this._isPaused) return;
    this._isPaused = true;

    // Stop RAF loop
    if (this._rafId !== null) {
      cancelAnimationFrame(this._rafId);
      this._rafId = null;
    }

    const audioCtx = AudioContextManager.getContext();
    if (!audioCtx) return;

    this._pausedAt = audioCtx.currentTime;

    // For linear curves, Web Audio's scheduled ramps continue even when
    // HTMLAudioElement is paused (audioCtx.currentTime keeps advancing).
    // Cancel the scheduled ramps and freeze gain at the current value.
    if (this._curve === 'linear') {
      const now = audioCtx.currentTime;
      if (this._outgoingGain && !this._fadeInOnly) {
        const currentOut = this._outgoingGain.gain.value;
        this._outgoingGain.gain.cancelScheduledValues(now);
        this._outgoingGain.gain.setValueAtTime(currentOut, now);
      }
      if (this._incomingGain) {
        const currentIn = this._incomingGain.gain.value;
        this._incomingGain.gain.cancelScheduledValues(now);
        this._incomingGain.gain.setValueAtTime(currentIn, now);
      }
    }

    if (IS_DEV) {
      console.log('CrossfadeManager: Crossfade paused');
    }
  }

  /**
   * Resume a paused crossfade: shift timing to compensate for the pause gap
   * and restart gain scheduling / RAF loop.
   */
  resumeCrossfade(): void {
    if (!this._isActive || !this._isPaused) return;
    this._isPaused = false;

    const audioCtx = AudioContextManager.getContext();
    if (!audioCtx) return;

    // Shift start time forward by the duration of the pause,
    // so progress resumes from where it was paused.
    const pauseDuration = audioCtx.currentTime - this._pausedAt;
    this._startTime += pauseDuration;

    if (this._curve === 'linear') {
      // Re-schedule linear ramps for the remaining duration
      const endTime = this._startTime + this._duration;
      if (this._outgoingGain && !this._fadeInOnly) {
        this._outgoingGain.gain.linearRampToValueAtTime(0, endTime);
      }
      if (this._incomingGain) {
        this._incomingGain.gain.linearRampToValueAtTime(this._incomingTargetGain, endTime);
      }

      // Restart RAF loop for completion check
      const checkComplete = (): void => {
        if (!this._isActive || this._isPaused) return;
        const ctx = AudioContextManager.getContext();
        if (!ctx) return;
        if (ctx.currentTime >= this._startTime + this._duration) {
          this._finish();
        } else {
          this._rafId = requestAnimationFrame(checkComplete);
        }
      };
      this._rafId = requestAnimationFrame(checkComplete);
    } else {
      // Restart RAF loop for non-linear curves
      this._startRAFLoop();
    }

    if (IS_DEV) {
      console.log(`CrossfadeManager: Crossfade resumed (pause gap=${pauseDuration.toFixed(2)}s)`);
    }
  }

  /**
   * Whether the crossfade is currently paused
   */
  isPaused(): boolean {
    return this._isPaused;
  }

  /**
   * Cancel current crossfade with a fast fade-out (100ms)
   * Used for manual skip/seek interruption.
   */
  cancel(): void {
    if (!this._isActive) return;

    if (this._rafId !== null) {
      cancelAnimationFrame(this._rafId);
      this._rafId = null;
    }

    const audioCtx = AudioContextManager.getContext();

    // Fast fade out outgoing, fast fade in incoming
    if (audioCtx && this._outgoingGain) {
      const now = audioCtx.currentTime;
      this._outgoingGain.gain.cancelScheduledValues(now);
      this._outgoingGain.gain.setValueAtTime(this._outgoingGain.gain.value, now);
      this._outgoingGain.gain.linearRampToValueAtTime(0, now + 0.1);
    }
    if (audioCtx && this._incomingGain) {
      const now = audioCtx.currentTime;
      this._incomingGain.gain.cancelScheduledValues(now);
      this._incomingGain.gain.setValueAtTime(this._incomingGain.gain.value, now);
      this._incomingGain.gain.linearRampToValueAtTime(this._incomingTargetGain, now + 0.1);
    }

    this._isActive = false;
    this._isPaused = false;
    this._onComplete = null;

    if (IS_DEV) {
      console.log('CrossfadeManager: Crossfade cancelled');
    }
  }

  /**
   * Whether a crossfade is currently active
   */
  isActive(): boolean {
    return this._isActive;
  }

  /**
   * Get crossfade progress (0-1), or -1 if not active
   */
  getProgress(): number {
    if (!this._isActive) return -1;
    const audioCtx = AudioContextManager.getContext();
    if (!audioCtx) return -1;
    const elapsed = audioCtx.currentTime - this._startTime;
    return Math.min(elapsed / this._duration, 1);
  }
}
