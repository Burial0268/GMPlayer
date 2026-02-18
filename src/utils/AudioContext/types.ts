/**
 * AudioContext Module Type Definitions
 */

/**
 * Options for creating a sound instance
 */
export interface SoundOptions {
  src: string | string[];
  volume?: number;
  preload?: boolean;
}

/**
 * Supported sound event types
 */
export type SoundEventType = 'load' | 'play' | 'pause' | 'end' | 'fade' | 'loaderror' | 'playerror' | 'progress';

/**
 * Sound event callback function signature
 */
export type SoundEventCallback = (...args: unknown[]) => void;

/**
 * Interface for Sound instances
 */
export interface ISound {
  playing(): boolean;
  play(): this;
  pause(): this;
  stop(): this;
  seek(pos?: number): number | this;
  duration(): number;
  volume(vol?: number): number | this;
  fade(from: number, to: number, duration: number): this;
  on(event: SoundEventType, callback: SoundEventCallback): this;
  once(event: SoundEventType, callback: SoundEventCallback): this;
  off(event: SoundEventType, callback?: SoundEventCallback): this;
  getFrequencyData(): Uint8Array<ArrayBuffer>;
  getFFTData(): number[];
  getLowFrequencyVolume(): number;
  getAverageAmplitude(): number;
  getEffectManager(): import('./AudioEffectManager').AudioEffectManager | null;
  unload(): void;
}

/**
 * Play song time data structure
 */
export interface PlaySongTime {
  currentTime: number;
  duration: number;
  barMoveDistance?: number;
  songTimePlayed?: string;
  songTimeDuration?: string;
}

// Global Window augmentations are in src/types/globals.d.ts
