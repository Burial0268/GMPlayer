/**
 * SpectralEQ — 3-band parametric EQ chain for spectral crossfade
 *
 * Extracted from CrossfadeManager.ts.
 * Manages creation, scheduling, pause/resume, and cleanup of
 * data-driven EQ filter chains during crossfade transitions.
 *
 * Band boundaries match TrackAnalyzer:
 *   lowshelf@300Hz   — low band (20-300Hz)
 *   peaking@1100Hz   — mid band (300-4000Hz)
 *   highshelf@4000Hz — high band (4000-16000Hz)
 */

import { AudioContextManager } from "../AudioContextManager";
import { buildLinearCurve, buildBassSwapCurve, bassSwapValueAt } from "./curves";
import type { SpectralCrossfadeData } from "./types";

/** EQ band center frequencies matching TrackAnalyzer boundaries */
const EQ_FREQUENCIES = [300, 1100, 4000] as const;
const EQ_TYPES: BiquadFilterType[] = ["lowshelf", "peaking", "highshelf"];

export class SpectralEQ {
  private _outgoingFilters: BiquadFilterNode[] = [];
  private _incomingFilters: BiquadFilterNode[] = [];
  private _spectralData: SpectralCrossfadeData | null = null;

  /**
   * Set up 3-band parametric EQ for data-driven spectral crossfade.
   * Outgoing: EQ ramps from 0dB → outTargetDb (reshapes toward incoming's balance)
   * Incoming: EQ ramps from inInitialDb → 0dB (reveals natural spectrum)
   */
  setup(
    ctx: AudioContext,
    outgoingGain: GainNode,
    incomingGain: GainNode,
    data: SpectralCrossfadeData,
    startTime: number,
    duration: number,
    fadeInOnly: boolean,
  ): void {
    this._spectralData = data;
    const resolution = Math.max(64, Math.ceil(duration * 48));
    const useBassSwap = data.bassSwapLow ?? false;

    // Create outgoing EQ chain (gain ramps 0dB → outTargetDb)
    if (!fadeInOnly) {
      this._outgoingFilters = this._createEQChain(ctx);
      for (let i = 0; i < 3; i++) {
        const f = this._outgoingFilters[i];
        f.gain.setValueAtTime(0, startTime);
        // Low band (i=0): use bass-swap curve for clean midpoint handoff
        const curve =
          i === 0 && useBassSwap
            ? buildBassSwapCurve(resolution, 0, data.outTargetDb[i])
            : buildLinearCurve(resolution, 0, data.outTargetDb[i]);
        f.gain.setValueCurveAtTime(curve, startTime, duration);
      }
      this._insertFilterChain(outgoingGain, this._outgoingFilters, ctx);
    }

    // Create incoming EQ chain (gain ramps inInitialDb → 0dB)
    this._incomingFilters = this._createEQChain(ctx);
    for (let i = 0; i < 3; i++) {
      const f = this._incomingFilters[i];
      f.gain.setValueAtTime(data.inInitialDb[i], startTime);
      // Low band (i=0): use bass-swap curve for clean midpoint handoff
      const curve =
        i === 0 && useBassSwap
          ? buildBassSwapCurve(resolution, data.inInitialDb[i], 0)
          : buildLinearCurve(resolution, data.inInitialDb[i], 0);
      f.gain.setValueCurveAtTime(curve, startTime, duration);
    }
    this._insertFilterChain(incomingGain, this._incomingFilters, ctx);
  }

  /**
   * Pause EQ automation at a given progress point.
   * Cancels scheduled values and freezes at interpolated gain.
   */
  pauseAt(progress: number, now: number): void {
    if (!this._spectralData) return;
    const useBassSwap = this._spectralData.bassSwapLow ?? false;

    for (let i = 0; i < this._outgoingFilters.length; i++) {
      const f = this._outgoingFilters[i];
      f.gain.cancelScheduledValues(now);
      const currentDb =
        i === 0 && useBassSwap
          ? bassSwapValueAt(progress, 0, this._spectralData.outTargetDb[i])
          : this._spectralData.outTargetDb[i] * progress;
      f.gain.setValueAtTime(currentDb, now);
    }
    for (let i = 0; i < this._incomingFilters.length; i++) {
      const f = this._incomingFilters[i];
      f.gain.cancelScheduledValues(now);
      const currentDb =
        i === 0 && useBassSwap
          ? bassSwapValueAt(progress, this._spectralData.inInitialDb[i], 0)
          : this._spectralData.inInitialDb[i] * (1 - progress);
      f.gain.setValueAtTime(currentDb, now);
    }
  }

  /**
   * Resume EQ automation from a paused progress point.
   */
  resumeFrom(progress: number, now: number, remainingDuration: number): void {
    if (!this._spectralData) return;
    const eqResolution = Math.max(32, Math.ceil(remainingDuration * 48));
    const useBassSwap = this._spectralData.bassSwapLow ?? false;

    for (let i = 0; i < this._outgoingFilters.length; i++) {
      const f = this._outgoingFilters[i];
      const currentDb =
        i === 0 && useBassSwap
          ? bassSwapValueAt(progress, 0, this._spectralData.outTargetDb[i])
          : this._spectralData.outTargetDb[i] * progress;
      const endDb = this._spectralData.outTargetDb[i];
      // For bass-swap low band, build the remaining portion of the stepped curve
      const curve =
        i === 0 && useBassSwap
          ? buildBassSwapCurve(eqResolution, currentDb, endDb)
          : buildLinearCurve(eqResolution, currentDb, endDb);
      f.gain.setValueCurveAtTime(curve, now, remainingDuration);
    }
    for (let i = 0; i < this._incomingFilters.length; i++) {
      const f = this._incomingFilters[i];
      const currentDb =
        i === 0 && useBassSwap
          ? bassSwapValueAt(progress, this._spectralData.inInitialDb[i], 0)
          : this._spectralData.inInitialDb[i] * (1 - progress);
      const curve =
        i === 0 && useBassSwap
          ? buildBassSwapCurve(eqResolution, currentDb, 0)
          : buildLinearCurve(eqResolution, currentDb, 0);
      f.gain.setValueCurveAtTime(curve, now, remainingDuration);
    }
  }

  /**
   * Force-complete: ramp all EQ gains to 0dB (pass-through) over rampTime.
   */
  forceComplete(now: number, rampTime: number): void {
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
  }

  /**
   * Cancel: set all EQ gains to 0dB immediately (pass-through).
   */
  cancel(now: number): void {
    for (const f of this._outgoingFilters) {
      f.gain.cancelScheduledValues(now);
      f.gain.setValueAtTime(0, now);
    }
    for (const f of this._incomingFilters) {
      f.gain.cancelScheduledValues(now);
      f.gain.setValueAtTime(0, now);
    }
  }

  /**
   * Clean up all filter chains, restoring direct gain→destination connections.
   */
  cleanup(): void {
    const audioCtx = AudioContextManager.getContext();
    if (!audioCtx) return;

    if (this._outgoingFilters.length > 0) {
      // Need the gain node that's connected to the first filter
      // Cleanup must be done externally if gain node reference is needed
      for (const f of this._outgoingFilters) {
        try {
          f.disconnect();
        } catch {
          /* already disconnected */
        }
      }
      this._outgoingFilters = [];
    }
    if (this._incomingFilters.length > 0) {
      for (const f of this._incomingFilters) {
        try {
          f.disconnect();
        } catch {
          /* already disconnected */
        }
      }
      this._incomingFilters = [];
    }
    this._spectralData = null;
  }

  /**
   * Clean up specific filter chain and reconnect gain node to destination.
   */
  cleanupWithReconnect(outgoingGain: GainNode | null, incomingGain: GainNode | null): void {
    const audioCtx = AudioContextManager.getContext();
    if (!audioCtx) return;

    if (this._outgoingFilters.length > 0 && outgoingGain) {
      this._removeFilterChain(outgoingGain, this._outgoingFilters, audioCtx);
      this._outgoingFilters = [];
    }
    if (this._incomingFilters.length > 0 && incomingGain) {
      this._removeFilterChain(incomingGain, this._incomingFilters, audioCtx);
      this._incomingFilters = [];
    }
    this._spectralData = null;
  }

  get hasFilters(): boolean {
    return this._outgoingFilters.length > 0 || this._incomingFilters.length > 0;
  }

  // ─── Private helpers ──────────────────────────────────────────

  /**
   * Create a 3-band parametric EQ chain: lowshelf → peaking → highshelf.
   * All gains start at 0dB (pass-through).
   */
  private _createEQChain(audioCtx: AudioContext): BiquadFilterNode[] {
    return EQ_TYPES.map((type, i) => {
      const f = audioCtx.createBiquadFilter();
      f.type = type;
      f.frequency.value = EQ_FREQUENCIES[i];
      if (type === "peaking") f.Q.value = 0.7;
      f.gain.value = 0;
      return f;
    });
  }

  /**
   * Insert a chain of BiquadFilterNodes between a GainNode and the destination.
   * Chain: gainNode → filter[0] → filter[1] → filter[2] → destination
   */
  private _insertFilterChain(
    gainNode: GainNode,
    filters: BiquadFilterNode[],
    ctx: AudioContext,
  ): void {
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
  private _removeFilterChain(
    gainNode: GainNode,
    filters: BiquadFilterNode[],
    ctx: AudioContext,
  ): void {
    try {
      gainNode.disconnect();
    } catch {
      /* already disconnected */
    }
    for (const f of filters) {
      try {
        f.disconnect();
      } catch {
        /* already disconnected */
      }
    }
    gainNode.connect(ctx.destination);
  }
}
