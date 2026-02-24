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

  unload(): void {
    if (this._outgoingSound) {
      if (IS_DEV) {
        console.log("SoundManager: unloading outgoing sound");
      }
      this._outgoingSound.unload();
      this._outgoingSound = null;
    }
    if (this._currentSound) {
      if (IS_DEV) {
        console.log("SoundManager: unloading current sound");
      }
      this._currentSound.unload();
      this._currentSound = null;
      // Clear global reference to allow garbage collection
      if (window.$player) {
        window.$player = undefined;
      }
    }
  }

  setCurrentSound(sound: ISound): void {
    this._currentSound = sound;
  }

  getCurrentSound(): ISound | null {
    return this._currentSound;
  }

  /**
   * Begin a crossfade transition: move current sound to outgoing,
   * set the new sound as current.
   */
  beginTransition(newSound: ISound): void {
    if (IS_DEV) {
      console.log("SoundManager: beginTransition — current → outgoing");
    }
    // If there's already an outgoing sound, unload it first
    if (this._outgoingSound) {
      this._outgoingSound.unload();
    }
    this._outgoingSound = this._currentSound;
    this._currentSound = newSound;
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
    this._outgoingSound = null;
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
}

export const SoundManager = new SoundManagerClass();
