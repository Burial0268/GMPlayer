/**
 * CrossfadeManager - GainNode crossfade curves and scheduling
 *
 * Manages the volume crossfade between outgoing and incoming sounds.
 * Supports equal-power, linear, and S-curve crossfade profiles.
 *
 * All curves are pre-computed as Float32Array and scheduled via
 * setValueCurveAtTime() for sample-accurate, jitter-free transitions.
 * Optional spectral crossfade applies a data-driven 3-band parametric EQ
 * to smoothly transition spectral energy balance between tracks.
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
function buildCurveArray(
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
function buildLinearCurve(
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

/** EQ band center frequencies matching TrackAnalyzer boundaries */
const EQ_FREQUENCIES = [300, 1100, 4000] as const;
const EQ_TYPES: BiquadFilterType[] = ['lowshelf', 'peaking', 'highshelf'];

/**
 * CrossfadeManager - Schedules and manages GainNode crossfades
 *
 * All curves (linear, equalPower, sCurve) are pre-computed as Float32Array
 * and scheduled via Web Audio API's setValueCurveAtTime(). The browser
 * interpolates between array samples at audio sample rate (44100/48000 Hz),
 * producing perfectly smooth transitions with no RAF jitter.
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
  private _incomingGainAdjustment: number = 1;
  private _isPaused: boolean = false;
  private _pausedProgress: number = 0;

  // Spectral crossfade EQ filter chains (3-band parametric EQ per track)
  private _outgoingFilters: BiquadFilterNode[] = [];
  private _incomingFilters: BiquadFilterNode[] = [];
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
      console.warn('CrossfadeManager: No AudioContext available');
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
      this._setupSpectralEQ(audioCtx, outgoingGain, incomingGain, now);
    }

    // Schedule gain curves
    this._scheduleCurves(audioCtx, now);

    // Start lightweight completion watcher
    this._startCompletionWatch();

    if (IS_DEV) {
      console.log(
        `CrossfadeManager: Started ${params.curve} crossfade, duration=${params.duration}s` +
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
   * Set up 3-band parametric EQ for data-driven spectral crossfade.
   * Outgoing: EQ ramps from 0dB → outTargetDb (reshapes toward incoming's balance)
   * Incoming: EQ ramps from inInitialDb → 0dB (reveals natural spectrum)
   *
   * Band boundaries match TrackAnalyzer:
   *   lowshelf@300Hz   — low band (20-300Hz)
   *   peaking@1100Hz   — mid band (300-4000Hz)
   *   highshelf@4000Hz — high band (4000-16000Hz)
   */
  private _setupSpectralEQ(
    audioCtx: AudioContext,
    outgoingGain: GainNode,
    incomingGain: GainNode,
    startTime: number
  ): void {
    const data = this._spectralData!;
    const resolution = Math.max(64, Math.ceil(this._duration * 48));

    // Create outgoing EQ chain (gain ramps 0dB → outTargetDb)
    if (!this._fadeInOnly) {
      this._outgoingFilters = this._createEQChain(audioCtx);
      for (let i = 0; i < 3; i++) {
        const f = this._outgoingFilters[i];
        f.gain.setValueAtTime(0, startTime);
        const curve = buildLinearCurve(resolution, 0, data.outTargetDb[i]);
        f.gain.setValueCurveAtTime(curve, startTime, this._duration);
      }
      this._insertFilterChain(outgoingGain, this._outgoingFilters, audioCtx);
    }

    // Create incoming EQ chain (gain ramps inInitialDb → 0dB)
    this._incomingFilters = this._createEQChain(audioCtx);
    for (let i = 0; i < 3; i++) {
      const f = this._incomingFilters[i];
      f.gain.setValueAtTime(data.inInitialDb[i], startTime);
      const curve = buildLinearCurve(resolution, data.inInitialDb[i], 0);
      f.gain.setValueCurveAtTime(curve, startTime, this._duration);
    }
    this._insertFilterChain(incomingGain, this._incomingFilters, audioCtx);
  }

  /**
   * Create a 3-band parametric EQ chain: lowshelf → peaking → highshelf.
   * All gains start at 0dB (pass-through).
   */
  private _createEQChain(audioCtx: AudioContext): BiquadFilterNode[] {
    return EQ_TYPES.map((type, i) => {
      const f = audioCtx.createBiquadFilter();
      f.type = type;
      f.frequency.value = EQ_FREQUENCIES[i];
      if (type === 'peaking') f.Q.value = 0.7;
      f.gain.value = 0;
      return f;
    });
  }

  /**
   * Insert a chain of BiquadFilterNodes between a GainNode and the destination.
   * Chain: gainNode → filter[0] → filter[1] → filter[2] → destination
   */
  private _insertFilterChain(gainNode: GainNode, filters: BiquadFilterNode[], ctx: AudioContext): void {
    gainNode.disconnect();
    gainNode.connect(filters[0]);
    for (let i = 0; i < filters.length - 1; i++) {
      filters[i].connect(filters[i + 1]);
    }
    filters[filters.length - 1].connect(ctx.destination);
  }

  /**
   * Remove a filter chain, restoring direct gainNode → destination.
   */
  private _removeFilterChain(gainNode: GainNode, filters: BiquadFilterNode[], ctx: AudioContext): void {
    try { gainNode.disconnect(); } catch { /* already disconnected */ }
    for (const f of filters) {
      try { f.disconnect(); } catch { /* already disconnected */ }
    }
    gainNode.connect(ctx.destination);
  }

  /**
   * Clean up all spectral crossfade EQ filters, restoring direct gain→destination connections.
   */
  private _cleanupSpectralFilters(): void {
    const audioCtx = AudioContextManager.getContext();
    if (!audioCtx) return;

    if (this._outgoingFilters.length > 0 && this._outgoingGain) {
      this._removeFilterChain(this._outgoingGain, this._outgoingFilters, audioCtx);
      this._outgoingFilters = [];
    }
    if (this._incomingFilters.length > 0 && this._incomingGain) {
      this._removeFilterChain(this._incomingGain, this._incomingFilters, audioCtx);
      this._incomingFilters = [];
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
    // cancelScheduledValues(0) removes ALL events from the timeline,
    // causing the AudioParam to briefly revert to its default value (1.0
    // for GainNode.gain) before the subsequent setValueAtTime takes effect.
    // This produces a one-render-quantum pop. cancelScheduledValues(now)
    // only truncates future events, preserving the held value from completed
    // automation.
    if (this._outgoingGain) {
      this._outgoingGain.gain.cancelScheduledValues(now);
      // For fadeInOnly, don't force to 0 — the song's natural fade is still in progress.
      // The outgoing sound will be stopped/unloaded by AutoMixEngine._onCrossfadeComplete.
      if (!this._fadeInOnly) {
        this._outgoingGain.gain.setValueAtTime(0, now);
      }
    }
    if (this._incomingGain) {
      this._incomingGain.gain.cancelScheduledValues(now);
      this._incomingGain.gain.setValueAtTime(this._incomingTargetGain, now);
    }

    // Now safe to remove spectral filters — outgoing gain is locked at 0,
    // so the filter disconnect → reconnect produces no audible glitch.
    this._cleanupSpectralFilters();

    if (IS_DEV) {
      console.log('CrossfadeManager: Crossfade complete');
    }

    const cb = this._onComplete;
    this._onComplete = null;
    cb?.();
  }

  /**
   * Pause the active crossfade: cancel scheduled curves and freeze gain at
   * the computed value for the current progress point.
   * The caller is responsible for pausing the actual sound playback.
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

    // Cancel spectral EQ gain automation and freeze at interpolated values
    if (this._spectralData) {
      for (let i = 0; i < this._outgoingFilters.length; i++) {
        const f = this._outgoingFilters[i];
        f.gain.cancelScheduledValues(now);
        const currentDb = this._spectralData.outTargetDb[i] * this._pausedProgress;
        f.gain.setValueAtTime(currentDb, now);
      }
      for (let i = 0; i < this._incomingFilters.length; i++) {
        const f = this._incomingFilters[i];
        f.gain.cancelScheduledValues(now);
        const currentDb = this._spectralData.inInitialDb[i] * (1 - this._pausedProgress);
        f.gain.setValueAtTime(currentDb, now);
      }
    }

    if (IS_DEV) {
      console.log(`CrossfadeManager: Crossfade paused at progress=${this._pausedProgress.toFixed(3)}`);
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
      // Practically done — just finish
      this._finish();
      return;
    }

    // Update start time so progress calculation works correctly
    // progress = (currentTime - startTime) / duration
    // At resume: pausedProgress = (now - newStartTime) / duration
    // → newStartTime = now - pausedProgress * duration
    this._startTime = now - this._pausedProgress * this._duration;

    // Schedule remaining gain curves
    this._scheduleCurves(audioCtx, now, this._pausedProgress, 1);

    // Re-schedule remaining spectral EQ gain curves
    if (this._spectralData) {
      const eqResolution = Math.max(32, Math.ceil(remainingDuration * 48));
      for (let i = 0; i < this._outgoingFilters.length; i++) {
        const f = this._outgoingFilters[i];
        const currentDb = this._spectralData.outTargetDb[i] * this._pausedProgress;
        const endDb = this._spectralData.outTargetDb[i];
        const curve = buildLinearCurve(eqResolution, currentDb, endDb);
        f.gain.setValueCurveAtTime(curve, now, remainingDuration);
      }
      for (let i = 0; i < this._incomingFilters.length; i++) {
        const f = this._incomingFilters[i];
        const currentDb = this._spectralData.inInitialDb[i] * (1 - this._pausedProgress);
        const curve = buildLinearCurve(eqResolution, currentDb, 0);
        f.gain.setValueCurveAtTime(curve, now, remainingDuration);
      }
    }

    // Restart completion watch
    this._startCompletionWatch();

    if (IS_DEV) {
      console.log(
        `CrossfadeManager: Crossfade resumed from progress=${this._pausedProgress.toFixed(3)}, ` +
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
   * Used when the outgoing audio source ends before the crossfade finishes —
   * the outgoing GainNode is now controlling silence, so we ramp the incoming
   * to full volume smoothly rather than snapping (which causes an audible jump).
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

    // Cancel curve automation and ramp to final values smoothly.
    // The outgoing source has ended (triggering this call), so its gain
    // controls silence. The incoming is mid-ramp — the 50ms ramp avoids
    // an audible snap from mid-curve to target volume.
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

    // Ramp spectral EQ gains to 0dB (pass-through) over the same 50ms
    for (const f of this._outgoingFilters) {
      f.gain.cancelScheduledValues(now);
      f.gain.setValueAtTime(f.gain.value, now);
      f.gain.linearRampToValueAtTime(0, now + rampTime);
    }
    for (const f of this._incomingFilters) {
      f.gain.cancelScheduledValues(now);
      f.gain.setValueAtTime(f.gain.value, now);
      f.gain.linearRampToValueAtTime(0, now + rampTime);
    }

    // Wait for the 50ms ramp to complete, then finish cleanly
    const endTime = now + rampTime;
    const waitForRamp = (): void => {
      if (!this._isActive) return; // Cancelled during ramp
      const ctx = AudioContextManager.getContext();
      if (!ctx || ctx.currentTime >= endTime) {
        this._finish();
      } else {
        this._rafId = requestAnimationFrame(waitForRamp);
      }
    };
    this._rafId = requestAnimationFrame(waitForRamp);

    if (IS_DEV) {
      console.log('CrossfadeManager: Force-completing with 50ms ramp');
    }
  }

  /**
   * Cancel current crossfade with a fast fade-out (100ms).
   * Used for manual skip/seek interruption.
   * Computes current gains from the curve function rather than relying on gain.value.
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

      // Set spectral EQ gains to 0dB (pass-through) BEFORE disconnecting.
      // Without this, removing filters mid-ramp lets the shaped audio through
      // at whatever gain level remains.
      for (const f of this._outgoingFilters) {
        f.gain.cancelScheduledValues(now);
        f.gain.setValueAtTime(0, now);
      }
      for (const f of this._incomingFilters) {
        f.gain.cancelScheduledValues(now);
        f.gain.setValueAtTime(0, now);
      }
    }

    // Clean up spectral filters (now at pass-through frequencies, safe to remove)
    this._cleanupSpectralFilters();

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
