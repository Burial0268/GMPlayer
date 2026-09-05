/**
 * PreBufferManager — Pre-buffering logic for seamless crossfade transitions
 *
 * Extracted from AutoMixEngine._startPreBuffer().
 * Fetches, downloads, and optionally analyzes the next track during WAITING
 * state so the crossfade can begin instantly when triggered.
 */

import { BufferedSound } from "../BufferedSound";
import { analyzeTrack } from "./TrackAnalyzer";
import { resolveSongUrl } from "../resolveSongUrl";
import type { CachedAnalysis } from "./types";

const IS_DEV = import.meta.env?.DEV ?? false;

interface PreBufferState {
  sound: BufferedSound;
  songIndex: number;
  songId: number | string | null;
  sourceUrl: string;
  analysis: CachedAnalysis | null;
}

export class PreBufferManager {
  private _preBuffered: PreBufferState | null = null;
  private _isBuffering: boolean = false;
  /** Bumped by cleanup() and each new pre-buffer chain. An in-flight
   * doPreBuffer() whose epoch is stale must release its sound instead of
   * publishing it — otherwise a cancel during download orphans a fully
   * buffered song (Blob URL + ArrayBuffer never revoked). */
  private _epoch: number = 0;

  /**
   * Start pre-buffering the next track. Fire-and-forget.
   * @param musicStore - Music store reference
   * @param analysisCache - Analysis cache map (read-only here; writes go
   * through `addToCache` so the owner's size cap always applies)
   * @param settings - Current settings
   * @param stateGetter - Function to check current AutoMix state (for bail-out)
   * @param addToCache - Owner's capped cache-insert path
   */
  startPreBuffer(
    musicStore: any,
    analysisCache: Map<number, CachedAnalysis>,
    settings: { volumeNorm: boolean; bpmMatch: boolean },
    stateGetter: () => string,
    addToCache: (entry: CachedAnalysis) => void,
  ): void {
    if (this._isBuffering || this._preBuffered) return;

    const playlist = musicStore.persistData.playlists;
    const currentIndex = musicStore.persistData.playSongIndex;
    const listLength = playlist.length;

    // Next song index (same rule as _doCrossfade / the store's own advance):
    // positional in every mode, because random mode shuffles the queue itself.
    const nextIndex = listLength > 0 ? (currentIndex + 1) % listLength : currentIndex;

    const nextSong = playlist[nextIndex];
    if (!nextSong) return;

    this._isBuffering = true;
    const epoch = ++this._epoch;

    const canContinue = (): boolean => {
      if (epoch !== this._epoch) return false;
      const state = stateGetter();
      return (
        (state === "idle" || state === "analyzing" || state === "waiting") &&
        musicStore.persistData.playSongIndex === currentIndex
      );
    };

    const doPreBuffer = async () => {
      // Step 1: Resolve music URL (unified: NCM + trial detection + UNM fallback)
      const result = await resolveSongUrl(nextSong);
      if (!result) {
        throw new Error("Failed to resolve music URL");
      }
      const url = result.url;

      // Bail if state changed
      if (!canContinue()) return;

      // Step 2: Create BufferedSound (starts silent, begins download)
      const sound = new BufferedSound({
        src: [url],
        preload: true,
        volume: 0,
      });

      // Step 3: Wait for load. On timeout/loaderror the sound keeps its
      // download buffer alive until unloaded — release it before rethrowing,
      // or every failed pre-buffer leaks a full song (~5-15MB).
      try {
        await new Promise<void>((resolve, reject) => {
          const timeout = setTimeout(() => reject(new Error("Pre-buffer load timeout")), 30000);
          sound.once("load", () => {
            clearTimeout(timeout);
            resolve();
          });
          sound.once("loaderror", () => {
            clearTimeout(timeout);
            reject(new Error("Pre-buffer load error"));
          });
        });
      } catch (err) {
        sound.stop();
        sound.unload();
        throw err;
      }

      // Bail if state changed during load
      if (!canContinue()) {
        sound.stop();
        sound.unload();
        return;
      }

      // Step 4: Analyze incoming track for volume normalization (if enabled + not cached)
      let preBufferedAnalysis: CachedAnalysis | null = null;
      if (settings.volumeNorm) {
        const cached = analysisCache.get(nextSong.id);
        if (cached) {
          preBufferedAnalysis = cached;
        } else {
          try {
            const blobUrl = sound.getBlobUrl();
            if (blobUrl) {
              const analysis = await analyzeTrack(blobUrl, { analyzeBPM: settings.bpmMatch });
              preBufferedAnalysis = { songId: nextSong.id, analysis };
              // Share with the state machine's cache so a discarded pre-buffer
              // doesn't force a full re-decode of the same track later. Must
              // go through the owner's capped insert path: a direct Map.set
              // would bypass eviction, and repeated skip/cancel churn (no
              // crossfade ever completing) would grow the cache unbounded.
              addToCache(preBufferedAnalysis);
            }
          } catch (err) {
            if (IS_DEV) console.warn("PreBufferManager: Analysis failed", err);
          }
        }
      }

      // Bail if state changed during analysis
      if (!canContinue()) {
        sound.stop();
        sound.unload();
        return;
      }

      // Step 5: Initialize audio graph so GainNode is ready for crossfade
      try {
        await sound.ensureAudioGraph();
      } catch (err) {
        sound.stop();
        sound.unload();
        throw err;
      }

      // Final bail check
      if (!canContinue()) {
        sound.stop();
        sound.unload();
        return;
      }

      // Store pre-buffered state. Defensive: never orphan an existing buffer —
      // with the epoch guard this should be unreachable, but an overwritten
      // BufferedSound would leak its whole download.
      if (this._preBuffered && this._preBuffered.sound !== sound) {
        this._preBuffered.sound.stop();
        this._preBuffered.sound.unload();
        this._preBuffered = null;
      }
      this._preBuffered = {
        sound,
        songIndex: nextIndex,
        songId: nextSong.id ?? null,
        sourceUrl: url,
        analysis: preBufferedAnalysis,
      };

      if (IS_DEV) {
        console.log(
          `PreBufferManager: Pre-buffered next track "${nextSong.name}" (index=${nextIndex})`,
        );
      }
    };

    doPreBuffer()
      .catch((err) => {
        if (IS_DEV) {
          console.warn("PreBufferManager: Pre-buffer failed, will use slow path", err);
        }
      })
      .finally(() => {
        // Only the live chain may clear the flag — a stale chain clearing it
        // would re-open the startPreBuffer guard while a newer chain runs.
        if (epoch === this._epoch) {
          this._isBuffering = false;
        }
      });
  }

  /**
   * Consume the pre-buffered sound (transfers ownership to caller).
   * Returns null if no pre-buffered sound available or index doesn't match.
   */
  consume(expectedIndex: number, expectedSongId?: number | string | null): PreBufferState | null {
    if (!this._preBuffered) return null;
    const expectedId = expectedSongId ?? null;
    const sameSong =
      expectedId === null ||
      this._preBuffered.songId === null ||
      String(this._preBuffered.songId) === String(expectedId);

    if (this._preBuffered.songIndex !== expectedIndex || !sameSong) {
      // Wrong track — clean up and return null
      this.cleanup();
      return null;
    }

    const result = this._preBuffered;
    this._preBuffered = null;
    return result;
  }

  /**
   * Clean up pre-buffered state. Safe to call at any time.
   */
  cleanup(): void {
    // Invalidate any in-flight doPreBuffer() chain: its next canContinue()
    // check fails and it releases its own sound.
    this._epoch++;
    if (this._preBuffered) {
      this._preBuffered.sound.stop();
      this._preBuffered.sound.unload();
      this._preBuffered = null;
    }
    this._isBuffering = false;
  }

  get isBuffering(): boolean {
    return this._isBuffering;
  }

  get hasBuffer(): boolean {
    return this._preBuffered !== null;
  }

  /** Get the analysis from the pre-buffered track (for _finalizeCrossfadeParams) */
  get preBufferedAnalysis(): CachedAnalysis | null {
    return this._preBuffered?.analysis ?? null;
  }
}
