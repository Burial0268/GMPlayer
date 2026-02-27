/**
 * AudioPreloader — Preloads the next song as a fully buffered BufferedSound.
 *
 * When the current song starts playing, this module creates a BufferedSound
 * for the next song in the playlist. On track change, the preloaded sound
 * is consumed directly, skipping API calls and network download entirely.
 *
 * - Preload depth: 1 song (~5-15MB in memory)
 * - Only activates in 'normal' play mode (random/single are unpredictable)
 * - Coexists with AutoMix (skips preload when AutoMix is active)
 */

import { BufferedSound } from "./BufferedSound";
import { getMusicUrl, getMusicNumUrl } from "@/api/song";
import { getAutoMixEngine } from "./AutoMix";
// Import stores directly to avoid circular dependency through barrel exports
import useMusicDataStore from "@/store/musicData";
import useSettingDataStore from "@/store/settingData";
import { MusicLevel } from "@/api/types";

// Lazy store access (called at runtime, not import time)
const musicStore = () => useMusicDataStore();
const settingStore = () => useSettingDataStore();

interface PreloadedEntry {
  songId: number;
  songIndex: number;
  sound: BufferedSound;
  ready: boolean;
}

const IS_DEV = import.meta.env?.DEV ?? false;
const useUnmServerHas = !!import.meta.env.VITE_UNM_API;

export class AudioPreloader {
  private _entry: PreloadedEntry | null = null;
  private _isPreloading: boolean = false;
  private _abortController: AbortController | null = null;

  /** Called when current song starts playing — preloads the next song */
  preloadNext(): void {
    const music = musicStore();
    const setting = settingStore();

    // Guard: only normal mode, not FM, list >= 2, not already preloading
    if (music.persistData.personalFmMode) return;
    if (music.persistData.playSongMode !== "normal") return;
    if (music.persistData.playlists.length < 2) return;
    if (this._isPreloading) return;

    // Guard: AutoMix active — let it handle transitions
    const autoMix = getAutoMixEngine();
    const state = autoMix.getState();
    if (
      state === "analyzing" ||
      state === "waiting" ||
      state === "crossfading" ||
      state === "finishing"
    ) {
      return;
    }

    // Compute next index
    const currentIndex = music.persistData.playSongIndex;
    const listLength = music.persistData.playlists.length;
    const nextIndex = (currentIndex + 1) % listLength;
    const nextSong = music.persistData.playlists[nextIndex];
    if (!nextSong) return;

    // Guard: already cached this exact song
    if (this._entry && this._entry.songId === nextSong.id && this._entry.songIndex === nextIndex) {
      return;
    }

    // Guard: VIP song with UNM enabled — UNM uses different source
    if (
      useUnmServerHas &&
      setting.useUnmServer &&
      !nextSong.pc &&
      (nextSong.fee === 1 || nextSong.fee === 4)
    ) {
      if (IS_DEV) {
        console.log(`[AudioPreloader] Skipping VIP song with UNM: ${nextSong.name}`);
      }
      return;
    }

    // Clean up any previous entry before starting new preload
    this.cleanup();

    this._isPreloading = true;
    this._abortController = new AbortController();
    const abortSignal = this._abortController.signal;

    if (IS_DEV) {
      console.log(
        `[AudioPreloader] Starting preload: "${nextSong.name}" (id: ${nextSong.id}, index: ${nextIndex})`,
      );
    }

    const level = setting.songLevel || "exhigh";

    this._resolveAndPreload(nextSong, nextIndex, level, abortSignal);
  }

  /** Resolve music URL (with trial detection + UNM fallback) and create BufferedSound */
  private async _resolveAndPreload(
    nextSong: any,
    nextIndex: number,
    level: string,
    abortSignal: AbortSignal,
  ): Promise<void> {
    try {
      // Step 1: Resolve URL (with trial version detection + UNM fallback)
      let url: string | null = null;

      try {
        const res = await getMusicUrl(nextSong.id, level as MusicLevel);
        if (abortSignal.aborted) return;

        const rawUrl = res?.data?.[0]?.url;
        if (rawUrl) {
          url = rawUrl.replace(/^http:/, "https:");
          // Check for trial version (jd-musicrep-ts) - needs UNM replacement
          if (url.includes("jd-musicrep-ts") && useUnmServerHas) {
            if (IS_DEV) {
              console.log(`[AudioPreloader] ${nextSong.name} is trial version, trying UNM`);
            }
            url = null; // Force UNM fallback
          }
        }
      } catch (err) {
        if (IS_DEV) {
          console.warn(`[AudioPreloader] getMusicUrl failed for ${nextSong.name}:`, err);
        }
        url = null;
      }

      if (abortSignal.aborted) return;

      // UNM fallback: if no URL or trial version detected
      if (!url && useUnmServerHas) {
        try {
          const unmRes = await getMusicNumUrl(nextSong.id);
          if (abortSignal.aborted) return;
          if (unmRes?.code === 200 && unmRes?.data?.url) {
            url = unmRes.data.url.replace(/^http:/, "https:");
            if (IS_DEV) {
              console.log(`[AudioPreloader] Got UNM URL for ${nextSong.name}`);
            }
          }
        } catch (unmErr) {
          if (IS_DEV) {
            console.warn(`[AudioPreloader] UNM fallback failed for ${nextSong.name}:`, unmErr);
          }
        }
      }

      if (!url || abortSignal.aborted) {
        if (IS_DEV && !url) {
          console.warn(`[AudioPreloader] No URL resolved for: ${nextSong.name}`);
        }
        this._isPreloading = false;
        return;
      }

      // Step 2: Create BufferedSound with volume=0 (will be set to real volume on consume)
      const sound = new BufferedSound({
        src: [url],
        preload: true,
        volume: 0,
      });

      const entry: PreloadedEntry = {
        songId: nextSong.id,
        songIndex: nextIndex,
        sound,
        ready: false,
      };
      this._entry = entry;

      // Wait for load with timeout
      const timeoutId = setTimeout(() => {
        if (!entry.ready && this._entry === entry) {
          if (IS_DEV) {
            console.warn(`[AudioPreloader] Timeout for: ${nextSong.name}`);
          }
          this.cleanup();
        }
      }, 30000);

      sound.once("load", () => {
        clearTimeout(timeoutId);
        if (abortSignal.aborted || this._entry !== entry) return;
        entry.ready = true;
        this._isPreloading = false;
        if (IS_DEV) {
          console.log(`[AudioPreloader] Preloaded: "${nextSong.name}" (id: ${nextSong.id})`);
        }
      });

      sound.once("loaderror", () => {
        clearTimeout(timeoutId);
        if (this._entry === entry) {
          if (IS_DEV) {
            console.warn(`[AudioPreloader] Load error for: ${nextSong.name}`);
          }
          this.cleanup();
        }
      });
    } catch (err) {
      if (abortSignal.aborted) return;
      if (IS_DEV) {
        console.warn("[AudioPreloader] _resolveAndPreload failed:", err);
      }
      this._isPreloading = false;
    }
  }

  /** Consume the preloaded BufferedSound if it matches songId. Returns null otherwise. */
  consume(songId: number): BufferedSound | null {
    if (!this._entry) return null;

    if (this._entry.songId === songId && this._entry.ready) {
      const sound = this._entry.sound;
      this._entry = null;
      this._isPreloading = false;
      if (IS_DEV) {
        console.log(`[AudioPreloader] Consumed preloaded sound for id: ${songId}`);
      }
      return sound;
    }

    // Mismatch or not ready — cleanup
    this.cleanup();
    return null;
  }

  /** Check if a specific song is preloaded (without consuming) */
  has(songId: number): boolean {
    return this._entry !== null && this._entry.songId === songId && this._entry.ready;
  }

  /** Clean up all preloaded resources */
  cleanup(): void {
    if (this._abortController) {
      this._abortController.abort();
      this._abortController = null;
    }
    if (this._entry) {
      // Unload the preloaded sound to free memory
      try {
        this._entry.sound.unload();
      } catch {
        // Ignore errors during cleanup
      }
      this._entry = null;
    }
    this._isPreloading = false;
  }

  get isPreloading(): boolean {
    return this._isPreloading;
  }
}

// Singleton
let _instance: AudioPreloader | null = null;

export function getAudioPreloader(): AudioPreloader {
  if (!_instance) {
    _instance = new AudioPreloader();
  }
  return _instance;
}
