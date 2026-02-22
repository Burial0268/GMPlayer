/**
 * CrossfadeScheduler — GainNode crossfade curve scheduling
 *
 * Renamed from CrossfadeManager, delegates to curves.ts + SpectralEQ.
 * Manages the volume crossfade between outgoing and incoming sounds.
 * Supports equal-power, linear, and S-curve crossfade profiles.
 *
 * All curves are pre-computed as Float32Array and scheduled via
 * setValueCurveAtTime() for sample-accurate, jitter-free transitions.
 * Optional spectral crossfade applies a data-driven 3-band parametric EQ
 * to smoothly transition spectral energy balance between tracks.
 */

import { AudioContextManager } from '../AudioContextManager';
import { getCrossfadeValues, buildCurveArray } from './curves';
import { SpectralEQ } from './SpectralEQ';
import type { CrossfadeCurve, CrossfadeParams, SpectralCrossfadeData } from './types';

const IS_DEV = import.meta.env?.DEV ?? false;

/**
 * CrossfadeScheduler - Schedules and manages GainNode crossfades
 *
 * All curves (linear, equalPower, sCurve) are pre-computed as Float32Array
 * and scheduled via Web Audio API's setValueCurveAtTime(). The browser
 * interpolates between array samples at audio sample rate (44100/48000 Hz),
 * producing perfectly smooth transitions with no RAF jitter.
 */
export class CrossfadeScheduler {
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
  private _incomingGainAdjustment: number = 1;
  private _isPaused: boolean = false;
  private _pausedProgress: number = 0;

  // Spectral crossfade EQ
  private _spectralEQ: SpectralEQ = new SpectralEQ();
  private _spectralData: SpectralCrossfadeData | null = null;

  /**
   * Schedule a full crossfade between outgoing and incoming GainNodes.
   *
   * Pre-computes the entire crossfade curve as Float32Array and schedules
   * via setValueCurveAtTime() for sample-accurate, jitter-free transitions.
   * A lightweight RAF loop monitors timing for the completion callback only.
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
    this._onComplete = onComplete ?? null;
    this._fadeInOnly = params.fadeInOnly ?? false;
    this._inShape = params.inShape ?? 1;
    this._outShape = params.outShape ?? 1;
    this._incomingGainAdjustment = params.incomingGainAdjustment ?? 1;
    this._incomingTargetGain = params.incomingGain * this._incomingGainAdjustment;
    this._spectralData = params.spectralCrossfade || null;
    this._isActive = true;

    const audioCtx = AudioContextManager.getContext();
    if (!audioCtx) {
      console.warn('CrossfadeScheduler: No AudioContext available');
      this._isActive = false;
      return;
    }

    const now = audioCtx.currentTime;

    // Set initial values — start outgoing from its current gain (no pre-ramp, no sudden level change).
    outgoingGain.gain.cancelScheduledValues(now);
    incomingGain.gain.cancelScheduledValues(now);

    const currentOutGain = outgoingGain.gain.value;
    outgoingGain.gain.setValueAtTime(currentOutGain, now);
    this._outgoingTargetGain = currentOutGain;
    this._startTime = now;

    incomingGain.gain.setValueAtTime(0, now);

    // Set up spectral crossfade EQ if data provided
    if (this._spectralData) {
      this._spectralEQ.setup(
        audioCtx, outgoingGain, incomingGain,
        this._spectralData, now, params.duration, this._fadeInOnly
      );
    }

    // Schedule gain curves
    this._scheduleCurves(audioCtx, now);

    // Start lightweight completion watcher
    this._startCompletionWatch();

    if (IS_DEV) {
      console.log(
        `CrossfadeScheduler: Started ${params.curve} crossfade, duration=${params.duration}s` +
        (this._spectralData ? ', spectral=on' : '')
      );
    }
  }

  /**
   * Pre-compute and schedule gain curves via setValueCurveAtTime().
   * Unified path for all curve types (linear, equalPower, sCurve).
   */
  private _scheduleCurves(
    audioCtx: AudioContext,
    startTime: number,
    startProgress: number = 0,
    endProgress: number = 1
  ): void {
    const effectiveDuration = this._duration * (endProgress - startProgress);
    if (effectiveDuration <= 0) return;

    // Resolution: 48 points/sec, minimum 64. This provides smooth interpolation
    // while keeping array sizes reasonable (a 10s crossfade = 480 samples).
    const resolution = Math.max(64, Math.ceil(effectiveDuration * 48));

    // Schedule incoming curve (always)
    const inCurve = buildCurveArray(
      resolution, startProgress, endProgress,
      this._curve, this._inShape, this._outShape,
      this._incomingTargetGain, 'incoming'
    );
    this._incomingGain!.gain.setValueCurveAtTime(inCurve, startTime, effectiveDuration);

    // Schedule outgoing curve (unless fadeInOnly)
    if (!this._fadeInOnly && this._outgoingGain) {
      const outCurve = buildCurveArray(
        resolution, startProgress, endProgress,
        this._curve, this._inShape, this._outShape,
        this._outgoingTargetGain, 'outgoing'
      );
      this._outgoingGain.gain.setValueCurveAtTime(outCurve, startTime, effectiveDuration);
    }
  }

  /**
   * Lightweight RAF loop that ONLY checks timing for the completion callback.
   * No gain manipulation — all gain is handled by setValueCurveAtTime().
   */
  private _startCompletionWatch(): void {
    const check = (): void => {
      if (!this._isActive || this._isPaused) return;
      const ctx = AudioContextManager.getContext();
      if (!ctx) {
        this._finish();
        return;
      }
      if (ctx.currentTime >= this._startTime + this._duration) {
        this._finish();
      } else {
        this._rafId = requestAnimationFrame(check);
      }
    };
    this._rafId = requestAnimationFrame(check);
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

    const audioCtx = AudioContextManager.getContext();
    const now = audioCtx?.currentTime ?? 0;

    // Set final gain values FIRST, before removing spectral filters.
    // Use cancelScheduledValues(now) — NOT cancelScheduledValues(0).
    if (this._outgoingGain) {
      this._outgoingGain.gain.cancelScheduledValues(now);
      // For fadeInOnly, don't force to 0 — the song's natural fade is still in progress.
      if (!this._fadeInOnly) {
        this._outgoingGain.gain.setValueAtTime(0, now);
      }
    }
    if (this._incomingGain) {
      this._incomingGain.gain.cancelScheduledValues(now);
      this._incomingGain.gain.setValueAtTime(this._incomingTargetGain, now);
    }

    // Now safe to remove spectral filters
    this._spectralEQ.cleanupWithReconnect(this._outgoingGain, this._incomingGain);

    if (IS_DEV) {
      console.log('CrossfadeScheduler: Crossfade complete');
    }

    const cb = this._onComplete;
    this._onComplete = null;
    cb?.();
  }

  /**
   * Pause the active crossfade: cancel scheduled curves and freeze gain at
   * the computed value for the current progress point.
   */
  pauseCrossfade(): void {
    if (!this._isActive || this._isPaused) return;
    this._isPaused = true;

    // Stop completion watch
    if (this._rafId !== null) {
      cancelAnimationFrame(this._rafId);
      this._rafId = null;
    }

    const audioCtx = AudioContextManager.getContext();
    if (!audioCtx) return;

    // Compute current progress and expected gain values from the curve function
    const elapsed = audioCtx.currentTime - this._startTime;
    this._pausedProgress = Math.min(elapsed / this._duration, 1);
    const [outVol, inVol] = getCrossfadeValues(this._pausedProgress, this._curve, this._inShape, this._outShape);

    const now = audioCtx.currentTime;

    // Cancel all scheduled automation and freeze at computed values
    if (this._outgoingGain && !this._fadeInOnly) {
      this._outgoingGain.gain.cancelScheduledValues(now);
      this._outgoingGain.gain.setValueAtTime(outVol * this._outgoingTargetGain, now);
    }
    if (this._incomingGain) {
      this._incomingGain.gain.cancelScheduledValues(now);
      this._incomingGain.gain.setValueAtTime(inVol * this._incomingTargetGain, now);
    }

    // Pause spectral EQ
    this._spectralEQ.pauseAt(this._pausedProgress, now);

    if (IS_DEV) {
      console.log(`CrossfadeScheduler: Crossfade paused at progress=${this._pausedProgress.toFixed(3)}`);
    }
  }

  /**
   * Resume a paused crossfade: compute the remaining curve from the paused
   * progress point and schedule it via setValueCurveAtTime().
   */
  resumeCrossfade(): void {
    if (!this._isActive || !this._isPaused) return;
    this._isPaused = false;

    const audioCtx = AudioContextManager.getContext();
    if (!audioCtx) return;

    const now = audioCtx.currentTime;
    const remainingProgress = 1 - this._pausedProgress;
    const remainingDuration = this._duration * remainingProgress;

    if (remainingDuration <= 0.01) {
      this._finish();
      return;
    }

    // Update start time so progress calculation works correctly
    this._startTime = now - this._pausedProgress * this._duration;

    // Schedule remaining gain curves
    this._scheduleCurves(audioCtx, now, this._pausedProgress, 1);

    // Re-schedule remaining spectral EQ gain curves
    this._spectralEQ.resumeFrom(this._pausedProgress, now, remainingDuration);

    // Restart completion watch
    this._startCompletionWatch();

    if (IS_DEV) {
      console.log(
        `CrossfadeScheduler: Crossfade resumed from progress=${this._pausedProgress.toFixed(3)}, ` +
        `remaining=${remainingDuration.toFixed(2)}s`
      );
    }
  }

  /**
   * Whether the crossfade is currently paused
   */
  isPaused(): boolean {
    return this._isPaused;
  }

  /**
   * Immediately complete the crossfade with a short 50ms ramp to final values.
   * Used when the outgoing audio source ends before the crossfade finishes.
   */
  forceComplete(): void {
    if (!this._isActive) return;

    if (this._rafId !== null) {
      cancelAnimationFrame(this._rafId);
      this._rafId = null;
    }

    const audioCtx = AudioContextManager.getContext();
    if (!audioCtx) {
      this._finish();
      return;
    }

    const now = audioCtx.currentTime;
    const rampTime = 0.05; // 50ms smooth ramp to final values

    if (this._outgoingGain && !this._fadeInOnly) {
      this._outgoingGain.gain.cancelScheduledValues(now);
      this._outgoingGain.gain.setValueAtTime(this._outgoingGain.gain.value, now);
      this._outgoingGain.gain.linearRampToValueAtTime(0, now + rampTime);
    }
    if (this._incomingGain) {
      this._incomingGain.gain.cancelScheduledValues(now);
      this._incomingGain.gain.setValueAtTime(this._incomingGain.gain.value, now);
      this._incomingGain.gain.linearRampToValueAtTime(this._incomingTargetGain, now + rampTime);
    }

    // Force-complete spectral EQ
    this._spectralEQ.forceComplete(now, rampTime);

    // Wait for the 50ms ramp to complete, then finish cleanly
    const endTime = now + rampTime;
    const waitForRamp = (): void => {
      if (!this._isActive) return;
      const ctx = AudioContextManager.getContext();
      if (!ctx || ctx.currentTime >= endTime) {
        this._finish();
      } else {
        this._rafId = requestAnimationFrame(waitForRamp);
      }
    };
    this._rafId = requestAnimationFrame(waitForRamp);

    if (IS_DEV) {
      console.log('CrossfadeScheduler: Force-completing with 50ms ramp');
    }
  }

  /**
   * Cancel current crossfade with a fast fade-out (100ms).
   * Used for manual skip/seek interruption.
   */
  cancel(): void {
    if (!this._isActive) return;

    if (this._rafId !== null) {
      cancelAnimationFrame(this._rafId);
      this._rafId = null;
    }

    const audioCtx = AudioContextManager.getContext();

    if (audioCtx) {
      const now = audioCtx.currentTime;
      const elapsed = now - this._startTime;
      const progress = Math.min(elapsed / this._duration, 1);
      const [outVol, inVol] = getCrossfadeValues(progress, this._curve, this._inShape, this._outShape);

      // Fast fade using computed values as starting points
      if (this._outgoingGain) {
        this._outgoingGain.gain.cancelScheduledValues(now);
        this._outgoingGain.gain.setValueAtTime(outVol * this._outgoingTargetGain, now);
        this._outgoingGain.gain.linearRampToValueAtTime(0, now + 0.1);
      }
      if (this._incomingGain) {
        this._incomingGain.gain.cancelScheduledValues(now);
        this._incomingGain.gain.setValueAtTime(inVol * this._incomingTargetGain, now);
        this._incomingGain.gain.linearRampToValueAtTime(this._incomingTargetGain, now + 0.1);
      }

      // Cancel spectral EQ
      this._spectralEQ.cancel(now);
    }

    // Clean up spectral filters
    this._spectralEQ.cleanupWithReconnect(this._outgoingGain, this._incomingGain);

    this._isActive = false;
    this._isPaused = false;
    this._onComplete = null;

    if (IS_DEV) {
      console.log('CrossfadeScheduler: Crossfade cancelled');
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
    if (this._isPaused) return this._pausedProgress;
    const audioCtx = AudioContextManager.getContext();
    if (!audioCtx) return -1;
    const elapsed = audioCtx.currentTime - this._startTime;
    return Math.min(elapsed / this._duration, 1);
  }

  /**
   * Get the incoming gain adjustment factor used in the current/last crossfade.
   * Returns 1.0 if no adjustment was applied.
   */
  getIncomingGainAdjustment(): number {
    return this._incomingGainAdjustment;
  }
}
