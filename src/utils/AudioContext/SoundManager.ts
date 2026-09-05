/**
 * SoundManager - Singleton manager for the current sound instance
 *
 * Key improvements:
 * - Reduced debug logging in production
 * - Proper cleanup of global references
 */

import type { ISound } from "./types";

const IS_DEV = import.meta.env?.DEV ?? false;

/**
 * SoundManager - Static manager to replace Howler global
 * Supports dual sound instances for AutoMix crossfade transitions.
 */
class SoundManagerClass {
  private _currentSound: ISound | null = null;
  private _outgoingSound: ISound | null = null;
  private _currentSongId: number | null = null;
  private _outgoingSongId: number | null = null;

  unload(): void {
    if (this._outgoingSound) {
      if (IS_DEV) {
        console.log("SoundManager: unloading outgoing sound");
      }
      this._outgoingSound.unload();
      this._outgoingSound = null;
      this._outgoingSongId = null;
    }
    if (this._currentSound) {
      const currentSound = this._currentSound;
      if (IS_DEV) {
        console.log("SoundManager: unloading current sound");
      }
      currentSound.unload();
      this._currentSound = null;
      this._currentSongId = null;
      // Clear global reference to allow garbage collection
      if (window.$player === currentSound) {
        window.$player = undefined;
      }
    }
  }

  setCurrentSound(sound: ISound, songId?: number | null): void {
    this._currentSound = sound;
    this._currentSongId = this._normalizeSongId(songId);
  }

  getCurrentSound(): ISound | null {
    return this._currentSound;
  }

  setCurrentSongId(songId: number | null | undefined, sound = this._currentSound): void {
    if (!sound) return;
    const normalized = this._normalizeSongId(songId);
    if (sound === this._currentSound) {
      this._currentSongId = normalized;
    } else if (sound === this._outgoingSound) {
      this._outgoingSongId = normalized;
    }
  }

  getSongId(sound = this._currentSound): number | null {
    if (!sound) return null;
    if (sound === this._currentSound) return this._currentSongId;
    if (sound === this._outgoingSound) return this._outgoingSongId;
    return null;
  }

  isCurrentSoundForSong(sound: ISound, songId: number | null | undefined): boolean {
    return sound === this._currentSound && this._currentSongId === this._normalizeSongId(songId);
  }

  unloadIfCurrent(sound: ISound): boolean {
    if (sound !== this._currentSound) return false;
    sound.unload();
    this._currentSound = null;
    this._currentSongId = null;
    if (window.$player === sound) {
      window.$player = undefined;
    }
    return true;
  }

  /**
   * Begin a crossfade transition: move current sound to outgoing,
   * set the new sound as current.
   */
  beginTransition(newSound: ISound, songId?: number | null): void {
    if (IS_DEV) {
      console.log("SoundManager: beginTransition — current → outgoing");
    }
    // If there's already an outgoing sound, unload it first
    if (this._outgoingSound) {
      this._outgoingSound.unload();
    }
    this._outgoingSound = this._currentSound;
    this._outgoingSongId = this._currentSongId;
    this._currentSound = newSound;
    this._currentSongId = this._normalizeSongId(songId);
  }

  /**
   * Unload the outgoing sound after crossfade completes.
   */
  unloadOutgoing(): void {
    if (this._outgoingSound) {
      if (IS_DEV) {
        console.log("SoundManager: unloading outgoing sound");
      }
      this._outgoingSound.unload();
      this._outgoingSound = null;
      this._outgoingSongId = null;
    }
  }

  /**
   * Revert a crossfade transition: move outgoing back to current,
   * stop and unload the current (incoming) sound.
   * Used by AutoMixEngine.cancelCrossfade() when transition needs to be undone.
   */
  revertTransition(): void {
    if (!this._outgoingSound) return;
    if (IS_DEV) {
      console.log("SoundManager: revertTransition — incoming → unloaded, outgoing → current");
    }
    if (this._currentSound) {
      this._currentSound.stop();
      this._currentSound.unload();
    }
    this._currentSound = this._outgoingSound;
    this._currentSongId = this._outgoingSongId;
    this._outgoingSound = null;
    this._outgoingSongId = null;
  }

  /**
   * Get the outgoing sound (during crossfade)
   */
  getOutgoingSound(): ISound | null {
    return this._outgoingSound;
  }

  /**
   * Check if a sound is currently loaded
   */
  hasSound(): boolean {
    return this._currentSound !== null;
  }

  /**
   * Check if currently playing
   */
  isPlaying(): boolean {
    return this._currentSound?.playing() ?? false;
  }

  /**
   * Normalize a song id for identity comparison.
   *
   * Accepts **negative** ids: an imported local file's `SongData.id` is a
   * negative hash of its path (`local::index::local_song_id`), so a
   * positive-only filter turned every local track's id into `null` — which made
   * `isCurrentSoundForSong` answer for the wrong sound and, through
   * `isCurrentLoadOwner`, had the load handler discard the sound it had just
   * loaded. The symptom was a local track that spun forever and never played.
   *
   * `0` is still rejected: it is what an absent or unparsable id coerces to, and
   * treating that as an identity would let two unrelated sounds match.
   */
  private _normalizeSongId(songId: number | null | undefined): number | null {
    const value = Number(songId);
    return Number.isFinite(value) && value !== 0 ? value : null;
  }
}

export const SoundManager = new SoundManagerClass();
