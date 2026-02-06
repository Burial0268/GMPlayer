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
  getLowFrequencyVolume(): number;
  unload(): void;
}

/**
 * Options for low frequency analyzer
 * Based on AMLL (Apple Music-like Lyrics) implementation
 */
export interface LowFreqAnalyzerOptions {
  /** Number of low frequency bins to analyze (default: 10) */
  binCount?: number;
  /** Boost multiplier applied to normalized volume (default: 3.0) */
  boostMultiplier?: number;
  /** Volume floor: when volume > floorThreshold, clamp to at least this value (default: 0.4) */
  minimumFloor?: number;
  /** Threshold above which minimumFloor is applied (default: 0.1) */
  floorThreshold?: number;
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

declare global {
  interface Window {
    $player: ISound | undefined;
    AudioContext: typeof AudioContext;
    webkitAudioContext: typeof AudioContext;
    $message: {
      info: (message: string, options?: Record<string, unknown>) => void;
      warning: (message: string, options?: Record<string, unknown>) => void;
      error: (message: string, options?: Record<string, unknown>) => void;
    };
    $setSiteTitle: () => void;
    $getPlaySongData: (data: unknown) => void;
  }
}

export {};
