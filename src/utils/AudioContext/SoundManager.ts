/**
 * SoundManager - Singleton manager for the current sound instance
 * Replaces the Howler global pattern
 */

import type { ISound } from './types';

/**
 * SoundManager - Static manager to replace Howler global
 */
class SoundManagerClass {
  private _currentSound: ISound | null = null;

  unload(): void {
    if (this._currentSound) {
      console.log('SoundManager: unloading current sound');
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
}

export const SoundManager = new SoundManagerClass();
