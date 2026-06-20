/**
 * TransitionStateMachine — Core state machine for AutoMix crossfade lifecycle
 *
 * State machine: IDLE → ANALYZING → WAITING → CROSSFADING → FINISHING → IDLE
 *
 * Key design decisions:
 *   - monitorPlayback() is SYNCHRONOUS — called per RAF frame, must never block
 *   - All heavy analysis runs in a Web Worker via TrackAnalyzer
 *   - Store references loaded once lazily (not per frame)
 *   - Never creates new AudioContexts — uses the global one
 *
 * Delegates to: CrossfadeScheduler, PreBufferManager, VocalActivityGuard,
 *               CompatibilityScorer, TransitionEffects
 */

import { CrossfadeScheduler } from "./CrossfadeScheduler";
import { PreBufferManager } from "./PreBufferManager";
import { VocalActivityGuard } from "./VocalActivityGuard";
import { CompatibilityScorer } from "./CompatibilityScorer";
import { TransitionEffects } from "./TransitionEffects";
import { getOutroTypeCrossfadeProfile } from "./types";
import { analyzeTrack, type OutroType } from "./TrackAnalyzer";
import { findNearestBeat } from "./BPMDetector";
import { AudioContextManager } from "../AudioContextManager";
import { BufferedSound } from "../BufferedSound";
import { SoundManager } from "../SoundManager";
import {
  adoptIncomingSound,
  handoffAutoMixToNativeBackend,
  syncNativeAutoMixCurrentSound,
} from "../PlayerFunctions";
import { resolveSongUrl } from "../resolveSongUrl";
import { getAudioBackendTransport } from "../../tauri/audioIpc";
import { isTauri, type AudioThreadEvent } from "../../tauri/audioBridge";
import useMusicDataStore from "@/store/musicData";
import useSettingDataStore from "@/store/settingData";
import type { ISound } from "../types";
import type {
  AutoMixState,
  CachedAnalysis,
  CrossfadeCurve,
  CrossfadeParams,
  SpectralCrossfadeData,
  TransitionStrategy,
} from "./types";

const IS_DEV = import.meta.env?.DEV ?? false;

/** Seconds before expected crossfade to start pre-analysis */
const PREPARE_AHEAD = 13;

/** Minimum crossfade duration in seconds */
const MIN_CROSSFADE_DURATION = 2;

/** Maximum entries in analysis cache */
const MAX_CACHE_SIZE = 6;

const clamp = (value: number, min: number, max: number): number =>
  Math.max(min, Math.min(max, value));

type AutoMixTransitionMode = "web" | "native";
type AutoMixTransitionPhase = "preparing" | "waiting" | "crossfading" | "finishing" | "committed";

interface ActiveAutoMixTransition {
  id: number;
  mode: AutoMixTransitionMode;
  fromIndex: number;
  toIndex: number;
  phase: AutoMixTransitionPhase;
  committed: boolean;
  holdUntil: number;
}

export class TransitionStateMachine {
  private _state: AutoMixState = "idle";

  // Injected dependencies
  private _crossfadeScheduler: CrossfadeScheduler;
  private _preBufferManager: PreBufferManager;
  private _vocalGuard: VocalActivityGuard;
  private _compatScorer: CompatibilityScorer;
  private _transitionEffects: TransitionEffects;

  // Analysis cache
  private _analysisCache: Map<number, CachedAnalysis> = new Map();

  // Current transition data
  private _currentAnalysis: CachedAnalysis | null = null;
  private _nextAnalysis: CachedAnalysis | null = null;
  private _crossfadeStartTime: number = 0;
  private _crossfadeDuration: number = 8;
  private _effectiveEnd: number = 0;
  private _outroType: OutroType | null = null;

  // Phrase-aware mix-out point (outgoing track)
  private _phraseMixOutTime: number | null = null;

  // Incoming sound during crossfade
  private _incomingSound: ISound | null = null;
  private _incomingSourceUrl: string | null = null;

  // Settings cache (refreshed from store)
  private _enabled: boolean = false;
  private _settingsCrossfadeDuration: number = 8;
  private _settingsCurve: CrossfadeCurve = "equalPower";
  private _settingsBpmMatch: boolean = true;
  private _settingsBeatAlign: boolean = true;
  private _settingsVolumeNorm: boolean = true;
  private _settingsSmartCurve: boolean = true;
  private _settingsTransitionEffects: boolean = true;
  private _settingsVocalGuard: boolean = true;
  private _settingsPhraseAlign: boolean = true;

  // Async guard: prevent duplicate analysis
  private _analyzingInFlight: boolean = false;

  // Failure cooldown: prevent retry loops after crossfade failure
  private _lastFailureTime: number = 0;

  // Persistent gain adjustment for the currently-playing track (from LUFS normalization).
  private _activeGainAdjustment: number = 1;

  // Last computed transition strategy (used across _finalizeCrossfadeParams and _doCrossfade)
  private _lastStrategy: TransitionStrategy | null = null;

  // Pending store update: next playlist index to set after crossfade.
  // Used as fallback in _onCrossfadeComplete when _waitForPlayStart is delayed (background tabs).
  private _pendingNextIndex: number = -1;
  private _transitionSeq: number = 0;
  private _activeTransition: ActiveAutoMixTransition | null = null;

  // Pause state during crossfade
  private _isPaused: boolean = false;

  // Software fade fallback timeout tracking
  private _softwareFadeTimerId: ReturnType<typeof setTimeout> | null = null;
  private _softwareFadeStartedAt: number = 0;
  private _softwareFadeRemaining: number = 0;

  // Delayed finishing→idle transition timer.
  private _finishingTimerId: ReturnType<typeof setTimeout> | null = null;
  private _nativeFinishTimerId: ReturnType<typeof setTimeout> | null = null;

  // Native AutoMix handoff state (Tauri backend owns the actual crossfade).
  private _nativeAutoMixPreparing: boolean = false;
  private _nativeAutoMixReady: boolean = false;
  private _nativeNextIndex: number = -1;
  private _nativeUnsubscribe: (() => void) | null = null;
  private _nativeForceStartSent: boolean = false;
  private _nativeHandoffHoldUntil: number = 0;

  // Store references (loaded once lazily)
  private _musicStoreRef: any = null;
  private _settingStoreRef: any = null;
  private _storesReady: boolean = false;

  // Unpause promise
  private _unpauseResolve: (() => void) | null = null;

  constructor(
    crossfadeScheduler: CrossfadeScheduler,
    preBufferManager: PreBufferManager,
    vocalGuard: VocalActivityGuard,
    compatScorer: CompatibilityScorer,
    transitionEffects: TransitionEffects,
  ) {
    this._crossfadeScheduler = crossfadeScheduler;
    this._preBufferManager = preBufferManager;
    this._vocalGuard = vocalGuard;
    this._compatScorer = compatScorer;
    this._transitionEffects = transitionEffects;

    if (IS_DEV) {
      console.log("TransitionStateMachine: Created");
    }
  }

  // ─── Public getters ────────────────────────────────────────────

  getState(): AutoMixState {
    return this._state;
  }

  isCrossfading(): boolean {
    return this._state === "crossfading" || this._state === "finishing";
  }

  isHandoffActive(): boolean {
    return (
      this.isCrossfading() ||
      Date.now() < this._nativeHandoffHoldUntil ||
      this._activeTransition !== null
    );
  }

  getCrossfadeProgress(): number {
    return this._crossfadeScheduler.getProgress();
  }

  getActiveGainAdjustment(): number {
    return this._activeGainAdjustment;
  }

  private _beginTransition(
    mode: AutoMixTransitionMode,
    fromIndex: number,
    toIndex: number,
    phase: AutoMixTransitionPhase,
  ): number {
    const id = ++this._transitionSeq;
    this._activeTransition = {
      id,
      mode,
      fromIndex,
      toIndex,
      phase,
      committed: false,
      holdUntil: 0,
    };
    return id;
  }

  private _isTransitionActive(id: number, mode?: AutoMixTransitionMode): boolean {
    const active = this._activeTransition;
    return !!active && active.id === id && (!mode || active.mode === mode);
  }

  private _setTransitionPhase(id: number, phase: AutoMixTransitionPhase): void {
    if (!this._isTransitionActive(id)) return;
    this._activeTransition!.phase = phase;
  }

  private _holdTransition(id: number, durationMs = 3000): void {
    if (!this._isTransitionActive(id)) return;
    this._activeTransition!.phase = "committed";
    this._activeTransition!.holdUntil = Date.now() + durationMs;
    setTimeout(() => {
      const active = this._activeTransition;
      if (active?.id === id && active.phase === "committed" && Date.now() >= active.holdUntil) {
        this._activeTransition = null;
      }
    }, durationMs + 50);
  }

  private _clearTransition(id?: number): void {
    if (id !== undefined && !this._isTransitionActive(id)) return;
    this._activeTransition = null;
  }

  // ─── Store loading (lazy, one-time) ────────────────────────────

  private _loadStores(): void {
    if (this._storesReady) return;
    try {
      this._musicStoreRef = useMusicDataStore();
      this._settingStoreRef = useSettingDataStore();
      this._storesReady = true;
      if (IS_DEV) {
        console.log("TransitionStateMachine: Stores loaded");
      }
    } catch (err) {
      console.error("TransitionStateMachine: Failed to load stores", err);
    }
  }

  private _refreshSettings(): void {
    if (!this._settingStoreRef) return;
    this._enabled = this._settingStoreRef.autoMixEnabled ?? false;
    this._settingsCrossfadeDuration = this._settingStoreRef.autoMixCrossfadeDuration ?? 8;
    this._settingsCurve = this._settingStoreRef.autoMixTransitionStyle ?? "equalPower";
    this._settingsBpmMatch = this._settingStoreRef.autoMixBpmMatch ?? true;
    this._settingsBeatAlign = this._settingStoreRef.autoMixBeatAlign ?? true;
    this._settingsVolumeNorm = this._settingStoreRef.autoMixVolumeNorm ?? true;
    this._settingsSmartCurve = this._settingStoreRef.autoMixSmartCurve ?? true;
    this._settingsTransitionEffects = this._settingStoreRef.autoMixTransitionEffects ?? true;
    this._settingsVocalGuard = this._settingStoreRef.autoMixVocalGuard ?? true;
    this._settingsPhraseAlign = this._settingStoreRef.autoMixPhraseAlign ?? true;
  }

  private _shouldBeActive(): boolean {
    if (!this._enabled) return false;
    if (!this._musicStoreRef) return false;

    const music = this._musicStoreRef;
    if (music.persistData.playSongMode === "single") return false;
    if (music.persistData.personalFmMode) return false;
    if (music.persistData.playlists.length < 2) return false;

    return true;
  }

  // ─── Core: monitorPlayback (SYNCHRONOUS) ───────────────────────

  monitorPlayback(currentSound: ISound): void {
    if (!this._storesReady) {
      this._loadStores();
      return;
    }

    this._refreshSettings();

    if (!this._shouldBeActive()) {
      if (this._state !== "idle") {
        this.cancelCrossfade();
      }
      return;
    }

    if (!currentSound || !currentSound.playing()) {
      if (this._state === "waiting" || this._state === "analyzing") {
        if (IS_DEV) {
          console.log(
            `TransitionStateMachine: Song ended during ${this._state} state, cleaning up`,
          );
        }
        this.cancelCrossfade();
      }
      return;
    }

    const currentTime = currentSound.seek() as number;
    const duration = currentSound.duration();
    if (!duration || duration <= 0) return;

    switch (this._state) {
      case "idle":
        this._handleIdle(currentTime, duration);
        break;
      case "analyzing":
        break;
      case "waiting":
        this._handleWaiting(currentTime);
        break;
      case "crossfading":
      case "finishing":
        break;
    }
  }

  // ─── State: IDLE ───────────────────────────────────────────────

  private _handleIdle(currentTime: number, duration: number): void {
    if (this._lastFailureTime > 0 && Date.now() - this._lastFailureTime < 60000) {
      return;
    }

    const effectiveDuration = this._getEffectiveCrossfadeDuration(duration);
    const triggerTime = duration - effectiveDuration - PREPARE_AHEAD;

    if (currentTime >= triggerTime && currentTime < duration - 1) {
      const currentSound = SoundManager.getCurrentSound();
      if (this._isNativeAutoMixSound(currentSound)) {
        this._startNativeAutoMixPrepare(duration);
        return;
      }
      this._startAnalysis();
    }
  }

  private _getEffectiveCrossfadeDuration(songDuration: number): number {
    const maxDuration = songDuration / 4;
    return Math.max(MIN_CROSSFADE_DURATION, Math.min(this._settingsCrossfadeDuration, maxDuration));
  }

  private _isNativeAutoMixSound(sound: ISound | null): boolean {
    return (
      isTauri() &&
      !!sound &&
      typeof (sound as { getSourceUrl?: () => string | null }).getSourceUrl === "function"
    );
  }

  private _startNativeAutoMixPrepare(duration: number): void {
    if (this._nativeAutoMixPreparing || this._nativeAutoMixReady) return;

    const music = this._musicStoreRef;
    if (!music) return;
    const playlist = music.persistData.playlists;
    const currentIndex = music.persistData.playSongIndex;
    const listLength = playlist.length;
    if (listLength < 2) return;

    const nextIndex = this._selectNextIndex(
      currentIndex,
      listLength,
      music.persistData.playSongMode,
    );
    const nextSong = playlist[nextIndex];
    if (!nextSong) return;

    const effectiveDuration = this._getEffectiveCrossfadeDuration(duration);
    const transitionId = this._beginTransition("native", currentIndex, nextIndex, "preparing");
    this._crossfadeDuration = effectiveDuration;
    this._effectiveEnd = duration;
    this._crossfadeStartTime = Math.max(0, duration - effectiveDuration);
    this._nativeNextIndex = nextIndex;
    this._nativeForceStartSent = false;
    this._nativeAutoMixPreparing = true;
    this._state = "analyzing";
    this._updateStoreState();

    void this._sendNativeAutoMixPrepare(transitionId, currentIndex, nextIndex, nextSong)
      .then(() => {
        // Native analysis completion is event-driven. Wait for
        // automixAnalysisReady/status so frontend timing cannot race backend
        // download/analysis work.
      })
      .catch((err) => {
        if (!this._isTransitionActive(transitionId, "native")) return;
        this._nativeNextIndex = -1;
        this._clearTransition(transitionId);
        if (this._state === "analyzing") {
          this._state = "idle";
          this._updateStoreState();
        }
        if (IS_DEV) {
          console.warn("TransitionStateMachine: Native AutoMix prepare failed", err);
        }
      })
      .finally(() => {
        this._nativeAutoMixPreparing = false;
      });
  }

  private _selectNextIndex(currentIndex: number, listLength: number, playMode: string): number {
    if (playMode !== "random") return (currentIndex + 1) % listLength;
    if (listLength <= 1) return currentIndex;

    let nextIndex = currentIndex;
    for (let i = 0; i < 8 && nextIndex === currentIndex; i++) {
      nextIndex = Math.floor(Math.random() * listLength);
    }
    if (nextIndex === currentIndex) {
      nextIndex = (currentIndex + 1) % listLength;
    }
    return nextIndex;
  }

  private async _sendNativeAutoMixPrepare(
    transitionId: number,
    currentIndex: number,
    nextIndex: number,
    nextSong: any,
  ): Promise<void> {
    const resolved = await resolveSongUrl(nextSong);
    if (!resolved?.url) throw new Error("Failed to resolve next song URL for native AutoMix");

    const transport = getAudioBackendTransport();
    await transport.connect();
    this._ensureNativeAutoMixSubscription();
    transport.sendOrQueue({
      type: "automixConfigure",
      config: {
        enabled: true,
        crossfadeDuration: this._settingsCrossfadeDuration,
        bpmMatch: this._settingsBpmMatch,
        beatAlign: this._settingsBeatAlign,
        volumeNorm: this._settingsVolumeNorm,
        smartCurve: this._settingsSmartCurve,
        transitionStyle: this._settingsCurve,
        transitionEffects: this._settingsTransitionEffects,
        vocalGuard: this._settingsVocalGuard,
      },
    });
    transport.sendOrQueue({
      type: "automixPrepareNext",
      currentIndex,
      nextIndex,
      transitionId,
      nextSong: {
        type: "local",
        filePath: resolved.url,
        origOrder: nextIndex,
      },
    });
  }

  private _ensureNativeAutoMixSubscription(): void {
    if (this._nativeUnsubscribe !== null) return;
    try {
      this._nativeUnsubscribe = getAudioBackendTransport().subscribe((event) => {
        this._handleNativeAutoMixEvent(event);
      });
    } catch (err) {
      if (IS_DEV) {
        console.warn("TransitionStateMachine: Native AutoMix event subscription failed", err);
      }
    }
  }

  private _acceptNativeTransitionEvent(transitionId?: number | null): boolean {
    if (transitionId === undefined || transitionId === null) return true;
    return this._isTransitionActive(transitionId, "native");
  }

  private _handleNativeAutoMixEvent(event: AudioThreadEvent): void {
    switch (event.type) {
      case "automixStatus": {
        const status = event.data.status;
        if (!this._acceptNativeTransitionEvent(status.transitionId)) return;
        if (status.nextIndex !== undefined && status.nextIndex !== null) {
          this._nativeNextIndex = status.nextIndex;
        }
        if (status.crossfadeStart !== undefined && status.crossfadeStart !== null) {
          this._crossfadeStartTime = status.crossfadeStart;
        }
        if (status.crossfadeDuration !== undefined && status.crossfadeDuration !== null) {
          this._crossfadeDuration = status.crossfadeDuration;
        }
        if (status.state === "waiting" && this._state === "analyzing") {
          this._nativeAutoMixReady = true;
          if (status.transitionId !== undefined && status.transitionId !== null) {
            this._setTransitionPhase(status.transitionId, "waiting");
          }
          this._state = "waiting";
          this._updateStoreState();
        }
        if (status.state === "failed") {
          this._handleNativeAutoMixFailure(status.error ?? "Native AutoMix failed");
        }
        if (
          status.state === "idle" &&
          this._state === "crossfading" &&
          this._nativeForceStartSent &&
          status.currentIndex === this._nativeNextIndex
        ) {
          this._completeNativeAutoMix(status.currentIndex);
        }
        break;
      }
      case "automixAnalysisReady":
        if (!this._acceptNativeTransitionEvent(event.data.transitionId)) return;
        if (this._state === "analyzing") {
          this._nativeAutoMixReady = true;
          if (event.data.transitionId !== undefined && event.data.transitionId !== null) {
            this._setTransitionPhase(event.data.transitionId, "waiting");
          }
          this._state = "waiting";
          this._updateStoreState();
        }
        break;
      case "automixCrossfadeStarted":
        if (!this._acceptNativeTransitionEvent(event.data.transitionId)) return;
        if (this._nativeNextIndex < 0) return;
        this._nativeAutoMixReady = false;
        this._nativeForceStartSent = true;
        if (event.data.transitionId !== undefined && event.data.transitionId !== null) {
          this._setTransitionPhase(event.data.transitionId, "crossfading");
        }
        this._crossfadeDuration = event.data.duration || this._crossfadeDuration;
        this._state = "crossfading";
        this._updateStoreState();
        this._armNativeAutoMixWatchdog();
        break;
      case "automixCrossfadeComplete":
        if (!this._acceptNativeTransitionEvent(event.data.transitionId)) return;
        this._completeNativeAutoMix(event.data.currentIndex);
        break;
      case "automixError":
        this._handleNativeAutoMixFailure(event.data.error);
        break;
    }
  }

  private _handleNativeAutoMixFailure(error: string): void {
    if (!this._nativeAutoMixPreparing && !this._nativeAutoMixReady && this._nativeNextIndex < 0) {
      return;
    }
    if (IS_DEV) {
      console.warn("TransitionStateMachine: Native AutoMix failed", error);
    }
    this._lastFailureTime = Date.now();
    this._nativeAutoMixPreparing = false;
    this._nativeAutoMixReady = false;
    this._nativeForceStartSent = false;
    this._nativeNextIndex = -1;
    this._clearTransition();
    if (this._nativeFinishTimerId !== null) {
      clearTimeout(this._nativeFinishTimerId);
      this._nativeFinishTimerId = null;
    }
    if (this._state === "analyzing" || this._state === "waiting" || this._state === "crossfading") {
      this._state = "idle";
      this._updateStoreState();
    }
  }

  private _armNativeAutoMixWatchdog(): void {
    if (this._nativeFinishTimerId !== null) {
      clearTimeout(this._nativeFinishTimerId);
    }
    const timeoutMs = Math.max(5000, this._crossfadeDuration * 3000 + 5000);
    this._nativeFinishTimerId = setTimeout(() => {
      this._nativeFinishTimerId = null;
      if (this._state === "crossfading" && this._nativeNextIndex >= 0) {
        this._handleNativeAutoMixFailure("Native AutoMix completion timeout");
      }
    }, timeoutMs);
  }

  private _completeNativeAutoMix(currentIndex: number): void {
    const transitionId = this._activeTransition?.mode === "native" ? this._activeTransition.id : 0;
    if (this._nativeFinishTimerId !== null) {
      clearTimeout(this._nativeFinishTimerId);
      this._nativeFinishTimerId = null;
    }

    if (transitionId > 0) {
      this._setTransitionPhase(transitionId, "finishing");
    }
    this._state = "finishing";
    this._updateStoreState();

    const nextIndex = currentIndex >= 0 ? currentIndex : this._nativeNextIndex;
    if (nextIndex >= 0 && this._musicStoreRef) {
      this._musicStoreRef.persistData.playSongIndex = nextIndex;
      if (typeof this._musicStoreRef.resetSongLyricState === "function") {
        this._musicStoreRef.resetSongLyricState();
      }
    }

    const currentSound = SoundManager.getCurrentSound();
    if (currentSound) {
      void syncNativeAutoMixCurrentSound(currentSound);
    }

    this._nativeHandoffHoldUntil = Date.now() + 3000;
    if (transitionId > 0) {
      this._holdTransition(transitionId);
    }
    this._nativeAutoMixPreparing = false;
    this._nativeAutoMixReady = false;
    this._nativeForceStartSent = false;
    this._nativeNextIndex = -1;

    if (this._finishingTimerId !== null) {
      clearTimeout(this._finishingTimerId);
    }
    this._finishingTimerId = setTimeout(() => {
      this._finishingTimerId = null;
      if (this._state === "finishing") {
        this._state = "idle";
        this._updateStoreState();
      }
    }, 800);
  }

  // ─── State: ANALYZING ──────────────────────────────────────────

  private _startAnalysis(): void {
    if (this._analyzingInFlight) return;

    this._state = "analyzing";
    this._analyzingInFlight = true;
    this._updateStoreState();

    this._doAnalysis()
      .then(() => {
        if (this._state === "analyzing") {
          this._state = "waiting";
          this._updateStoreState();
        }
      })
      .catch((err) => {
        if (IS_DEV) {
          console.warn(
            "TransitionStateMachine: Analysis failed, falling back to time-based transition",
            err,
          );
        }
        if (this._state === "analyzing") {
          this._currentAnalysis = null;
          this._nextAnalysis = null;
          this._computeCrossfadeParams();
          this._state = "waiting";
          this._updateStoreState();
        }
      })
      .finally(() => {
        this._analyzingInFlight = false;
      });
  }

  private async _doAnalysis(): Promise<void> {
    const music = this._musicStoreRef;
    if (!music) return;

    const playlist = music.persistData.playlists;
    const currentIndex = music.persistData.playSongIndex;

    // Analyze current track if not cached
    const currentSong = playlist[currentIndex];
    if (currentSong && !this._analysisCache.has(currentSong.id)) {
      const analysisUrl = this._getSoundAnalysisUrl(SoundManager.getCurrentSound());
      if (analysisUrl) {
        try {
          const analysis = await analyzeTrack(analysisUrl, {
            analyzeBPM: this._settingsBpmMatch,
          });
          this._currentAnalysis = { songId: currentSong.id, analysis };
          this._addToCache(this._currentAnalysis);
        } catch (err) {
          if (IS_DEV) console.warn("TransitionStateMachine: Current track analysis failed", err);
        }
      }
    } else if (currentSong && this._analysisCache.has(currentSong.id)) {
      this._currentAnalysis = this._analysisCache.get(currentSong.id)!;
    }

    // Check cache for the next track
    const nextIndex =
      music.persistData.playSongMode === "random" ? -1 : (currentIndex + 1) % playlist.length;
    if (nextIndex >= 0) {
      const nextSong = playlist[nextIndex];
      if (nextSong && this._analysisCache.has(nextSong.id)) {
        this._nextAnalysis = this._analysisCache.get(nextSong.id)!;
      }
    }

    this._computeCrossfadeParams();
  }

  private _getSoundAnalysisUrl(sound: ISound | null): string | null {
    if (!sound) return null;
    if (sound instanceof BufferedSound) {
      return sound.getBlobUrl();
    }
    const sourceUrl = (sound as { getSourceUrl?: () => string | null }).getSourceUrl?.();
    if (sourceUrl) {
      return sourceUrl;
    }
    return null;
  }

  // ─── Crossfade parameter computation ───────────────────────────

  private _computeCrossfadeParams(): void {
    const currentSound = SoundManager.getCurrentSound();
    if (!currentSound) return;

    const duration = currentSound.duration();
    const trailingSilence = this._currentAnalysis?.analysis.energy.trailingSilence ?? 0;
    const effectiveEnd = duration - trailingSilence;
    this._effectiveEnd = effectiveEnd;

    this._crossfadeDuration = this._getEffectiveCrossfadeDuration(effectiveEnd);

    // Reset phrase mix point
    this._phraseMixOutTime = null;

    const outro = this._currentAnalysis?.analysis.outro;
    if (outro) {
      this._outroType = outro.outroType;

      if (outro.outroConfidence >= 0.75) {
        this._crossfadeStartTime = outro.suggestedCrossfadeStart;
      } else {
        this._crossfadeStartTime = effectiveEnd - this._crossfadeDuration;
      }

      const remainingTime = duration - this._crossfadeStartTime;
      switch (outro.outroType) {
        case "hard":
          this._crossfadeDuration = Math.max(
            MIN_CROSSFADE_DURATION,
            Math.min(3, this._getEffectiveCrossfadeDuration(effectiveEnd)),
          );
          if (outro.outroConfidence < 0.75) {
            this._crossfadeStartTime = effectiveEnd - this._crossfadeDuration;
          }
          break;
        case "fadeOut":
          this._crossfadeDuration = Math.max(
            MIN_CROSSFADE_DURATION,
            Math.min(remainingTime * 0.8, this._getEffectiveCrossfadeDuration(effectiveEnd)),
          );
          break;
        case "reverbTail":
          this._crossfadeDuration = Math.max(
            MIN_CROSSFADE_DURATION,
            Math.min(outro.musicalEndOffset, this._getEffectiveCrossfadeDuration(effectiveEnd)),
          );
          break;
        case "slowDown":
          this._crossfadeDuration = Math.max(
            MIN_CROSSFADE_DURATION,
            Math.min(remainingTime * 0.7, this._getEffectiveCrossfadeDuration(effectiveEnd)),
          );
          break;
        case "sustained":
          this._crossfadeDuration = Math.max(
            MIN_CROSSFADE_DURATION,
            Math.min(outro.musicalEndOffset + 2, this._getEffectiveCrossfadeDuration(effectiveEnd)),
          );
          break;
        case "musicalOutro":
          this._crossfadeDuration = Math.max(
            MIN_CROSSFADE_DURATION,
            Math.min(remainingTime * 0.6, this._getEffectiveCrossfadeDuration(effectiveEnd)),
          );
          break;
        case "loopFade":
          this._crossfadeDuration = Math.max(
            MIN_CROSSFADE_DURATION,
            Math.min(remainingTime * 0.8, this._getEffectiveCrossfadeDuration(effectiveEnd)),
          );
          break;
      }
    } else if (this._currentAnalysis?.analysis.energy) {
      const energy = this._currentAnalysis.analysis.energy;
      const isFadeOut = energy.isFadeOut;
      this._outroType = isFadeOut ? "fadeOut" : "hard";

      const outroOffset = energy.outroStartOffset;
      if (isFadeOut) {
        const outroContentDuration = outroOffset - trailingSilence;
        const fadeInPoint = outroContentDuration * 0.5;
        this._crossfadeStartTime = Math.max(0, duration - trailingSilence - fadeInPoint);
      } else {
        this._crossfadeStartTime = Math.max(0, duration - outroOffset);
      }
    } else {
      this._outroType = null;
      this._crossfadeStartTime = effectiveEnd - this._crossfadeDuration;
    }

    // Phrase-aware mix point selection (DJ-style 16-beat phrase alignment)
    const phrases = this._currentAnalysis?.analysis.phrases;
    if (this._settingsPhraseAlign && phrases?.mixOutPhrase) {
      const phraseStart = phrases.mixOutPhrase.start;
      // Only use phrase mix point if it's within reasonable range of the computed start
      const phraseDiff = Math.abs(phraseStart - this._crossfadeStartTime);
      const maxPhraseShift = this._crossfadeDuration * 0.75;

      if (phraseDiff <= maxPhraseShift && phraseStart < effectiveEnd - MIN_CROSSFADE_DURATION) {
        this._phraseMixOutTime = phraseStart;
        this._crossfadeStartTime = phraseStart;
        if (IS_DEV) {
          console.log(
            `TransitionStateMachine: Phrase-aligned mix-out at ${phraseStart.toFixed(1)}s ` +
              `(phrase #${phrases.mixOutPhrase.index})`,
          );
        }
      }
    }

    // Beat-align (skip for certain outro types where timing is less critical)
    const skipBeatAlign =
      this._outroType === "fadeOut" ||
      this._outroType === "reverbTail" ||
      this._outroType === "sustained" ||
      this._outroType === "loopFade";
    if (this._settingsBeatAlign && !skipBeatAlign && this._currentAnalysis?.analysis.bpm) {
      const bpmResult = this._currentAnalysis.analysis.bpm;
      this._crossfadeStartTime = findNearestBeat(
        bpmResult.beatGrid,
        this._crossfadeStartTime,
        bpmResult.analysisOffset,
      );
    }

    if (this._crossfadeStartTime > effectiveEnd - MIN_CROSSFADE_DURATION) {
      this._crossfadeStartTime = effectiveEnd - this._crossfadeDuration;
    }
    if (this._crossfadeStartTime < 0) {
      this._crossfadeStartTime = 0;
    }

    if (IS_DEV) {
      console.log(
        `TransitionStateMachine: Crossfade params — start=${this._crossfadeStartTime.toFixed(1)}s, ` +
          `duration=${this._crossfadeDuration.toFixed(1)}s` +
          (trailingSilence > 0 ? `, trailingSilence=${trailingSilence.toFixed(1)}s` : "") +
          (this._outroType ? `, outroType=${this._outroType}` : "") +
          (outro ? `, confidence=${outro.outroConfidence.toFixed(2)}` : "") +
          (this._phraseMixOutTime !== null
            ? `, phraseMix=${this._phraseMixOutTime.toFixed(1)}s`
            : ""),
      );
    }
  }

  // ─── Energy contrast computation ────────────────────────────────

  private _computeEnergyContrast(): number {
    const outroMB = this._currentAnalysis?.analysis.outro?.multibandEnergy;
    const introMB = this._nextAnalysis?.analysis.intro?.multibandEnergy;

    if (outroMB && introMB) {
      const outroLen = outroMB.low.length;
      const introLen = introMB.low.length;
      const outroWindows = Math.min(8, outroLen);
      const introWindows = Math.min(8, introLen);
      if (outroWindows < 2 || introWindows < 2) return 1;

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

      if (outroE < 0.0001 || introE < 0.0001) return 1;
      return introE / outroE;
    }

    const outEnergy = this._currentAnalysis?.analysis.energy;
    const inEnergy = this._nextAnalysis?.analysis.energy;
    if (!outEnergy || !inEnergy) return 1;

    const outEps = outEnergy.energyPerSecond;
    const outLen = outEps.length;
    const outStart = Math.max(0, outLen - 5);
    let outAvg = 0;
    for (let i = outStart; i < outLen; i++) outAvg += outEps[i];
    outAvg /= outLen - outStart || 1;

    const inEps = inEnergy.energyPerSecond;
    const inEnd = Math.min(5, inEps.length);
    let inAvg = 0;
    for (let i = 0; i < inEnd; i++) inAvg += inEps[i];
    inAvg /= inEnd || 1;

    if (outAvg < 0.001 || inAvg < 0.001) return 1;
    return inAvg / outAvg;
  }

  // ─── Spectral crossfade data computation ─────────────────────────

  private _computeSpectralCrossfadeData(): SpectralCrossfadeData | false {
    const outroMB = this._currentAnalysis?.analysis.outro?.multibandEnergy;
    const introMB = this._nextAnalysis?.analysis.intro?.multibandEnergy;

    if (!outroMB || !introMB) return false;

    const bands = ["low", "mid", "high"] as const;
    const outroAvg = [0, 0, 0];
    const introAvg = [0, 0, 0];

    const outroWindows = Math.min(8, outroMB.low.length);
    const introWindows = Math.min(8, introMB.low.length);

    if (outroWindows < 2 || introWindows < 2) return false;

    for (let b = 0; b < 3; b++) {
      const band = bands[b];
      const outroData = outroMB[band];
      const introData = introMB[band];

      let outSum = 0;
      for (let i = outroData.length - outroWindows; i < outroData.length; i++) {
        outSum += outroData[i];
      }
      outroAvg[b] = outSum / outroWindows;

      let inSum = 0;
      for (let i = 0; i < introWindows; i++) {
        inSum += introData[i];
      }
      introAvg[b] = inSum / introWindows;
    }

    const diffDb: [number, number, number] = [0, 0, 0];
    let maxAbsDiff = 0;

    for (let b = 0; b < 3; b++) {
      if (outroAvg[b] < 0.0001 && introAvg[b] < 0.0001) {
        diffDb[b] = 0;
        continue;
      }
      if (outroAvg[b] < 0.0001) {
        diffDb[b] = 6;
      } else if (introAvg[b] < 0.0001) {
        diffDb[b] = -6;
      } else {
        diffDb[b] = 10 * Math.log10(introAvg[b] / outroAvg[b]);
      }
      diffDb[b] = Math.max(-6, Math.min(6, diffDb[b]));
      maxAbsDiff = Math.max(maxAbsDiff, Math.abs(diffDb[b]));
    }

    if (maxAbsDiff < 1.5) return false;

    const outTargetDb: [number, number, number] = [diffDb[0], diffDb[1], diffDb[2]];
    const inInitialDb: [number, number, number] = [-diffDb[0], -diffDb[1], -diffDb[2]];
    const bassSwapLow = outroAvg[0] > 0.01 && introAvg[0] > 0.01 && Math.abs(diffDb[0]) >= 2.0;

    if (IS_DEV) {
      console.log(
        `TransitionStateMachine: Spectral crossfade — ` +
          `outTarget=[${outTargetDb.map((d) => d.toFixed(1)).join(", ")}]dB, ` +
          `inInitial=[${inInitialDb.map((d) => d.toFixed(1)).join(", ")}]dB` +
          (bassSwapLow ? ", bassSwap=on" : ""),
      );
    }

    return { outTargetDb, inInitialDb, bassSwapLow };
  }

  private _computeIncomingGainAdjustment(energyContrast: number): number {
    if (!this._settingsVolumeNorm) return 1;

    const nextVolume = this._nextAnalysis?.analysis.volume;
    if (!nextVolume) return 1;

    const absoluteAdjustment = clamp(nextVolume.gainAdjustment ?? 1, 0.55, 1.8);
    const currentVolume = this._currentAnalysis?.analysis.volume;
    let adjustment = absoluteAdjustment;

    if (currentVolume) {
      const currentEffectiveLufs =
        currentVolume.estimatedLUFS + 20 * Math.log10(Math.max(0.001, this._activeGainAdjustment));
      const relativeAdjustment = Math.pow(
        10,
        (currentEffectiveLufs - nextVolume.estimatedLUFS) / 20,
      );

      // Relative matching keeps the handoff level stable; absolute targeting
      // gently pulls the playlist toward the configured loudness over time.
      adjustment =
        Math.pow(clamp(relativeAdjustment, 0.5, 2.0), 0.75) * Math.pow(absoluteAdjustment, 0.25);
    }

    if (energyContrast > 3) {
      adjustment *= energyContrast > 6 ? 0.86 : 0.92;
    } else if (energyContrast < 0.33) {
      adjustment *= 1.08;
    }

    return clamp(adjustment, 0.55, 1.8);
  }

  private _computeOverlapHeadroomDb(
    energyContrast: number,
    loudnessScore: number,
    fadeInOnly: boolean,
  ): number {
    let headroom = fadeInOnly ? -1.2 : -0.8;

    if (energyContrast > 6 || energyContrast < 0.2) {
      headroom -= 1.1;
    } else if (energyContrast > 3 || energyContrast < 0.33) {
      headroom -= 0.6;
    }

    if (loudnessScore < 0.35) {
      headroom -= 0.8;
    } else if (loudnessScore < 0.55) {
      headroom -= 0.4;
    }

    return clamp(headroom, -2.8, 0);
  }

  // ─── Consolidated crossfade parameter finalization ──────────────

  private _finalizeCrossfadeParams(volume: number, outgoingSound: ISound): CrossfadeParams {
    const duration = outgoingSound.duration();
    const trailingSilence = this._currentAnalysis?.analysis.energy.trailingSilence ?? 0;
    const effectiveEnd = duration - trailingSilence;

    let crossfadeDuration = this._crossfadeDuration;

    let effectiveCurve: CrossfadeCurve = this._settingsCurve;
    let effectiveFadeInOnly = this._outroType === "fadeOut" || this._outroType === "loopFade";
    let effectiveInShape = 1;
    let effectiveOutShape = 1;

    const outroConfidence = this._currentAnalysis?.analysis.outro?.outroConfidence ?? 0;
    if (this._settingsSmartCurve && this._outroType && outroConfidence >= 0.75) {
      const profile = getOutroTypeCrossfadeProfile(this._outroType);
      effectiveCurve = profile.curve;
      effectiveFadeInOnly = profile.fadeInOnly;
      effectiveInShape = profile.inShape ?? 1;
      effectiveOutShape = profile.outShape ?? 1;
    }

    // Incoming intro adjustments
    const incomingIntro = this._nextAnalysis?.analysis.intro;
    if (incomingIntro) {
      if (incomingIntro.quietIntroDuration > crossfadeDuration) {
        crossfadeDuration = Math.min(
          incomingIntro.quietIntroDuration,
          this._settingsCrossfadeDuration,
        );
      }
    }

    // Use CompatibilityScorer for unified scoring
    const compatScore = this._compatScorer.computeOverall(
      this._currentAnalysis?.analysis ?? null,
      this._nextAnalysis?.analysis ?? null,
    );
    const strategy = this._compatScorer.computeTransitionStrategy(compatScore, this._outroType);

    // Store strategy for reuse in _doCrossfade (avoid recomputing compatibility)
    this._lastStrategy = strategy;

    // Apply compatibility-based duration adjustment
    crossfadeDuration *= strategy.durationMultiplier;

    // Energy contrast: handle large volume differences
    const energyContrast = this._computeEnergyContrast();
    if (energyContrast > 6) {
      crossfadeDuration = Math.min(crossfadeDuration * 1.3, this._settingsCrossfadeDuration);
    } else if (energyContrast > 3) {
      crossfadeDuration = Math.min(crossfadeDuration * 1.2, this._settingsCrossfadeDuration);
    } else if (energyContrast < 0.33) {
      crossfadeDuration = Math.min(crossfadeDuration * 1.15, this._settingsCrossfadeDuration);
    }

    // Clamp final duration
    crossfadeDuration = Math.max(
      MIN_CROSSFADE_DURATION,
      Math.min(crossfadeDuration, this._getEffectiveCrossfadeDuration(effectiveEnd)),
    );

    // Safety clamp: account for async delay
    const outgoingNow = outgoingSound.seek() as number;
    const remainingContent = Math.max(0, effectiveEnd - outgoingNow);
    if (crossfadeDuration > remainingContent) {
      if (IS_DEV) {
        console.log(
          `TransitionStateMachine: Safety-clamped crossfade ${crossfadeDuration.toFixed(1)}s → ` +
            `${Math.max(0.5, remainingContent).toFixed(1)}s`,
        );
      }
      crossfadeDuration = Math.max(0.5, remainingContent);
    }

    const incomingGainAdjustment = this._computeIncomingGainAdjustment(energyContrast);
    const overlapHeadroomDb = this._computeOverlapHeadroomDb(
      energyContrast,
      compatScore.loudness,
      effectiveFadeInOnly,
    );

    // Filter sweep replaces spectral EQ (both serve frequency-domain smoothing;
    // filter sweep is far more aggressive for truly incompatible tracks)
    let spectralCrossfade: SpectralCrossfadeData | false = false;
    if (this._settingsSmartCurve && !effectiveFadeInOnly && !strategy.useFilterSweep) {
      spectralCrossfade = this._computeSpectralCrossfadeData();
    }

    // Apply curve/shape overrides from strategy
    if (strategy.recommendedCurve && outroConfidence < 0.75) {
      // No strong outro detection → use recommended curve
      effectiveCurve = strategy.recommendedCurve;
    }
    if (strategy.shapeOverride) {
      if (outroConfidence >= 0.75) {
        // Blend: average of outro profile and strategy override
        effectiveInShape = (effectiveInShape + strategy.shapeOverride.inShape) / 2;
        effectiveOutShape = (effectiveOutShape + strategy.shapeOverride.outShape) / 2;
      } else {
        effectiveInShape = strategy.shapeOverride.inShape;
        effectiveOutShape = strategy.shapeOverride.outShape;
      }
      // Safety clamp per anti-overfitting principle
      effectiveInShape = Math.max(0.7, Math.min(1.3, effectiveInShape));
      effectiveOutShape = Math.max(0.7, Math.min(1.3, effectiveOutShape));
    }

    if (IS_DEV) {
      console.log(
        `TransitionStateMachine: Finalized params — duration=${crossfadeDuration.toFixed(1)}s, ` +
          `curve=${effectiveCurve}, inShape=${effectiveInShape.toFixed(2)}, ` +
          `outShape=${effectiveOutShape.toFixed(2)}, ` +
          `gainAdj=${incomingGainAdjustment.toFixed(3)}, ` +
          `headroom=${overlapHeadroomDb.toFixed(1)}dB, ` +
          `compat=${compatScore.overall.toFixed(2)}, ` +
          `spectral=${spectralCrossfade !== false}`,
      );
    }

    return {
      duration: crossfadeDuration,
      curve: effectiveCurve,
      incomingGain: volume,
      outgoingGain: volume,
      fadeInOnly: effectiveFadeInOnly,
      outroType: this._outroType ?? undefined,
      inShape: effectiveInShape,
      outShape: effectiveOutShape,
      incomingGainAdjustment,
      overlapHeadroomDb,
      spectralCrossfade,
    };
  }

  // ─── State: WAITING ────────────────────────────────────────────

  private _handleWaiting(currentTime: number): void {
    if (this._nativeAutoMixReady) {
      if (!this._nativeForceStartSent && currentTime >= this._crossfadeStartTime + 0.25) {
        this._nativeForceStartSent = true;
        getAudioBackendTransport().sendOrQueue({ type: "automixForceStart" });
      }
      return;
    }

    if (!this._preBufferManager.isBuffering && !this._preBufferManager.hasBuffer) {
      this._preBufferManager.startPreBuffer(
        this._musicStoreRef,
        this._analysisCache,
        { volumeNorm: this._settingsVolumeNorm, bpmMatch: this._settingsBpmMatch },
        () => this._state,
      );
      // Store pre-buffered analysis for _finalizeCrossfadeParams()
      if (this._preBufferManager.preBufferedAnalysis) {
        this._nextAnalysis = this._preBufferManager.preBufferedAnalysis;
      }
    }

    // Update next analysis from pre-buffer if available
    if (this._preBufferManager.preBufferedAnalysis && !this._nextAnalysis) {
      this._nextAnalysis = this._preBufferManager.preBufferedAnalysis;
    }

    if (currentTime >= this._crossfadeStartTime) {
      if (this._shouldDeferCrossfade(currentTime)) {
        return;
      }

      const remaining = this._effectiveEnd - currentTime;
      if (remaining < this._crossfadeDuration && remaining >= 1) {
        this._crossfadeDuration = remaining;
        if (IS_DEV) {
          console.log(
            `TransitionStateMachine: Clamped crossfade duration to ${remaining.toFixed(1)}s ` +
              `(remaining content time after energy gate deferral)`,
          );
        }
      }

      this._initiateCrossfade();
    }
  }

  /**
   * Energy gate + vocal guard: defer crossfade start if conditions not met.
   */
  private _shouldDeferCrossfade(currentTime: number): boolean {
    // Skip the gate for types where the audio IS already declining
    if (
      this._outroType === "fadeOut" ||
      this._outroType === "silence" ||
      this._outroType === "reverbTail" ||
      this._outroType === "loopFade"
    ) {
      return false;
    }

    // ── Vocal activity guard ──
    if (this._settingsVocalGuard && this._outroType) {
      const outroMB = this._currentAnalysis?.analysis.outro?.multibandEnergy;
      if (
        outroMB &&
        this._vocalGuard.shouldDeferForVocals(
          currentTime,
          this._crossfadeStartTime,
          this._effectiveEnd,
          outroMB,
          this._crossfadeDuration,
        )
      ) {
        return true;
      }
    }

    // ── Energy gate ──
    const energy = this._currentAnalysis?.analysis.energy;
    if (!energy) return false;

    const maxDefer = Math.min(this._crossfadeDuration * 0.5, 5);
    const maxDeferByRemaining = Math.max(
      0,
      this._effectiveEnd - this._crossfadeStartTime - MIN_CROSSFADE_DURATION,
    );
    if (currentTime >= this._crossfadeStartTime + Math.min(maxDefer, maxDeferByRemaining))
      return false;

    const sec = Math.floor(currentTime);
    if (sec < 3 || sec >= energy.energyPerSecond.length) return false;

    const e3sAgo = energy.energyPerSecond[sec - 3];
    const e1sAgo = energy.energyPerSecond[sec - 1];
    const eNow = energy.energyPerSecond[sec];

    if (eNow < energy.averageEnergy * 0.5) return false;
    if (e3sAgo > 0.05 && eNow / e3sAgo < 0.75) return false;
    if (e3sAgo > e1sAgo && e1sAgo > eNow && eNow / e3sAgo < 0.85) return false;

    if (IS_DEV) {
      console.log(
        `TransitionStateMachine: Energy gate deferred crossfade ` +
          `(e=${eNow.toFixed(2)}, avg=${energy.averageEnergy.toFixed(2)}, ` +
          `e3sAgo=${e3sAgo.toFixed(2)}, maxDefer=${Math.min(maxDefer, maxDeferByRemaining).toFixed(1)}s)`,
      );
    }
    return true;
  }

  // ─── State: CROSSFADING ────────────────────────────────────────

  private _initiateCrossfade(): void {
    this._state = "crossfading";
    this._updateStoreState();

    this._doCrossfade().catch((err) => {
      console.error(
        "TransitionStateMachine: Crossfade failed, falling back to normal transition",
        err,
      );
      this._lastFailureTime = Date.now();

      const currentSound = SoundManager.getCurrentSound();
      const songAlreadyEnded = currentSound && !currentSound.playing();

      this.cancelCrossfade();

      if (songAlreadyEnded && this._musicStoreRef) {
        if (IS_DEV) {
          console.log(
            "TransitionStateMachine: Song ended during failed crossfade, triggering normal transition",
          );
        }
        this._musicStoreRef.setPlaySongIndex("next");
      }
    });
  }

  private async _doCrossfade(): Promise<void> {
    const music = this._musicStoreRef;
    if (!music) throw new Error("No music store");

    const playlist = music.persistData.playlists;
    const currentIndex = music.persistData.playSongIndex;
    const listLength = playlist.length;

    const nextIndex = this._selectNextIndex(
      currentIndex,
      listLength,
      music.persistData.playSongMode,
    );

    const nextSong = playlist[nextIndex];
    if (!nextSong) throw new Error("No next song");
    const transitionId = this._beginTransition("web", currentIndex, nextIndex, "crossfading");

    let incomingSound: BufferedSound;
    let incomingSourceUrl: string | null = null;

    // ★ Fast path: use pre-buffered sound
    const preBuffered = this._preBufferManager.consume(nextIndex);
    if (preBuffered) {
      incomingSound = preBuffered.sound;
      incomingSourceUrl = preBuffered.sourceUrl;
      this._nextAnalysis = preBuffered.analysis;

      if (IS_DEV) {
        console.log(`TransitionStateMachine: Using pre-buffered sound for "${nextSong.name}"`);
      }
    } else {
      // Slow path: fallback
      this._preBufferManager.cleanup();

      const resolved = await resolveSongUrl(nextSong);
      if (!resolved?.url) throw new Error("Failed to resolve music URL");

      if (!this._isTransitionActive(transitionId, "web") || this._state !== "crossfading") return;

      const url = resolved.url;
      incomingSourceUrl = url;

      incomingSound = new BufferedSound({
        src: [url],
        preload: true,
        volume: 0,
      });

      await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => reject(new Error("Load timeout")), 30000);
        incomingSound.once("load", () => {
          clearTimeout(timeout);
          resolve();
        });
        incomingSound.once("loaderror", () => {
          clearTimeout(timeout);
          reject(new Error("Load error"));
        });
      });

      if (!this._isTransitionActive(transitionId, "web") || this._state !== "crossfading") return;

      if (this._settingsVolumeNorm) {
        const nextSongId = nextSong.id;
        const cachedNext = this._analysisCache.get(nextSongId);
        if (cachedNext) {
          this._nextAnalysis = cachedNext;
        } else {
          try {
            const blobUrl = incomingSound.getBlobUrl();
            if (blobUrl) {
              const analysis = await analyzeTrack(blobUrl, { analyzeBPM: this._settingsBpmMatch });
              this._nextAnalysis = { songId: nextSongId, analysis };
              this._addToCache(this._nextAnalysis);
            }
          } catch (err) {
            if (IS_DEV) console.warn("TransitionStateMachine: Incoming track analysis failed", err);
          }
        }
      }

      if (!this._isTransitionActive(transitionId, "web") || this._state !== "crossfading") return;

      await incomingSound.ensureAudioGraph();

      if (!this._isTransitionActive(transitionId, "web") || this._state !== "crossfading") return;
    }

    this._incomingSound = incomingSound;
    this._incomingSourceUrl = incomingSourceUrl;

    const outgoingSound = SoundManager.getCurrentSound();
    if (!outgoingSound) throw new Error("No outgoing sound");

    // Store pending index early — used as fallback by _onCrossfadeComplete
    // if _waitForPlayStart is delayed (background tab throttling).
    this._pendingNextIndex = nextIndex;

    // Register the outgoing 'end' safety net EARLY
    let outgoingEndedEarly = false;
    outgoingSound.once("end", () => {
      if (this._isTransitionActive(transitionId, "web") && this._state === "crossfading") {
        if (this._crossfadeScheduler.isActive()) {
          if (IS_DEV) {
            console.log(
              "TransitionStateMachine: Outgoing song ended during crossfade, force-completing",
            );
          }
          this._crossfadeScheduler.forceComplete();
        } else {
          outgoingEndedEarly = true;
          if (IS_DEV) {
            console.log("TransitionStateMachine: Outgoing song ended before crossfade scheduled");
          }
        }
      }
    });

    SoundManager.beginTransition(incomingSound);
    incomingSound.play();
    this._commitWebTransition(transitionId, incomingSound, nextIndex);

    const volume = music.persistData.playVolume;
    const params = this._finalizeCrossfadeParams(volume, outgoingSound);

    if (outgoingEndedEarly) {
      params.duration = 0.5;
      params.fadeInOnly = true;
    }

    const outgoingGain = this._getGainNode(outgoingSound);
    const incomingGain = this._getGainNode(incomingSound);

    if (outgoingGain && incomingGain) {
      this._crossfadeScheduler.scheduleFullCrossfade(outgoingGain, incomingGain, params, () =>
        this._onCrossfadeComplete(transitionId),
      );

      // ── Transition effects ──
      if (this._settingsTransitionEffects) {
        const strategy = this._lastStrategy;
        const audioCtx = AudioContextManager.getContext();

        if (audioCtx && strategy && strategy.useEffects) {
          const now = audioCtx.currentTime;
          const bpm = this._currentAnalysis?.analysis.bpm?.bpm;
          const advancedIntensity = strategy.advancedIntensity;

          // Filter sweep (replaces spectral EQ for low-compat transitions)
          if (strategy.useFilterSweep && outgoingGain && incomingGain) {
            this._transitionEffects.createFilterSweep(
              audioCtx,
              outgoingGain,
              incomingGain,
              strategy.filterSweepIntensity,
              now,
              params.duration,
              params.fadeInOnly ?? false,
            );
          }

          // Reverb tail (with enhanced gain for low compat when filter sweep is active)
          if (strategy.useReverbTail && outgoingGain) {
            const decayTime = this._outroType === "hard" ? 1.5 : 2.5;
            this._transitionEffects.createReverbTail(
              audioCtx,
              outgoingGain,
              decayTime,
              now,
              params.duration,
            );
          }

          if (strategy.useEchoThrow && outgoingGain) {
            const echoDuration = Math.min(4.0, Math.max(1.0, params.duration * 0.55));
            this._transitionEffects.createEchoThrow(
              audioCtx,
              outgoingGain,
              echoDuration,
              now,
              bpm,
              advancedIntensity,
            );
          }

          if (strategy.useReverseSwell) {
            const swellDuration = Math.min(2.5, Math.max(0.8, params.duration * 0.35));
            this._transitionEffects.createReverseSwell(
              audioCtx,
              swellDuration,
              now,
              bpm,
              advancedIntensity,
            );
          }

          if (
            strategy.useBeatGate &&
            !strategy.useFilterSweep &&
            params.spectralCrossfade === false &&
            incomingGain
          ) {
            const gateDuration = Math.min(2.5, Math.max(0.6, params.duration * 0.35));
            this._transitionEffects.createBeatGate(
              audioCtx,
              incomingGain,
              gateDuration,
              now,
              bpm,
              Math.min(0.75, advancedIntensity),
            );
          }

          // Noise riser (with enhanced duration for low compat)
          if (strategy.useNoiseRiser) {
            const maxRiser = 1.5 + strategy.filterSweepIntensity * 1.0;
            const riserDuration = Math.min(maxRiser, params.duration * 0.25);
            this._transitionEffects.createNoiseRiser(audioCtx, riserDuration, now, bpm);
          }
        }
      }
    } else {
      // Fallback: software fade
      if (this._outroType !== "fadeOut" && this._outroType !== "loopFade") {
        outgoingSound.fade(volume, 0, params.duration * 1000);
      }
      incomingSound.fade(0, volume * (params.incomingGainAdjustment ?? 1), params.duration * 1000);
      this._softwareFadeStartedAt = Date.now();
      this._softwareFadeRemaining = params.duration * 1000;
      this._softwareFadeTimerId = setTimeout(() => {
        this._softwareFadeTimerId = null;
        this._onCrossfadeComplete(transitionId);
      }, this._softwareFadeRemaining);
    }

    await this._waitForPlayStart(incomingSound);

    if (
      !this._isTransitionActive(transitionId, "web") ||
      (this._state !== "crossfading" && this._state !== "finishing")
    )
      return;

    if (this._isPaused) {
      await this._waitForUnpause();
      if (
        !this._isTransitionActive(transitionId, "web") ||
        (this._state !== "crossfading" && this._state !== "finishing")
      )
        return;
    }

    if (IS_DEV) {
      console.log(`TransitionStateMachine: Crossfade started → "${nextSong.name}"`);
    }
  }

  private _commitWebTransition(
    transitionId: number,
    incomingSound: ISound,
    nextIndex: number,
  ): void {
    if (!this._isTransitionActive(transitionId, "web")) return;
    const active = this._activeTransition;
    if (active?.committed) return;
    active.committed = true;

    const music = this._musicStoreRef;
    if (music) {
      if (music.persistData.playSongIndex !== nextIndex) {
        music.persistData.playSongIndex = nextIndex;
      }
      if (typeof music.resetSongLyricState === "function") {
        music.resetSongLyricState();
      }
    }

    window.$player = incomingSound;
    adoptIncomingSound(incomingSound);
    this._pendingNextIndex = -1;
  }

  private _waitForPlayStart(sound: ISound): Promise<void> {
    return new Promise((resolve) => {
      if (sound.playing()) {
        resolve();
        return;
      }

      let resolved = false;
      const done = () => {
        if (resolved) return;
        resolved = true;
        resolve();
      };

      const retryTimer = setTimeout(() => {
        if (!resolved && !sound.playing() && !this._isPaused) {
          if (IS_DEV) {
            console.warn("TransitionStateMachine: Play not started after 2s, retrying");
          }
          sound.play();
        }
      }, 2000);

      const deadline = setTimeout(() => {
        clearTimeout(retryTimer);
        if (!resolved) {
          if (IS_DEV) {
            console.warn("TransitionStateMachine: Play confirmation timeout, proceeding anyway");
          }
          done();
        }
      }, 3000);

      sound.once("play", () => {
        clearTimeout(retryTimer);
        clearTimeout(deadline);
        done();
      });
    });
  }

  private _waitForUnpause(): Promise<void> {
    if (!this._isPaused) return Promise.resolve();
    return new Promise((resolve) => {
      this._unpauseResolve = resolve;
    });
  }

  private _getGainNode(sound: ISound): GainNode | null {
    if (sound instanceof BufferedSound) {
      const inner = sound.getInnerSound();
      if (inner) {
        return inner.getGainNode();
      }
    }
    if ("getGainNode" in sound && typeof (sound as any).getGainNode === "function") {
      return (sound as any).getGainNode();
    }
    return null;
  }

  // ─── State: FINISHING ──────────────────────────────────────────

  private _onCrossfadeComplete(transitionId?: number): void {
    if (transitionId !== undefined && !this._isTransitionActive(transitionId, "web")) return;
    if (this._softwareFadeTimerId !== null) {
      clearTimeout(this._softwareFadeTimerId);
      this._softwareFadeTimerId = null;
    }
    this._softwareFadeRemaining = 0;
    this._isPaused = false;

    if (transitionId !== undefined) {
      this._setTransitionPhase(transitionId, "finishing");
    }
    this._state = "finishing";
    this._updateStoreState();

    // Clean up transition effects
    this._transitionEffects.cleanup();

    const outgoing = SoundManager.getOutgoingSound();
    SoundManager.unloadOutgoing();
    if (outgoing) {
      setTimeout(() => {
        outgoing.stop();
        outgoing.unload();
      }, 50);
    }

    const currentSound = SoundManager.getCurrentSound();
    if (currentSound && this._musicStoreRef) {
      const userVolume = this._musicStoreRef.persistData.playVolume;
      currentSound.volume(userVolume);
    }

    this._activeGainAdjustment = this._crossfadeScheduler.getIncomingGainAdjustment();

    if (currentSound && this._activeGainAdjustment !== 1) {
      const gainNode = this._getGainNode(currentSound);
      if (gainNode && this._musicStoreRef) {
        const userVolume = this._musicStoreRef.persistData.playVolume;
        gainNode.gain.value = userVolume * this._activeGainAdjustment;
      }
    }

    // Fallback: if _waitForPlayStart was delayed (background tab), the normal
    // _doCrossfade path may not have updated the store yet. Apply pending updates
    // so the debounced watcher in Player/index.vue sees isCrossfading()=true.
    if (this._pendingNextIndex >= 0 && this._musicStoreRef) {
      const music = this._musicStoreRef;
      if (music.persistData.playSongIndex !== this._pendingNextIndex) {
        music.persistData.playSongIndex = this._pendingNextIndex;
        if (typeof music.resetSongLyricState === "function") {
          music.resetSongLyricState();
        }
        if (IS_DEV) {
          console.log("TransitionStateMachine: Applied pending playSongIndex (bg tab fallback)");
        }
      }
      if (currentSound && window.$player !== currentSound) {
        window.$player = currentSound;
        if (transitionId === undefined || this._isTransitionActive(transitionId, "web")) {
          adoptIncomingSound(currentSound);
        }
      }
      this._pendingNextIndex = -1;
    }

    const nativeHandoffUrl = this._incomingSourceUrl;
    if (currentSound && nativeHandoffUrl) {
      void handoffAutoMixToNativeBackend(currentSound, nativeHandoffUrl);
    }

    this._currentAnalysis = this._nextAnalysis;
    this._nextAnalysis = null;
    this._incomingSound = null;
    this._incomingSourceUrl = null;
    this._lastStrategy = null;
    this._phraseMixOutTime = null;
    this._evictCache();

    if (this._finishingTimerId !== null) {
      clearTimeout(this._finishingTimerId);
    }
    if (transitionId !== undefined) {
      this._holdTransition(transitionId);
    }
    this._finishingTimerId = setTimeout(() => {
      this._finishingTimerId = null;
      if (this._state === "finishing") {
        this._state = "idle";
        this._updateStoreState();
        if (IS_DEV) {
          console.log("TransitionStateMachine: Finishing → idle (delayed transition)");
        }
      }
    }, 800);

    if (IS_DEV) {
      console.log("TransitionStateMachine: Crossfade complete, entering finishing hold (800ms)");
    }
  }

  // ─── Cancel ────────────────────────────────────────────────────

  cancelCrossfade(): void {
    if (this._state === "idle") return;

    if (IS_DEV) {
      console.log("TransitionStateMachine: Cancelling crossfade");
    }

    this._crossfadeScheduler.cancel();
    this._transitionEffects.cleanup();

    if (this._softwareFadeTimerId !== null) {
      clearTimeout(this._softwareFadeTimerId);
      this._softwareFadeTimerId = null;
    }
    this._softwareFadeRemaining = 0;

    if (this._finishingTimerId !== null) {
      clearTimeout(this._finishingTimerId);
      this._finishingTimerId = null;
    }
    if (this._nativeFinishTimerId !== null) {
      clearTimeout(this._nativeFinishTimerId);
      this._nativeFinishTimerId = null;
    }

    this._preBufferManager.cleanup();
    this._nativeAutoMixPreparing = false;
    this._nativeAutoMixReady = false;
    this._nativeForceStartSent = false;
    this._nativeNextIndex = -1;
    this._nativeHandoffHoldUntil = 0;
    this._clearTransition();
    if (isTauri()) {
      try {
        getAudioBackendTransport().sendOrQueue({ type: "automixCancel" });
      } catch {
        /* ignore: transport may not be connected during teardown */
      }
    }

    if (this._incomingSound) {
      const incoming = this._incomingSound;

      if (SoundManager.getCurrentSound() === incoming) {
        const outgoing = SoundManager.getOutgoingSound();
        if (outgoing) {
          SoundManager.revertTransition();
          if (this._musicStoreRef) {
            outgoing.volume(this._musicStoreRef.persistData.playVolume);
          }
          window.$player = outgoing;
        }
      } else {
        incoming.stop();
        incoming.unload();
      }

      this._incomingSound = null;
    }
    this._incomingSourceUrl = null;

    this._analyzingInFlight = false;
    this._nextAnalysis = null;
    this._lastStrategy = null;
    this._pendingNextIndex = -1;
    this._isPaused = false;
    this._activeGainAdjustment = 1;
    this._phraseMixOutTime = null;
    this._state = "idle";
    this._updateStoreState();

    if (this._unpauseResolve) {
      this._unpauseResolve();
      this._unpauseResolve = null;
    }
  }

  pauseCrossfade(): boolean {
    if (this._state !== "crossfading" || this._isPaused) return false;

    if (this._crossfadeScheduler.isActive()) {
      this._isPaused = true;
      this._crossfadeScheduler.pauseCrossfade();

      // Pause transition effects
      const audioCtx = AudioContextManager.getContext();
      if (audioCtx) {
        this._transitionEffects.pause(audioCtx);
      }

      if (this._softwareFadeTimerId !== null) {
        clearTimeout(this._softwareFadeTimerId);
        this._softwareFadeTimerId = null;
        const elapsed = Date.now() - this._softwareFadeStartedAt;
        this._softwareFadeRemaining = Math.max(0, this._softwareFadeRemaining - elapsed);
      }

      SoundManager.getCurrentSound()?.pause();
      SoundManager.getOutgoingSound()?.pause();

      if (IS_DEV) {
        console.log("TransitionStateMachine: Crossfade paused (frozen)");
      }
      return true;
    } else {
      if (IS_DEV) {
        console.log("TransitionStateMachine: Crossfade paused during setup — cancelling");
      }
      this.cancelCrossfade();
      return false;
    }
  }

  resumeCrossfade(): void {
    if (this._state !== "crossfading" || !this._isPaused) return;
    this._isPaused = false;

    SoundManager.getOutgoingSound()?.play();
    SoundManager.getCurrentSound()?.play();

    this._crossfadeScheduler.resumeCrossfade();

    // Resume transition effects
    const audioCtx = AudioContextManager.getContext();
    if (audioCtx) {
      this._transitionEffects.resume(audioCtx);
    }

    if (this._softwareFadeRemaining > 0 && !this._crossfadeScheduler.isActive()) {
      this._softwareFadeStartedAt = Date.now();
      const transitionId =
        this._activeTransition?.mode === "web" ? this._activeTransition.id : undefined;
      this._softwareFadeTimerId = setTimeout(() => {
        this._softwareFadeTimerId = null;
        this._onCrossfadeComplete(transitionId);
      }, this._softwareFadeRemaining);
    }

    if (this._unpauseResolve) {
      this._unpauseResolve();
      this._unpauseResolve = null;
    }

    if (IS_DEV) {
      console.log("TransitionStateMachine: Crossfade resumed");
    }
  }

  // ─── Track lifecycle hooks ─────────────────────────────────────

  onTrackStarted(sound: ISound, songId: number): void {
    if (this._state === "crossfading" || this._state === "finishing") return;

    this._activeGainAdjustment = 1;
    this._preBufferManager.cleanup();

    this._state = "idle";
    this._lastFailureTime = 0;
    this._updateStoreState();

    if (this._isNativeAutoMixSound(sound)) {
      return;
    }

    if (
      this._enabled &&
      (sound instanceof BufferedSound ||
        !!(sound as { getSourceUrl?: () => string | null }).getSourceUrl?.())
    ) {
      this._preAnalyzeTrack(sound, songId);
    }
  }

  private _preAnalyzeTrack(sound: ISound, songId: number): void {
    if (this._analysisCache.has(songId)) return;

    const doAnalysis = async () => {
      let analysisUrl = this._getSoundAnalysisUrl(sound);
      if (!analysisUrl && sound instanceof BufferedSound) {
        await new Promise<void>((resolve) => {
          sound.once("load", resolve);
          setTimeout(resolve, 30000);
        });
        analysisUrl = this._getSoundAnalysisUrl(sound);
      }
      if (!analysisUrl) return;

      const analysis = await analyzeTrack(analysisUrl, {
        analyzeBPM: this._settingsBpmMatch,
      });
      this._addToCache({ songId, analysis });

      if (IS_DEV) {
        console.log(`TransitionStateMachine: Pre-analyzed track ${songId}`);
      }
    };

    doAnalysis().catch((err) => {
      if (IS_DEV) {
        console.warn(`TransitionStateMachine: Pre-analysis failed for ${songId}`, err);
      }
    });
  }

  // ─── Cache management ──────────────────────────────────────────

  private _addToCache(entry: CachedAnalysis): void {
    this._analysisCache.set(entry.songId, entry);
    this._evictCache();
  }

  private _evictCache(): void {
    if (this._analysisCache.size <= MAX_CACHE_SIZE) return;
    const keys = Array.from(this._analysisCache.keys());
    const toRemove = keys.length - MAX_CACHE_SIZE;
    for (let i = 0; i < toRemove; i++) {
      this._analysisCache.delete(keys[i]);
    }
  }

  // ─── Store state update ────────────────────────────────────────

  private _updateStoreState(): void {
    if (!this._musicStoreRef) return;

    const outro = this._currentAnalysis?.analysis.outro;
    const progress =
      this._state === "crossfading" || this._state === "finishing"
        ? this._crossfadeScheduler.getProgress()
        : -1;

    let incomingSongName: string | null = null;
    let incomingSongId: number | null = null;
    if (this._state === "crossfading" || this._state === "waiting") {
      const playlist = this._musicStoreRef.persistData?.playlists;
      const currentIndex = this._musicStoreRef.persistData?.playSongIndex;
      if (playlist && currentIndex !== null && currentIndex !== undefined) {
        const nextIndex =
          this._musicStoreRef.persistData.playSongMode === "random"
            ? -1
            : (currentIndex + 1) % playlist.length;
        if (nextIndex >= 0 && playlist[nextIndex]) {
          incomingSongName = playlist[nextIndex].name;
          incomingSongId = playlist[nextIndex].id;
        }
      }
    }

    this._musicStoreRef.autoMixState = {
      phase: this._state,
      outroType: this._outroType ?? null,
      outroConfidence: outro?.outroConfidence ?? 0,
      crossfadeStartTime: this._crossfadeStartTime,
      crossfadeDuration: this._crossfadeDuration,
      crossfadeProgress: progress,
      incomingSongName,
      incomingSongId,
    };
  }

  // ─── Cleanup ───────────────────────────────────────────────────

  destroy(): void {
    this.cancelCrossfade();
    if (this._finishingTimerId !== null) {
      clearTimeout(this._finishingTimerId);
      this._finishingTimerId = null;
    }
    if (this._nativeUnsubscribe !== null) {
      this._nativeUnsubscribe();
      this._nativeUnsubscribe = null;
    }
    this._analysisCache.clear();
    this._transitionEffects.cleanup();
  }
}
