/**
 * AutoMixEngine - Core crossfade orchestrator for Apple Music-like AutoMix
 *
 * Manages the crossfade lifecycle:
 *   IDLE → ANALYZING → WAITING → CROSSFADING → FINISHING → IDLE
 *
 * Key design decisions:
 *   - monitorPlayback() is SYNCHRONOUS — called per RAF frame, must never block
 *   - All heavy analysis runs in a Web Worker via TrackAnalyzer
 *   - Store references loaded once lazily (not per frame)
 *   - Single analyzeTrack() call returns volume + energy + BPM + fingerprint (no double decode)
 *   - Never creates new AudioContexts — uses the global one
 */

import { CrossfadeManager, getOutroTypeCrossfadeProfile, type CrossfadeCurve, type CrossfadeParams } from './CrossfadeManager';
import { analyzeTrack, spectralSimilarity, type TrackAnalysis, type OutroType } from './TrackAnalyzer';
import { findNearestBeat, type BPMResult } from './BPMDetector';
import { BufferedSound } from './BufferedSound';
import { SoundManager } from './SoundManager';
import type { ISound } from './types';

const IS_DEV = import.meta.env?.DEV ?? false;

export type AutoMixState = 'idle' | 'analyzing' | 'waiting' | 'crossfading' | 'finishing';

interface CachedAnalysis {
  songId: number;
  analysis: TrackAnalysis;
}

/** Seconds before expected crossfade to start pre-analysis */
const PREPARE_AHEAD = 15;

/** Minimum crossfade duration in seconds */
const MIN_CROSSFADE_DURATION = 2;

/** Maximum entries in analysis cache */
const MAX_CACHE_SIZE = 10;

/**
 * AutoMixEngine - Singleton orchestrator
 */
export class AutoMixEngine {
  private _state: AutoMixState = 'idle';
  private _crossfadeManager: CrossfadeManager = new CrossfadeManager();
  private _analysisCache: Map<number, CachedAnalysis> = new Map();

  // Current transition data
  private _currentAnalysis: CachedAnalysis | null = null;
  private _nextAnalysis: CachedAnalysis | null = null;
  private _crossfadeStartTime: number = 0;
  private _crossfadeDuration: number = 8;
  private _outroType: OutroType | null = null;

  // Incoming sound during crossfade
  private _incomingSound: ISound | null = null;

  // Pre-buffered incoming sound (prepared during WAITING, consumed during CROSSFADING)
  private _preBufferedSound: BufferedSound | null = null;
  private _preBufferedSongIndex: number = -1;
  private _preBufferedAnalysis: CachedAnalysis | null = null;
  private _preBuffering: boolean = false;

  // Settings cache (refreshed from store)
  private _enabled: boolean = false;
  private _settingsCrossfadeDuration: number = 8;
  private _settingsCurve: CrossfadeCurve = 'equalPower';
  private _settingsBpmMatch: boolean = true;
  private _settingsBeatAlign: boolean = true;
  private _settingsVolumeNorm: boolean = true;
  private _settingsSmartCurve: boolean = true;

  // Async guard: prevent duplicate analysis
  private _analyzingInFlight: boolean = false;

  // Failure cooldown: prevent retry loops after crossfade failure
  private _lastFailureTime: number = 0;

  // Pause state during crossfade
  private _isPaused: boolean = false;

  // Software fade fallback timeout tracking
  private _softwareFadeTimerId: ReturnType<typeof setTimeout> | null = null;
  private _softwareFadeStartedAt: number = 0;
  private _softwareFadeRemaining: number = 0;

  // Store references (loaded once lazily)
  private _musicStoreRef: any = null;
  private _settingStoreRef: any = null;
  private _storesReady: boolean = false;
  private _storesLoading: boolean = false;

  constructor() {
    if (IS_DEV) {
      console.log('AutoMixEngine: Created');
    }
  }

  // ─── Public getters ────────────────────────────────────────────

  getState(): AutoMixState {
    return this._state;
  }

  isCrossfading(): boolean {
    return this._state === 'crossfading' || this._state === 'finishing';
  }

  getCrossfadeProgress(): number {
    return this._crossfadeManager.getProgress();
  }

  // ─── Store loading (lazy, one-time) ────────────────────────────

  /**
   * Kick off async store loading. Returns immediately.
   * Stores become available after the dynamic import resolves.
   */
  private _loadStores(): void {
    if (this._storesLoading || this._storesReady) return;
    this._storesLoading = true;

    Promise.all([
      import('@/store/musicData'),
      import('@/store/settingData'),
    ]).then(([musicMod, settingMod]) => {
      this._musicStoreRef = musicMod.default();
      this._settingStoreRef = settingMod.default();
      this._storesReady = true;
      if (IS_DEV) {
        console.log('AutoMixEngine: Stores loaded');
      }
    }).catch((err) => {
      console.error('AutoMixEngine: Failed to load stores', err);
    }).finally(() => {
      this._storesLoading = false;
    });
  }

  /**
   * Refresh settings from store (cheap property reads).
   */
  private _refreshSettings(): void {
    if (!this._settingStoreRef) return;
    this._enabled = this._settingStoreRef.autoMixEnabled ?? false;
    this._settingsCrossfadeDuration = this._settingStoreRef.autoMixCrossfadeDuration ?? 8;
    this._settingsCurve = this._settingStoreRef.autoMixTransitionStyle ?? 'equalPower';
    this._settingsBpmMatch = this._settingStoreRef.autoMixBpmMatch ?? true;
    this._settingsBeatAlign = this._settingStoreRef.autoMixBeatAlign ?? true;
    this._settingsVolumeNorm = this._settingStoreRef.autoMixVolumeNorm ?? true;
    this._settingsSmartCurve = this._settingStoreRef.autoMixSmartCurve ?? true;
  }

  /**
   * Check all preconditions for AutoMix to be active.
   */
  private _shouldBeActive(): boolean {
    if (!this._enabled) return false;
    if (!this._musicStoreRef) return false;

    const music = this._musicStoreRef;
    if (music.persistData.playSongMode === 'single') return false;
    if (music.persistData.personalFmMode) return false;
    if (music.persistData.playlists.length < 2) return false;

    return true;
  }

  // ─── Core: monitorPlayback (SYNCHRONOUS) ───────────────────────

  /**
   * Called per RAF frame from the spectrum update loop.
   * MUST be synchronous — never awaits, never blocks.
   * All async work is kicked off as fire-and-forget promises.
   */
  monitorPlayback(currentSound: ISound): void {
    // Lazy-load stores on first call (async, returns immediately)
    if (!this._storesReady) {
      this._loadStores();
      return; // Stores not ready yet — skip this frame
    }

    this._refreshSettings();

    if (!this._shouldBeActive()) {
      if (this._state !== 'idle') {
        this.cancelCrossfade();
      }
      return;
    }

    if (!currentSound || !currentSound.playing()) return;

    const currentTime = currentSound.seek() as number;
    const duration = currentSound.duration();
    if (!duration || duration <= 0) return;

    switch (this._state) {
      case 'idle':
        this._handleIdle(currentTime, duration);
        break;
      case 'analyzing':
        // async analysis in flight — nothing to do this frame
        break;
      case 'waiting':
        this._handleWaiting(currentTime);
        break;
      case 'crossfading':
      case 'finishing':
        // managed by CrossfadeManager / completion callback
        break;
    }
  }

  // ─── State: IDLE ───────────────────────────────────────────────

  private _handleIdle(currentTime: number, duration: number): void {
    // Don't re-attempt crossfade within 60s of a failure (prevents retry loops)
    if (this._lastFailureTime > 0 && Date.now() - this._lastFailureTime < 60000) {
      return;
    }

    // Use a conservative estimate for trigger time before analysis provides precise data.
    // After analysis, _computeCrossfadeParams sets the exact crossfade start time.
    const effectiveDuration = this._getEffectiveCrossfadeDuration(duration);
    const triggerTime = duration - effectiveDuration - PREPARE_AHEAD;

    if (currentTime >= triggerTime && currentTime < duration - 1) {
      this._startAnalysis();
    }
  }

  private _getEffectiveCrossfadeDuration(songDuration: number): number {
    const maxDuration = songDuration / 4;
    return Math.max(
      MIN_CROSSFADE_DURATION,
      Math.min(this._settingsCrossfadeDuration, maxDuration)
    );
  }

  // ─── State: ANALYZING ──────────────────────────────────────────

  /**
   * Kick off analysis as a fire-and-forget promise.
   * Transitions to WAITING on success, back to IDLE on failure.
   */
  private _startAnalysis(): void {
    if (this._analyzingInFlight) return;

    this._state = 'analyzing';
    this._analyzingInFlight = true;
    this._updateStoreState();

    this._doAnalysis()
      .then(() => {
        if (this._state === 'analyzing') {
          this._state = 'waiting';
          this._updateStoreState();
        }
      })
      .catch((err) => {
        if (IS_DEV) {
          console.warn('AutoMixEngine: Analysis failed, falling back to time-based transition', err);
        }
        // Fall back to simple time-based crossfade (no analysis data)
        if (this._state === 'analyzing') {
          this._currentAnalysis = null;
          this._nextAnalysis = null;
          this._computeCrossfadeParams();
          this._state = 'waiting';
          this._updateStoreState();
        }
      })
      .finally(() => {
        this._analyzingInFlight = false;
      });
  }

  /**
   * Perform analysis. This is async but runs off the main thread (Worker).
   * - Uses the GLOBAL AudioContext for decoding (no new contexts).
   * - Single analyzeTrack() call covers volume + energy + BPM + fingerprint.
   * - No double decode.
   */
  private async _doAnalysis(): Promise<void> {
    const music = this._musicStoreRef;
    if (!music) return;

    const playlist = music.persistData.playlists;
    const currentIndex = music.persistData.playSongIndex;

    // Analyze current track if not cached
    const currentSong = playlist[currentIndex];
    if (currentSong && !this._analysisCache.has(currentSong.id)) {
      const blobUrl = this._getSoundBlobUrl(SoundManager.getCurrentSound());
      if (blobUrl) {
        try {
          const analysis = await analyzeTrack(blobUrl, {
            analyzeBPM: this._settingsBpmMatch,
          });
          this._currentAnalysis = { songId: currentSong.id, analysis };
          this._addToCache(this._currentAnalysis);
        } catch (err) {
          if (IS_DEV) console.warn('AutoMixEngine: Current track analysis failed', err);
        }
      }
    } else if (currentSong && this._analysisCache.has(currentSong.id)) {
      this._currentAnalysis = this._analysisCache.get(currentSong.id)!;
    }

    // Check cache for the next track (populated by preAnalyzeTrack from previous plays)
    const nextIndex = music.persistData.playSongMode === 'random'
      ? -1  // can't predict random
      : (currentIndex + 1) % playlist.length;
    if (nextIndex >= 0) {
      const nextSong = playlist[nextIndex];
      if (nextSong && this._analysisCache.has(nextSong.id)) {
        this._nextAnalysis = this._analysisCache.get(nextSong.id)!;
      }
    }

    // Compute crossfade params from whatever analysis we have
    this._computeCrossfadeParams();
  }

  /**
   * Extract blob URL from a sound instance (works with BufferedSound).
   */
  private _getSoundBlobUrl(sound: ISound | null): string | null {
    if (!sound) return null;
    if (sound instanceof BufferedSound) {
      return sound.getBlobUrl();
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

    this._crossfadeDuration = this._getEffectiveCrossfadeDuration(effectiveEnd);

    // ── Tier 1: OutroAnalysis available (multiband classification) ──
    const outro = this._currentAnalysis?.analysis.outro;
    if (outro) {
      this._outroType = outro.outroType;

      // Only trust the analysis-suggested crossfade start when confidence is high.
      // Low-confidence classifications (e.g. default 'hard' fallback at 0.5)
      // should use conservative timing to avoid cutting into active content.
      if (outro.outroConfidence >= 0.7) {
        this._crossfadeStartTime = outro.suggestedCrossfadeStart;
      } else {
        this._crossfadeStartTime = effectiveEnd - this._crossfadeDuration;
      }

      // Adjust crossfade duration by outro type
      const remainingTime = duration - this._crossfadeStartTime;
      switch (outro.outroType) {
        case 'hard':
          // Hard endings maintain energy until the cliff — keep crossfade short
          this._crossfadeDuration = Math.max(
            MIN_CROSSFADE_DURATION,
            Math.min(3, this._getEffectiveCrossfadeDuration(effectiveEnd))
          );
          // For low-confidence, recalculate start to align with the shorter duration
          if (outro.outroConfidence < 0.7) {
            this._crossfadeStartTime = effectiveEnd - this._crossfadeDuration;
          }
          break;
        case 'fadeOut':
          // Longer duration: cover 80% of remaining time
          this._crossfadeDuration = Math.max(
            MIN_CROSSFADE_DURATION,
            Math.min(remainingTime * 0.8, this._getEffectiveCrossfadeDuration(effectiveEnd))
          );
          break;
        case 'reverbTail':
          // Shorter duration: just cover the tail length
          this._crossfadeDuration = Math.max(
            MIN_CROSSFADE_DURATION,
            Math.min(outro.musicalEndOffset, this._getEffectiveCrossfadeDuration(effectiveEnd))
          );
          break;
        case 'slowDown':
          // Cover 70% of remaining deceleration region
          this._crossfadeDuration = Math.max(
            MIN_CROSSFADE_DURATION,
            Math.min(remainingTime * 0.7, this._getEffectiveCrossfadeDuration(effectiveEnd))
          );
          break;
        case 'sustained':
          // Cover musicalEndOffset + 2s
          this._crossfadeDuration = Math.max(
            MIN_CROSSFADE_DURATION,
            Math.min(outro.musicalEndOffset + 2, this._getEffectiveCrossfadeDuration(effectiveEnd))
          );
          break;
        case 'musicalOutro':
          // Cover 60% of remaining time
          this._crossfadeDuration = Math.max(
            MIN_CROSSFADE_DURATION,
            Math.min(remainingTime * 0.6, this._getEffectiveCrossfadeDuration(effectiveEnd))
          );
          break;
        case 'loopFade':
          // Cover 80% of remaining time (like fadeOut)
          this._crossfadeDuration = Math.max(
            MIN_CROSSFADE_DURATION,
            Math.min(remainingTime * 0.8, this._getEffectiveCrossfadeDuration(effectiveEnd))
          );
          break;
        // noiseEnd, silence: use configured duration (already set)
      }
    }
    // ── Tier 2: Only EnergyAnalysis (worker returned no outro) ──
    else if (this._currentAnalysis?.analysis.energy) {
      const energy = this._currentAnalysis.analysis.energy;
      const isFadeOut = energy.isFadeOut;
      this._outroType = isFadeOut ? 'fadeOut' : 'hard';

      const outroOffset = energy.outroStartOffset;

      if (isFadeOut) {
        const outroContentDuration = outroOffset - trailingSilence;
        const fadeInPoint = outroContentDuration * 0.5;
        this._crossfadeStartTime = Math.max(0, duration - trailingSilence - fadeInPoint);
      } else {
        this._crossfadeStartTime = Math.max(0, duration - outroOffset);
      }
    }
    // ── Tier 3: No analysis at all ──
    else {
      this._outroType = null;
      this._crossfadeStartTime = effectiveEnd - this._crossfadeDuration;
    }

    // Beat-align the crossfade start time (skip for types where
    // beat alignment in these regions produces unnatural results)
    const skipBeatAlign = this._outroType === 'fadeOut'
      || this._outroType === 'reverbTail'
      || this._outroType === 'sustained'
      || this._outroType === 'loopFade';
    if (this._settingsBeatAlign && !skipBeatAlign && this._currentAnalysis?.analysis.bpm) {
      const bpmResult = this._currentAnalysis.analysis.bpm;
      this._crossfadeStartTime = findNearestBeat(
        bpmResult.beatGrid,
        this._crossfadeStartTime,
        bpmResult.analysisOffset
      );
    }

    // Adjust duration based on spectral similarity
    if (this._currentAnalysis && this._nextAnalysis) {
      const similarity = spectralSimilarity(
        this._currentAnalysis.analysis.fingerprint,
        this._nextAnalysis.analysis.fingerprint
      );
      const factor = 0.7 + (1 - similarity) * 0.6;
      this._crossfadeDuration *= factor;
      this._crossfadeDuration = Math.max(
        MIN_CROSSFADE_DURATION,
        Math.min(this._crossfadeDuration, this._getEffectiveCrossfadeDuration(effectiveEnd))
      );
    }

    // Ensure crossfade starts before the content ends (not the file end)
    if (this._crossfadeStartTime > effectiveEnd - MIN_CROSSFADE_DURATION) {
      this._crossfadeStartTime = effectiveEnd - this._crossfadeDuration;
    }

    // Never start before 0
    if (this._crossfadeStartTime < 0) {
      this._crossfadeStartTime = 0;
    }

    if (IS_DEV) {
      console.log(
        `AutoMixEngine: Crossfade params — start=${this._crossfadeStartTime.toFixed(1)}s, ` +
        `duration=${this._crossfadeDuration.toFixed(1)}s` +
        (trailingSilence > 0 ? `, trailingSilence=${trailingSilence.toFixed(1)}s` : '') +
        (this._outroType ? `, outroType=${this._outroType}` : '') +
        (outro ? `, confidence=${outro.outroConfidence.toFixed(2)}` : '')
      );
    }
  }

  // ─── Spectral similarity alignment ─────────────────────────────

  /**
   * Refine crossfade start time by finding the point in the outgoing outro
   * that best matches the incoming intro's spectral character.
   *
   * Uses a sliding-window approach:
   *  1. Compute incoming intro's spectral signature (band ratio of first 2s)
   *  2. Scan outgoing outro windows within ±5s of planned start
   *  3. At each candidate, compute spectral similarity + energy compatibility
   *  4. Pick the position with highest combined score
   *
   * Adjusts _crossfadeStartTime if a significantly better match is found.
   */
  private _refineTransitionAlignment(): void {
    const outroData = this._currentAnalysis?.analysis.outro;
    const introData = this._nextAnalysis?.analysis.intro;

    if (!outroData?.multibandEnergy || !introData?.multibandEnergy) return;

    const outroMB = outroData.multibandEnergy;
    const introMB = introData.multibandEnergy;

    // Step 1: Compute incoming intro signature (first 2s = 8 windows at 250ms)
    const introSigWindows = Math.min(8, introMB.low.length);
    if (introSigWindows < 2) return;

    const introSig = this._computeBandRatio(introMB, 0, introSigWindows);

    // Step 2: Map planned crossfade start to outro window index
    // Outro multiband covers last `outroMB.low.length * 0.25` seconds of content
    const outroWindowCount = outroMB.low.length;
    const outroDurationSec = outroWindowCount * 0.25;
    const trailingSilence = this._currentAnalysis!.analysis.energy.trailingSilence;
    const songDuration = this._currentAnalysis!.analysis.duration;

    // outroAnalysisStart = absolute time where outro multiband region begins
    const outroAnalysisStart = songDuration - trailingSilence - outroDurationSec;
    const plannedWindowIdx = Math.round((this._crossfadeStartTime - outroAnalysisStart) / 0.25);

    // Step 3: Scan ±20 windows (±5s) around planned start
    const searchRange = 20;
    const sigWindowCount = introSigWindows;
    let bestScore = -1;
    let bestWindowIdx = plannedWindowIdx;

    for (let w = plannedWindowIdx - searchRange; w <= plannedWindowIdx + searchRange; w++) {
      if (w < 0 || w + sigWindowCount > outroWindowCount) continue;

      const candidateSig = this._computeBandRatio(outroMB, w, w + sigWindowCount);
      const similarity = this._cosineSim3(candidateSig, introSig);

      // Energy compatibility: how close are the energy levels?
      let outroE = 0, introE = 0;
      for (let i = 0; i < sigWindowCount; i++) {
        outroE += outroMB.low[w + i] + outroMB.mid[w + i] + outroMB.high[w + i];
        introE += introMB.low[i] + introMB.mid[i] + introMB.high[i];
      }
      outroE /= sigWindowCount;
      introE /= sigWindowCount;
      const energyRatio = (outroE > 0.0001 && introE > 0.0001)
        ? 1 - Math.min(1, Math.abs(Math.log(outroE / introE)))
        : 0;

      const score = 0.7 * similarity + 0.3 * energyRatio;

      if (score > bestScore) {
        bestScore = score;
        bestWindowIdx = w;
      }
    }

    // Step 4: Apply adjustment if significantly better match found
    const newStartTime = outroAnalysisStart + bestWindowIdx * 0.25;

    // Compute original position's score for comparison
    const originalScore = this._computeMatchScore(
      outroMB, introMB, plannedWindowIdx, introSigWindows
    );

    if (bestScore - originalScore > 0.05 && Math.abs(newStartTime - this._crossfadeStartTime) > 0.25) {
      // Clamp: don't adjust beyond ±5s from original
      const clampedStart = Math.max(
        this._crossfadeStartTime - 5,
        Math.min(this._crossfadeStartTime + 5, newStartTime)
      );

      if (IS_DEV) {
        console.log(
          `AutoMixEngine: Spectral alignment adjusted crossfade start ` +
          `${this._crossfadeStartTime.toFixed(1)}→${clampedStart.toFixed(1)}s ` +
          `(score ${originalScore.toFixed(2)}→${bestScore.toFixed(2)}, ` +
          `shift=${(clampedStart - this._crossfadeStartTime).toFixed(1)}s)`
        );
      }

      this._crossfadeStartTime = clampedStart;
    }
  }

  private _computeBandRatio(
    mb: { low: number[]; mid: number[]; high: number[] },
    start: number,
    end: number
  ): [number, number, number] {
    let sumLow = 0, sumMid = 0, sumHigh = 0;
    for (let i = start; i < end; i++) {
      sumLow += mb.low[i];
      sumMid += mb.mid[i];
      sumHigh += mb.high[i];
    }
    const total = sumLow + sumMid + sumHigh;
    if (total < 1e-10) return [0.33, 0.33, 0.33];
    return [sumLow / total, sumMid / total, sumHigh / total];
  }

  private _cosineSim3(a: [number, number, number], b: [number, number, number]): number {
    let dot = 0, n1 = 0, n2 = 0;
    for (let i = 0; i < 3; i++) {
      dot += a[i] * b[i];
      n1 += a[i] * a[i];
      n2 += b[i] * b[i];
    }
    const d = Math.sqrt(n1) * Math.sqrt(n2);
    return d === 0 ? 0 : dot / d;
  }

  private _computeMatchScore(
    outroMB: { low: number[]; mid: number[]; high: number[] },
    introMB: { low: number[]; mid: number[]; high: number[] },
    windowIdx: number,
    sigLen: number
  ): number {
    if (windowIdx < 0 || windowIdx + sigLen > outroMB.low.length) return 0;
    const candidateSig = this._computeBandRatio(outroMB, windowIdx, windowIdx + sigLen);
    const introSig = this._computeBandRatio(introMB, 0, sigLen);
    const similarity = this._cosineSim3(candidateSig, introSig);

    let outroE = 0, introE = 0;
    for (let i = 0; i < sigLen; i++) {
      outroE += outroMB.low[windowIdx + i] + outroMB.mid[windowIdx + i] + outroMB.high[windowIdx + i];
      introE += introMB.low[i] + introMB.mid[i] + introMB.high[i];
    }
    outroE /= sigLen;
    introE /= sigLen;
    const energyRatio = (outroE > 0.0001 && introE > 0.0001)
      ? 1 - Math.min(1, Math.abs(Math.log(outroE / introE)))
      : 0;

    return 0.7 * similarity + 0.3 * energyRatio;
  }

  // ─── State: WAITING ────────────────────────────────────────────

  private _handleWaiting(currentTime: number): void {
    // Kick off pre-buffering on first entry (once)
    if (!this._preBuffering && !this._preBufferedSound) {
      this._startPreBuffer();
    }
    if (currentTime >= this._crossfadeStartTime) {
      if (this._shouldDeferCrossfade(currentTime)) {
        return;
      }
      this._initiateCrossfade(); // fire-and-forget async
    }
  }

  /**
   * Energy gate: defer crossfade start if the outgoing track's energy
   * is NOT declining. Applies broadly to ALL outro types, since any
   * classification can misjudge when musical content truly winds down.
   *
   * Exceptions: fadeOut, silence, reverbTail, loopFade — these types have
   * reliable detection (the audio IS already fading/silent/reverbing).
   *
   * Maximum deferral: half the planned crossfade duration, capped at 5s.
   */
  private _shouldDeferCrossfade(currentTime: number): boolean {
    // Skip the gate for types where the audio IS already declining
    if (this._outroType === 'fadeOut'
      || this._outroType === 'silence'
      || this._outroType === 'reverbTail'
      || this._outroType === 'loopFade') {
      return false;
    }

    const energy = this._currentAnalysis?.analysis.energy;
    if (!energy) return false;

    // Cap deferral: never defer past the point where a meaningful crossfade
    // can still happen. Without this, maxDefer can exceed the remaining song
    // time when crossfadeStartTime is close to the song end (e.g. Tier 2 hard
    // with small outroStartOffset), causing the gate to never release.
    const maxDefer = Math.min(this._crossfadeDuration * 0.5, 5);
    const sound = SoundManager.getCurrentSound();
    const songDuration = sound?.duration() ?? 0;
    const maxDeferByRemaining = Math.max(0, songDuration - this._crossfadeStartTime - MIN_CROSSFADE_DURATION);
    if (currentTime >= this._crossfadeStartTime + Math.min(maxDefer, maxDeferByRemaining)) return false;

    const sec = Math.floor(currentTime);
    if (sec < 3 || sec >= energy.energyPerSecond.length) return false;

    // Check: is energy over the last 3 seconds still high and not declining?
    const e3sAgo = energy.energyPerSecond[sec - 3];
    const e1sAgo = energy.energyPerSecond[sec - 1];
    const eNow = energy.energyPerSecond[sec];

    // Gate condition 1: current energy is still above 50% of song average
    if (eNow < energy.averageEnergy * 0.5) return false;

    // Gate condition 2: energy hasn't declined by 25%+ over the last 3 seconds
    if (e3sAgo > 0.05 && eNow / e3sAgo < 0.75) return false;

    // Gate condition 3: energy isn't in a clear downward trend
    if (e3sAgo > e1sAgo && e1sAgo > eNow && eNow / e3sAgo < 0.85) return false;

    // Energy is still high and not declining → defer
    if (IS_DEV) {
      console.log(
        `AutoMixEngine: Energy gate deferred crossfade ` +
        `(e=${eNow.toFixed(2)}, avg=${energy.averageEnergy.toFixed(2)}, ` +
        `e3sAgo=${e3sAgo.toFixed(2)}, maxDefer=${Math.min(maxDefer, maxDeferByRemaining).toFixed(1)}s)`
      );
    }
    return true;
  }

  /**
   * Pre-buffer the next track during WAITING state.
   * Fire-and-forget: fetches URL, downloads audio, optionally analyzes,
   * and initializes audio graph so crossfade can begin instantly.
   */
  private _startPreBuffer(): void {
    const music = this._musicStoreRef;
    if (!music) return;

    const playlist = music.persistData.playlists;
    const currentIndex = music.persistData.playSongIndex;
    const listLength = playlist.length;

    // Determine next song index (same logic as _doCrossfade)
    let nextIndex: number;
    if (music.persistData.playSongMode === 'random') {
      nextIndex = Math.floor(Math.random() * listLength);
    } else {
      nextIndex = (currentIndex + 1) % listLength;
    }

    const nextSong = playlist[nextIndex];
    if (!nextSong) return;

    this._preBuffering = true;

    const doPreBuffer = async () => {
      // Step 1: Fetch music URL
      const { getMusicUrl } = await import('@/api/song');
      const res = await getMusicUrl(nextSong.id);
      if (!res?.data?.[0]?.url) throw new Error('Failed to get music URL');

      // Bail if state changed
      if (this._state !== 'waiting') return;

      const url = res.data[0].url.replace(/^http:/, 'https:');

      // Step 2: Create BufferedSound (starts silent, begins download)
      const sound = new BufferedSound({
        src: [url],
        preload: true,
        volume: 0,
      });

      // Step 3: Wait for load
      await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => reject(new Error('Pre-buffer load timeout')), 30000);
        sound.once('load', () => { clearTimeout(timeout); resolve(); });
        sound.once('loaderror', () => { clearTimeout(timeout); reject(new Error('Pre-buffer load error')); });
      });

      // Bail if state changed during load
      if (this._state !== 'waiting') {
        sound.stop();
        sound.unload();
        return;
      }

      // Step 4: Analyze incoming track for volume normalization (if enabled + not cached)
      let preBufferedAnalysis: CachedAnalysis | null = null;
      if (this._settingsVolumeNorm) {
        const cached = this._analysisCache.get(nextSong.id);
        if (cached) {
          preBufferedAnalysis = cached;
        } else {
          try {
            const blobUrl = sound.getBlobUrl();
            if (blobUrl) {
              const analysis = await analyzeTrack(blobUrl, { analyzeBPM: this._settingsBpmMatch });
              preBufferedAnalysis = { songId: nextSong.id, analysis };
              this._addToCache(preBufferedAnalysis);
            }
          } catch (err) {
            if (IS_DEV) console.warn('AutoMixEngine: Pre-buffer analysis failed', err);
          }
        }
      }

      // Bail if state changed during analysis
      if (this._state !== 'waiting') {
        sound.stop();
        sound.unload();
        return;
      }

      // Step 5: Initialize audio graph so GainNode is ready for crossfade
      await sound.ensureAudioGraph();

      // Final bail check
      if (this._state !== 'waiting') {
        sound.stop();
        sound.unload();
        return;
      }

      // Store pre-buffered state
      this._preBufferedSound = sound;
      this._preBufferedSongIndex = nextIndex;
      this._preBufferedAnalysis = preBufferedAnalysis;

      // Refine crossfade alignment with the now-available incoming analysis
      if (preBufferedAnalysis) {
        this._nextAnalysis = preBufferedAnalysis;
        this._refineTransitionAlignment();
      }

      if (IS_DEV) {
        console.log(`AutoMixEngine: Pre-buffered next track "${nextSong.name}" (index=${nextIndex})`);
      }
    };

    doPreBuffer().catch((err) => {
      if (IS_DEV) {
        console.warn('AutoMixEngine: Pre-buffer failed, will use slow path', err);
      }
    }).finally(() => {
      this._preBuffering = false;
    });
  }

  // ─── State: CROSSFADING ────────────────────────────────────────

  /**
   * Fire-and-forget: start the actual crossfade transition.
   */
  private _initiateCrossfade(): void {
    this._state = 'crossfading';
    this._updateStoreState();

    this._doCrossfade().catch((err) => {
      console.error('AutoMixEngine: Crossfade failed, falling back to normal transition', err);

      // Record failure time to prevent immediate retry loops
      this._lastFailureTime = Date.now();

      // Snapshot the current sound BEFORE cancelCrossfade, because cancel may
      // manipulate SoundManager state.
      const currentSound = SoundManager.getCurrentSound();
      const songAlreadyEnded = currentSound && !currentSound.playing();

      // Use cancelCrossfade for uniform cleanup (handles all state combinations)
      this.cancelCrossfade();

      // If the song ended while we were waiting (e.g., 30s load timeout),
      // the 'end' event was suppressed because isCrossfading() was true.
      // Now we need to trigger the normal next-song transition.
      if (songAlreadyEnded && this._musicStoreRef) {
        if (IS_DEV) {
          console.log('AutoMixEngine: Song ended during failed crossfade, triggering normal transition');
        }
        this._musicStoreRef.setPlaySongIndex('next');
      }
    });
  }

  private async _doCrossfade(): Promise<void> {
    const music = this._musicStoreRef;
    if (!music) throw new Error('No music store');

    const playlist = music.persistData.playlists;
    const currentIndex = music.persistData.playSongIndex;
    const listLength = playlist.length;

    // Determine next song index
    let nextIndex: number;
    if (music.persistData.playSongMode === 'random') {
      nextIndex = Math.floor(Math.random() * listLength);
    } else {
      nextIndex = (currentIndex + 1) % listLength;
    }

    const nextSong = playlist[nextIndex];
    if (!nextSong) throw new Error('No next song');

    let incomingSound: BufferedSound;

    // ★ Fast path: use pre-buffered sound (instant crossfade)
    if (this._preBufferedSound && this._preBufferedSongIndex === nextIndex) {
      incomingSound = this._preBufferedSound;
      this._nextAnalysis = this._preBufferedAnalysis;
      // Consume pre-buffer state
      this._preBufferedSound = null;
      this._preBufferedSongIndex = -1;
      this._preBufferedAnalysis = null;

      if (IS_DEV) {
        console.log(`AutoMixEngine: Using pre-buffered sound for "${nextSong.name}"`);
      }
    } else {
      // Slow path: fallback — fetch, download, analyze (same as before)
      // Clean up stale pre-buffer if any
      this._cleanupPreBuffer();

      // Fetch music URL for next track
      const { getMusicUrl } = await import('@/api/song');
      const res = await getMusicUrl(nextSong.id);
      if (!res?.data?.[0]?.url) throw new Error('Failed to get music URL');

      // Bail if cancelled during URL fetch
      if (this._state !== 'crossfading') return;

      const url = res.data[0].url.replace(/^http:/, 'https:');

      // Create incoming BufferedSound (starts silent)
      incomingSound = new BufferedSound({
        src: [url],
        preload: true,
        volume: 0,
      });

      // Wait for load
      await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => reject(new Error('Load timeout')), 30000);
        incomingSound.once('load', () => { clearTimeout(timeout); resolve(); });
        incomingSound.once('loaderror', () => { clearTimeout(timeout); reject(new Error('Load error')); });
      });

      // Bail if cancelled during load
      if (this._state !== 'crossfading') return;

      // Analyze incoming track for volume normalization
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
            if (IS_DEV) console.warn('AutoMixEngine: Incoming track analysis failed', err);
          }
        }
      }

      // Bail if cancelled during analysis
      if (this._state !== 'crossfading') return;

      // Refine crossfade alignment with the now-available incoming analysis
      this._refineTransitionAlignment();

      // Initialize audio graph (pre-buffered path already did this)
      await incomingSound.ensureAudioGraph();

      // Bail if cancelled during audio graph init
      if (this._state !== 'crossfading') return;
    }

    this._incomingSound = incomingSound;

    const outgoingSound = SoundManager.getCurrentSound();
    if (!outgoingSound) throw new Error('No outgoing sound');

    // Move current → outgoing, incoming → current
    SoundManager.beginTransition(incomingSound);

    // Start playback
    incomingSound.play();

    // Loudness compensation: relative gain ratio between tracks.
    // Applied as a mid-crossfade envelope (not endpoint targets) to avoid
    // discontinuities at crossfade boundaries.
    let loudnessCompensation = 1;
    if (this._settingsVolumeNorm) {
      const outAdj = this._currentAnalysis?.analysis.volume.gainAdjustment ?? 1;
      const inAdj = this._nextAnalysis?.analysis.volume.gainAdjustment ?? 1;
      loudnessCompensation = Math.max(0.5, Math.min(2.0, inAdj / outAdj));
    }

    const volume = music.persistData.playVolume;

    // Get GainNodes for Web Audio API crossfade
    const outgoingGain = this._getGainNode(outgoingSound);
    const incomingGain = this._getGainNode(incomingSound);

    // Adjust crossfade duration based on incoming track's intro analysis
    let adjustedDuration = this._crossfadeDuration;
    const incomingIntro = this._nextAnalysis?.analysis.intro;
    if (incomingIntro) {
      if (incomingIntro.quietIntroDuration > adjustedDuration) {
        // Long quiet intro: safe to extend crossfade for smoother transition
        // (incoming content is quiet, so overlap with outgoing is harmless)
        adjustedDuration = Math.min(
          incomingIntro.quietIntroDuration,
          this._settingsCrossfadeDuration
        );
      }
      // Note: we do NOT shorten the crossfade for loud intros. The crossfade
      // curve already handles volume balance (at 25% progress, incoming gain
      // is only 0.38 with equal-power). Shortening aggressively would make
      // most transitions feel abrupt since most songs reach 80% energy quickly.

      if (IS_DEV) {
        console.log(
          `AutoMixEngine: Incoming intro — ` +
          `quiet=${incomingIntro.quietIntroDuration.toFixed(1)}s, ` +
          `build=${incomingIntro.energyBuildDuration.toFixed(1)}s, ` +
          `ratio=${incomingIntro.introEnergyRatio.toFixed(2)}, ` +
          `duration=${this._crossfadeDuration.toFixed(1)}→${adjustedDuration.toFixed(1)}s`
        );
      }
    }

    if (outgoingGain && incomingGain) {
      // Determine curve and fadeInOnly — optionally override per outro type
      let effectiveCurve: CrossfadeCurve = this._settingsCurve;
      let effectiveFadeInOnly = this._outroType === 'fadeOut' || this._outroType === 'loopFade';
      let effectiveInShape = 1;
      let effectiveOutShape = 1;

      const outroConfidence = this._currentAnalysis?.analysis.outro?.outroConfidence ?? 0;
      if (this._settingsSmartCurve && this._outroType && outroConfidence >= 0.7) {
        const profile = getOutroTypeCrossfadeProfile(this._outroType);
        effectiveCurve = profile.curve;
        effectiveFadeInOnly = profile.fadeInOnly;
        effectiveInShape = profile.inShape ?? 1;
        effectiveOutShape = profile.outShape ?? 1;
      }

      // Adjust incoming ramp shape based on intro character
      // Quiet intros: faster ramp is safe (content is quiet anyway)
      // Loud intros: slower ramp to minimize perceived overlap
      if (incomingIntro) {
        if (incomingIntro.introEnergyRatio < 0.3) {
          effectiveInShape = Math.max(0.5, effectiveInShape - 0.3);
        } else if (incomingIntro.introEnergyRatio > 0.8) {
          effectiveInShape = Math.min(2.0, effectiveInShape + 0.3);
        }
      }

      const params: CrossfadeParams = {
        duration: adjustedDuration,
        curve: effectiveCurve,
        incomingGain: volume,
        outgoingGain: volume,
        fadeInOnly: effectiveFadeInOnly,
        outroType: this._outroType ?? undefined,
        inShape: effectiveInShape,
        outShape: effectiveOutShape,
        loudnessCompensation,
      };

      this._crossfadeManager.scheduleFullCrossfade(
        outgoingGain,
        incomingGain,
        params,
        () => this._onCrossfadeComplete()
      );
    } else {
      // Fallback: software fade
      if (this._outroType !== 'fadeOut' && this._outroType !== 'loopFade') {
        outgoingSound.fade(volume, 0, adjustedDuration * 1000);
      }
      incomingSound.fade(0, volume, adjustedDuration * 1000);
      this._softwareFadeStartedAt = Date.now();
      this._softwareFadeRemaining = adjustedDuration * 1000;
      this._softwareFadeTimerId = setTimeout(() => {
        this._softwareFadeTimerId = null;
        this._onCrossfadeComplete();
      }, this._softwareFadeRemaining);
    }

    // Wait for playback to actually start before updating store/UI.
    // _audio.play() is async — if we update the store now, the song data watcher
    // resets duration to 0, and the time tracking loop can't update it because
    // playing() returns false until the browser's play promise resolves.
    // On failure (autoplay blocked, AbortError), retry once as a recovery attempt.
    await this._waitForPlayStart(incomingSound);

    // Bail if cancelled during play wait
    if (this._state !== 'crossfading') return;

    // If paused during setup, wait for resume before updating store.
    // Without this, the async flow continues and changes playSongIndex while paused.
    if (this._isPaused) {
      await this._waitForUnpause();
      if (this._state !== 'crossfading') return;
    }

    // Update store state (triggers song data watcher which fetches lyrics)
    music.persistData.playSongIndex = nextIndex;
    if (typeof music.resetSongLyricState === 'function') {
      music.resetSongLyricState();
    }

    // Update global player reference
    window.$player = incomingSound;

    // Adopt incoming sound: register event handlers, start time/spectrum tracking
    const { adoptIncomingSound } = await import('./PlayerFunctions');
    adoptIncomingSound(incomingSound);

    if (IS_DEV) {
      console.log(`AutoMixEngine: Crossfade started → "${nextSong.name}"`);
    }
  }

  /**
   * Wait for the incoming sound to start playing.
   * Retries once if play doesn't start within 2 seconds.
   * Returns without blocking indefinitely (3s max).
   */
  private _waitForPlayStart(sound: ISound): Promise<void> {
    return new Promise((resolve) => {
      // Already playing (fast path)
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

      // Listen for the play event
      sound.once('play', done);

      // After 2s, retry once if not playing (but not if crossfade is paused)
      const retryTimer = setTimeout(() => {
        if (!resolved && !sound.playing() && !this._isPaused) {
          if (IS_DEV) {
            console.warn('AutoMixEngine: Play not started after 2s, retrying');
          }
          sound.play();
        }
      }, 2000);

      // Hard deadline: proceed after 3s regardless
      const deadline = setTimeout(() => {
        clearTimeout(retryTimer);
        if (!resolved) {
          if (IS_DEV) {
            console.warn('AutoMixEngine: Play confirmation timeout, proceeding anyway');
          }
          done();
        }
      }, 3000);

      // If play event fires, clear timers
      const origDone = done;
      const cleanDone = () => {
        clearTimeout(retryTimer);
        clearTimeout(deadline);
        origDone();
      };
      // Replace the once listener
      sound.off('play', done);
      sound.once('play', cleanDone);
    });
  }

  /**
   * Wait for _isPaused to become false. Resolved by resumeCrossfade()
   * or cancelCrossfade(). Used to block the _doCrossfade async flow
   * at safe points so it doesn't update the store while paused.
   */
  private _unpauseResolve: (() => void) | null = null;

  private _waitForUnpause(): Promise<void> {
    if (!this._isPaused) return Promise.resolve();
    return new Promise((resolve) => {
      this._unpauseResolve = resolve;
    });
  }

  /**
   * Extract GainNode from a sound instance.
   */
  private _getGainNode(sound: ISound): GainNode | null {
    if (sound instanceof BufferedSound) {
      const inner = sound.getInnerSound();
      if (inner) {
        return inner.getGainNode();
      }
    }
    // Direct NativeSound
    if ('getGainNode' in sound && typeof (sound as any).getGainNode === 'function') {
      return (sound as any).getGainNode();
    }
    return null;
  }

  // ─── State: FINISHING ──────────────────────────────────────────

  private _onCrossfadeComplete(): void {
    // Clear software fade timeout (in case GainNode path completed first)
    if (this._softwareFadeTimerId !== null) {
      clearTimeout(this._softwareFadeTimerId);
      this._softwareFadeTimerId = null;
    }
    this._softwareFadeRemaining = 0;
    this._isPaused = false;

    this._state = 'finishing';
    this._updateStoreState();

    // Unload outgoing
    const outgoing = SoundManager.getOutgoingSound();
    if (outgoing) {
      outgoing.stop();
      outgoing.unload();
    }
    SoundManager.unloadOutgoing();

    // Sync the incoming sound's internal _volume field to the user's playVolume.
    // During crossfade, CrossfadeManager manipulated the GainNode directly,
    // but NativeSound._volume was never updated (still 0 from creation).
    // Without this, sound.volume() getter returns 0 and any subsequent
    // volume interaction (setVolume, fade) would use the wrong base value.
    const currentSound = SoundManager.getCurrentSound();
    if (currentSound && this._musicStoreRef) {
      const userVolume = this._musicStoreRef.persistData.playVolume;
      currentSound.volume(userVolume);
    }

    // Rotate analysis cache
    this._currentAnalysis = this._nextAnalysis;
    this._nextAnalysis = null;
    this._incomingSound = null;
    this._evictCache();

    this._state = 'idle';
    this._updateStoreState();

    if (IS_DEV) {
      console.log('AutoMixEngine: Crossfade complete, back to idle');
    }
  }

  // ─── Cancel ────────────────────────────────────────────────────

  /**
   * Cleanup pre-buffered sound state. Safe to call at any time.
   */
  private _cleanupPreBuffer(): void {
    if (this._preBufferedSound) {
      this._preBufferedSound.stop();
      this._preBufferedSound.unload();
      this._preBufferedSound = null;
    }
    this._preBufferedSongIndex = -1;
    this._preBufferedAnalysis = null;
    this._preBuffering = false;
  }

  cancelCrossfade(): void {
    if (this._state === 'idle') return;

    if (IS_DEV) {
      console.log('AutoMixEngine: Cancelling crossfade');
    }

    this._crossfadeManager.cancel();

    // Clear software fade timeout
    if (this._softwareFadeTimerId !== null) {
      clearTimeout(this._softwareFadeTimerId);
      this._softwareFadeTimerId = null;
    }
    this._softwareFadeRemaining = 0;

    // Cleanup pre-buffer state
    this._cleanupPreBuffer();

    // Cleanup incoming sound properly
    if (this._incomingSound) {
      const incoming = this._incomingSound;

      // If SoundManager already transitioned (incoming is current), revert
      if (SoundManager.getCurrentSound() === incoming) {
        const outgoing = SoundManager.getOutgoingSound();
        if (outgoing) {
          // Restore outgoing as the active sound
          SoundManager.revertTransition();
          // Restore outgoing volume (it was being faded out by CrossfadeManager)
          if (this._musicStoreRef) {
            outgoing.volume(this._musicStoreRef.persistData.playVolume);
          }
          window.$player = outgoing;
        }
      } else {
        // Incoming not yet in SoundManager — just unload it
        incoming.stop();
        incoming.unload();
      }

      this._incomingSound = null;
    }

    this._analyzingInFlight = false;
    this._nextAnalysis = null;
    this._isPaused = false;
    this._state = 'idle';
    this._updateStoreState();

    // Resolve pending unpause promise (unblocks _doCrossfade if awaiting)
    if (this._unpauseResolve) {
      this._unpauseResolve();
      this._unpauseResolve = null;
    }
  }

  /**
   * Pause during crossfade. Two cases:
   *
   * 1. CrossfadeManager active (audible crossfade running):
   *    Freeze gain scheduling, cancel software fade timeout, pause both sounds.
   *    Returns true — caller should NOT fall through to normal fadePlayOrPause.
   *
   * 2. Still in async setup (not yet audible):
   *    Cancel the crossfade entirely. The transition hasn't audibly started,
   *    so cancellation is invisible to the user.
   *    Returns false — caller SHOULD fall through to normal fadePlayOrPause.
   */
  pauseCrossfade(): boolean {
    if (this._state !== 'crossfading' || this._isPaused) return false;

    if (this._crossfadeManager.isActive()) {
      // ── Audible crossfade: freeze it ──
      this._isPaused = true;
      this._crossfadeManager.pauseCrossfade();

      // Cancel software fade timeout, record remaining time for resume
      if (this._softwareFadeTimerId !== null) {
        clearTimeout(this._softwareFadeTimerId);
        this._softwareFadeTimerId = null;
        const elapsed = Date.now() - this._softwareFadeStartedAt;
        this._softwareFadeRemaining = Math.max(0, this._softwareFadeRemaining - elapsed);
      }

      // Pause both sounds
      SoundManager.getCurrentSound()?.pause();
      SoundManager.getOutgoingSound()?.pause();

      if (IS_DEV) {
        console.log('AutoMixEngine: Crossfade paused (frozen)');
      }
      return true;
    } else {
      // ── Async setup phase: cancel entirely ──
      // cancelCrossfade reverts SoundManager, resets state to idle.
      // The caller should fall through to normal pause logic.
      if (IS_DEV) {
        console.log('AutoMixEngine: Crossfade paused during setup — cancelling');
      }
      this.cancelCrossfade();
      return false;
    }
  }

  /**
   * Resume a paused crossfade: resume both sounds and the CrossfadeManager.
   * Only meaningful after pauseCrossfade() returned true (frozen crossfade).
   */
  resumeCrossfade(): void {
    if (this._state !== 'crossfading' || !this._isPaused) return;
    this._isPaused = false;

    // Resume sounds (outgoing first — if it ended naturally during pause,
    // play() is a no-op on an ended HTMLAudioElement)
    SoundManager.getOutgoingSound()?.play();
    SoundManager.getCurrentSound()?.play();

    // Resume CrossfadeManager (shifts _startTime to compensate for pause gap)
    this._crossfadeManager.resumeCrossfade();

    // Re-schedule software fade timeout if we were using the fallback path
    if (this._softwareFadeRemaining > 0 && !this._crossfadeManager.isActive()) {
      this._softwareFadeStartedAt = Date.now();
      this._softwareFadeTimerId = setTimeout(() => {
        this._softwareFadeTimerId = null;
        this._onCrossfadeComplete();
      }, this._softwareFadeRemaining);
    }

    // Resolve pending unpause promise (unblocks _doCrossfade if awaiting)
    if (this._unpauseResolve) {
      this._unpauseResolve();
      this._unpauseResolve = null;
    }

    if (IS_DEV) {
      console.log('AutoMixEngine: Crossfade resumed');
    }
  }

  // ─── Track lifecycle hooks ─────────────────────────────────────

  /**
   * Called when a new track starts (from PlayerFunctions.createSound).
   * Resets state and optionally pre-analyzes.
   */
  onTrackStarted(sound: ISound, songId: number): void {
    // If crossfade flow handled this, don't interfere
    if (this._state === 'crossfading' || this._state === 'finishing') return;

    // Clear stale pre-buffer from previous track's WAITING state
    this._cleanupPreBuffer();

    this._state = 'idle';
    this._lastFailureTime = 0; // Reset cooldown for new track
    this._updateStoreState();

    // Pre-analyze current track in background (Worker — non-blocking)
    if (this._enabled && sound instanceof BufferedSound) {
      this._preAnalyzeTrack(sound, songId);
    }
  }

  /**
   * Pre-analyze a track in the background. Fire-and-forget.
   */
  private _preAnalyzeTrack(sound: BufferedSound, songId: number): void {
    if (this._analysisCache.has(songId)) return;

    const doAnalysis = async () => {
      // Wait for the blob URL to be available
      let blobUrl = sound.getBlobUrl();

      if (!blobUrl) {
        await new Promise<void>((resolve) => {
          sound.once('load', resolve);
          setTimeout(resolve, 30000);
        });
        blobUrl = sound.getBlobUrl();
      }

      if (!blobUrl) return;

      const analysis = await analyzeTrack(blobUrl, {
        analyzeBPM: this._settingsBpmMatch,
      });

      this._addToCache({ songId, analysis });

      if (IS_DEV) {
        console.log(`AutoMixEngine: Pre-analyzed track ${songId}`);
      }
    };

    doAnalysis().catch((err) => {
      if (IS_DEV) {
        console.warn(`AutoMixEngine: Pre-analysis failed for ${songId}`, err);
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
    const progress = (this._state === 'crossfading' || this._state === 'finishing')
      ? this._crossfadeManager.getProgress()
      : -1;

    // Determine incoming song info
    let incomingSongName: string | null = null;
    let incomingSongId: number | null = null;
    if (this._state === 'crossfading' || this._state === 'waiting') {
      const playlist = this._musicStoreRef.persistData?.playlists;
      const currentIndex = this._musicStoreRef.persistData?.playSongIndex;
      if (playlist && currentIndex != null) {
        const nextIndex = this._musicStoreRef.persistData.playSongMode === 'random'
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
    this._analysisCache.clear();
  }
}

// ─── Singleton ─────────────────────────────────────────────────────

let _instance: AutoMixEngine | null = null;

export function getAutoMixEngine(): AutoMixEngine {
  if (!_instance) {
    _instance = new AutoMixEngine();
  }
  return _instance;
}
