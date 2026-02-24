/**
 * AutoMixService — Thin facade (singleton) replacing AutoMixEngine
 *
 * Same public API as the old AutoMixEngine.
 * Internally delegates to TransitionStateMachine which coordinates all sub-modules:
 *   CrossfadeScheduler, PreBufferManager, VocalActivityGuard,
 *   CompatibilityScorer, TransitionEffects
 */

import { TransitionStateMachine } from "./TransitionStateMachine";
import { CrossfadeScheduler } from "./CrossfadeScheduler";
import { PreBufferManager } from "./PreBufferManager";
import { VocalActivityGuard } from "./VocalActivityGuard";
import { CompatibilityScorer } from "./CompatibilityScorer";
import { TransitionEffects } from "./TransitionEffects";
import type { AutoMixState } from "./types";
import type { ISound } from "../types";

const IS_DEV = import.meta.env?.DEV ?? false;

export class AutoMixService {
  private _stateMachine: TransitionStateMachine;

  constructor() {
    this._stateMachine = new TransitionStateMachine(
      new CrossfadeScheduler(),
      new PreBufferManager(),
      new VocalActivityGuard(),
      new CompatibilityScorer(),
      new TransitionEffects(),
    );

    if (IS_DEV) {
      console.log("AutoMixService: Created");
    }
  }

  // ─── Public API (backward-compatible with old AutoMixEngine) ──

  getState(): AutoMixState {
    return this._stateMachine.getState();
  }

  isCrossfading(): boolean {
    return this._stateMachine.isCrossfading();
  }

  getCrossfadeProgress(): number {
    return this._stateMachine.getCrossfadeProgress();
  }

  getActiveGainAdjustment(): number {
    return this._stateMachine.getActiveGainAdjustment();
  }

  monitorPlayback(currentSound: ISound): void {
    this._stateMachine.monitorPlayback(currentSound);
  }

  onTrackStarted(sound: ISound, songId: number): void {
    this._stateMachine.onTrackStarted(sound, songId);
  }

  cancelCrossfade(): void {
    this._stateMachine.cancelCrossfade();
  }

  pauseCrossfade(): boolean {
    return this._stateMachine.pauseCrossfade();
  }

  resumeCrossfade(): void {
    this._stateMachine.resumeCrossfade();
  }

  destroy(): void {
    this._stateMachine.destroy();
  }
}

// ─── Singleton ─────────────────────────────────────────────────────

let _instance: AutoMixService | null = null;

/**
 * Get the singleton AutoMixService instance.
 * Backward-compatible name for existing code that calls getAutoMixEngine().
 */
export function getAutoMixEngine(): AutoMixService {
  if (!_instance) {
    _instance = new AutoMixService();
  }
  return _instance;
}
