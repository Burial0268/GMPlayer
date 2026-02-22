/**
 * PreBufferManager — Pre-buffering logic for seamless crossfade transitions
 *
 * Extracted from AutoMixEngine._startPreBuffer().
 * Fetches, downloads, and optionally analyzes the next track during WAITING
 * state so the crossfade can begin instantly when triggered.
 */

import { BufferedSound } from '../BufferedSound';
import { analyzeTrack, type TrackAnalysis } from './TrackAnalyzer';
import { getMusicUrl } from '@/api/song';
import type { CachedAnalysis } from './types';

const IS_DEV = import.meta.env?.DEV ?? false;

interface PreBufferState {
  sound: BufferedSound;
  songIndex: number;
  analysis: CachedAnalysis | null;
}

export class PreBufferManager {
  private _preBuffered: PreBufferState | null = null;
  private _isBuffering: boolean = false;

  /**
   * Start pre-buffering the next track. Fire-and-forget.
   * @param musicStore - Music store reference
   * @param analysisCache - Analysis cache map
   * @param settings - Current settings
   * @param stateGetter - Function to check current AutoMix state (for bail-out)
   */
  startPreBuffer(
    musicStore: any,
    analysisCache: Map<number, CachedAnalysis>,
    settings: { volumeNorm: boolean; bpmMatch: boolean },
    stateGetter: () => string
  ): void {
    if (this._isBuffering || this._preBuffered) return;

    const playlist = musicStore.persistData.playlists;
    const currentIndex = musicStore.persistData.playSongIndex;
    const listLength = playlist.length;

    // Determine next song index (same logic as _doCrossfade)
    let nextIndex: number;
    if (musicStore.persistData.playSongMode === 'random') {
      nextIndex = Math.floor(Math.random() * listLength);
    } else {
      nextIndex = (currentIndex + 1) % listLength;
    }

    const nextSong = playlist[nextIndex];
    if (!nextSong) return;

    this._isBuffering = true;

    const doPreBuffer = async () => {
      // Step 1: Fetch music URL
      const res = await getMusicUrl(nextSong.id);
      if (!res?.data?.[0]?.url) throw new Error('Failed to get music URL');

      // Bail if state changed
      if (stateGetter() !== 'waiting') return;

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
      if (stateGetter() !== 'waiting') {
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
            }
          } catch (err) {
            if (IS_DEV) console.warn('PreBufferManager: Analysis failed', err);
          }
        }
      }

      // Bail if state changed during analysis
      if (stateGetter() !== 'waiting') {
        sound.stop();
        sound.unload();
        return;
      }

      // Step 5: Initialize audio graph so GainNode is ready for crossfade
      await sound.ensureAudioGraph();

      // Final bail check
      if (stateGetter() !== 'waiting') {
        sound.stop();
        sound.unload();
        return;
      }

      // Store pre-buffered state
      this._preBuffered = {
        sound,
        songIndex: nextIndex,
        analysis: preBufferedAnalysis,
      };

      if (IS_DEV) {
        console.log(`PreBufferManager: Pre-buffered next track "${nextSong.name}" (index=${nextIndex})`);
      }
    };

    doPreBuffer().catch((err) => {
      if (IS_DEV) {
        console.warn('PreBufferManager: Pre-buffer failed, will use slow path', err);
      }
    }).finally(() => {
      this._isBuffering = false;
    });
  }

  /**
   * Consume the pre-buffered sound (transfers ownership to caller).
   * Returns null if no pre-buffered sound available or index doesn't match.
   */
  consume(expectedIndex: number): PreBufferState | null {
    if (!this._preBuffered) return null;
    if (this._preBuffered.songIndex !== expectedIndex) {
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
