/**
 * SoundManager - Singleton manager for the current sound instance
 *
 * Key improvements:
 * - Reduced debug logging in production
 * - Proper cleanup of global references
 */

import type { ISound } from './types';

const IS_DEV = import.meta.env?.DEV ?? false;

/**
 * SoundManager - Static manager to replace Howler global
 */
class SoundManagerClass {
  private _currentSound: ISound | null = null;

  unload(): void {
    if (this._currentSound) {
      if (IS_DEV) {
        console.log('SoundManager: unloading current sound');
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
