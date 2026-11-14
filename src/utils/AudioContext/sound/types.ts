/**
 * The one contract every sound implements, and the capability split on top of it.
 *
 * `ISound` is what playback, AutoMix and the UI may assume of any sound.
 * `BackendSound` adds what only a sound driven by the Rust backend (Tauri IPC
 * or WASM) can answer; narrow with `isBackendSound`, never with `instanceof`.
 */

import type { AudioEffectManager } from "../analysis/AudioEffectManager";
import type {
  NativePlannerStatus,
  NativePlaybackManifest,
  NativeResolverConfig,
  SongData,
  TrackIdentity,
} from "../../tauri/audio/protocol";

export interface SoundOptions {
  src: string | string[];
  volume?: number;
  preload?: boolean;
}

/** Event name → the arguments its listeners receive. */
export interface SoundEventMap {
  load: [];
  play: [];
  pause: [];
  end: [];
  fade: [];
  loaderror: [error?: unknown];
  playerror: [error?: unknown];
  progress: [ratio: number];
  stalled: [];
  waiting: [];
  resumed: [];
  /** The backend started a track this frontend did not ask for. */
  trackchange: [change: BackendTrackChange];
}

/**
 * A backend-initiated track change, whole: a subscriber adopts it from these
 * fields alone and reads nothing back from the sound. Emitted once per backend
 * timeline, after the sound itself is anchored on the new track.
 */
export interface BackendTrackChange {
  timelineEpoch: number;
  identity: TrackIdentity | null;
  musicId: string;
  playlistIndex: number;
  position: number;
  duration: number;
}

export type SoundEventType = keyof SoundEventMap;

export const SOUND_EVENTS = [
  "load",
  "play",
  "pause",
  "end",
  "fade",
  "loaderror",
  "playerror",
  "progress",
  "stalled",
  "waiting",
  "resumed",
  "trackchange",
] as const satisfies readonly SoundEventType[];

export type SoundEventCallback<E extends SoundEventType = SoundEventType> = (
  ...args: SoundEventMap[E]
) => void;

export interface AudioQualityInfo {
  bitrate: number;
  sampleRate: number;
  channels: number;
}

export interface ISound {
  /** `element`: an `<audio>` in this page. `backend`: the Rust audio backend. */
  readonly kind: "element" | "backend";
  /** Playback position in seconds. */
  readonly position: number;
  /** User volume, 0..1. */
  readonly volume: number;

  playing(): boolean;
  duration(): number;
  play(): this;
  pause(): this;
  stop(): this;
  seekTo(position: number): this;
  setVolume(volume: number): this;
  fade(from: number, to: number, duration: number): this;

  on<E extends SoundEventType>(event: E, callback: SoundEventCallback<E>): this;
  once<E extends SoundEventType>(event: E, callback: SoundEventCallback<E>): this;
  off<E extends SoundEventType>(event: E, callback?: SoundEventCallback<E>): this;

  getFrequencyData(): Uint8Array<ArrayBuffer>;
  getFFTData(): ArrayLike<number>;
  getLowFrequencyVolume(): number;
  getAverageAmplitude(): number;
  getEffectManager(): AudioEffectManager | null;
  getGainNode(): GainNode | null;
  ensureAudioGraph(): Promise<boolean>;

  unload(): void;
}

export interface BackendLoadOptions {
  allowInitialBackendAttach?: boolean;
  /**
   * Adopt a track the backend is *already* playing, matched on stable identity
   * rather than on `local:<url>`.
   *
   * The URL is re-resolved with a fresh CDN token every session, so after a
   * WebView reload the id-based comparison can never match and the controller
   * would replace live playback with the frontend's persisted (stale) track.
   * When this is set and the backend reports the same identity, `load()`
   * attaches without sending `setPlaylist`/`jumpToSong` at all.
   */
  attachIdentity?: TrackIdentity | null;
}

export interface BackendSound extends ISound {
  readonly kind: "backend";

  load(initialPosition?: number, options?: BackendLoadOptions): Promise<void>;
  isDestroyed(): boolean;
  isBuffering(): boolean;
  getSourceUrl(): string;
  getAudioQuality(): AudioQualityInfo | null;
  requestStatusSync(timeoutMs?: number): Promise<void>;

  setAnalysisEnabled(enabled: boolean): void;
  setFFTEnabled(enabled: boolean): void;

  setNativeManifest(manifest: NativePlaybackManifest): void;
  clearNativeManifest(revision: number): void;
  setNativeResolverConfig(config: NativeResolverConfig): void;
  setNativePlannerEnabled(enabled: boolean): void;
  isNativePlannerActive(): boolean;
  getNativePlannerStatus(): NativePlannerStatus | null;
  applyNativeQueueWindow(songs: SongData[], options: { windowed: boolean }): boolean;
  clearPlaybackQueue(): void;
  setListenTogetherRoom(roomId: string | null): boolean;
}

export function isBackendSound(sound: ISound | null | undefined): sound is BackendSound {
  return sound?.kind === "backend";
}
