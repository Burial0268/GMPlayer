/**
 * Outbound protocol: the `AudioThreadMessage` union and its payload types.
 *
 * Mirrors the Rust `AudioThreadMessage` enum in
 * `src-tauri/crates/audio-backend/src/player/messages.rs`. A single
 * `audio_send_msg` command carries every one of these, AMLL-style, instead of
 * one Tauri command per playback action — keep both sides in lockstep.
 */
import type { NativePlaybackManifest, NativeResolverConfig, TrackIdentity } from "./manifest";

/**
 * Display metadata carried with a queued track.
 *
 * The backend drives the OS media session (SMTC / MediaSession) on its own, and
 * on Android it keeps doing so after the WebView is gone. Every streamed track
 * is sent as `local` with an https `filePath`, which Rust downloads to a temp
 * file before decoding — so without this the only name available at load time
 * is that temp file's random stem. Send the real metadata and the notification
 * is correct on the first frame.
 */
export interface TrackDisplay {
  title?: string;
  artist?: string;
  album?: string;
  artworkUrl?: string;
}

export interface SongData {
  type: "local" | "custom";
  filePath?: string;
  id?: string;
  songJsonData?: string;
  origOrder: number;
  display?: TrackDisplay;
}
export interface AutoMixConfig {
  enabled: boolean;
  crossfadeDuration: number;
  bpmMatch: boolean;
  beatAlign: boolean;
  volumeNorm: boolean;
  smartCurve: boolean;
  transitionStyle: "linear" | "equalPower" | "sCurve";
  transitionEffects: boolean;
  vocalGuard: boolean;
}

export interface EqualizerBand {
  enabled?: boolean;
  filterType: "peaking" | "lowShelf" | "highShelf";
  frequency: number;
  gainDb: number;
  q: number;
}

export interface EqualizerConfig {
  enabled: boolean;
  preampDb?: number;
  bands?: EqualizerBand[];
}

export interface LimiterConfig {
  enabled: boolean;
  thresholdDb?: number;
  ceilingDb?: number;
  releaseMs?: number;
}

export interface DspConfig {
  enabled: boolean;
  inputGainDb?: number;
  equalizer?: EqualizerConfig;
  outputGainDb?: number;
  limiter?: LimiterConfig;
}

export type AudioThreadMessage =
  | { type: "resumeAudio" }
  | { type: "pauseAudio" }
  | { type: "resumeOrPauseAudio" }
  | { type: "seekAudio"; position: number; requestId?: number; expectedMusicId?: string }
  | { type: "jumpToSong"; songIndex: number }
  | { type: "jumpToSongAt"; songIndex: number; position: number }
  | { type: "prevSong" }
  | { type: "nextSong" }
  | { type: "nextSongGapless" }
  | {
      type: "setPlaylist";
      songs: SongData[];
      windowed?: boolean;
      playIndex?: number;
      initialPosition?: number;
      loadRequestId?: number;
    }
  | { type: "setVolume"; volume: number }
  | { type: "setVolumeRelative"; volume: number }
  | { type: "setAudioOutput"; name: string }
  | { type: "setAnalysis"; enabled: boolean }
  | { type: "setFFT"; enabled: boolean }
  | { type: "setFFTRange"; fromFreq: number; toFreq: number }
  | { type: "setEqualizer"; config: EqualizerConfig }
  | { type: "setDsp"; config: DspConfig }
  | { type: "syncStatus" }
  | { type: "close" }
  | { type: "setMediaControlsEnabled"; enabled: boolean }
  | { type: "automixSetEnabled"; enabled: boolean }
  | { type: "automixConfigure"; config: AutoMixConfig }
  | {
      type: "automixPrepareNext";
      currentIndex: number;
      nextIndex: number;
      nextSong: SongData;
      transitionId?: number | null;
    }
  | { type: "automixCancel" }
  | { type: "automixForceStart"; generation?: number | null }
  | { type: "automixCompleteNative"; generation: number; currentIndex: number; position: number }
  | { type: "setNativeManifest"; manifest: NativePlaybackManifest }
  | { type: "clearNativeManifest"; revision: number }
  | { type: "setNativeResolverConfig"; config: NativeResolverConfig }
  /**
   * Announce the track about to load, before its URL is resolved. Swaps the OS
   * media session immediately instead of after the resolve + download.
   */
  | { type: "announceTrack"; identity: TrackIdentity; display: TrackDisplay }
  | { type: "setNativePlannerEnabled"; enabled: boolean }
  | { type: "syncNativePlannerStatus" }
  /**
   * Arm/disarm the listen-together keepalive in the backend. `roomId: null`
   * disarms. The heartbeat must outlive the WebView — on Android the page is
   * destroyed while playback continues, and a missed heartbeat drops the user
   * out of the room server-side.
   */
  | { type: "setListenTogetherRoom"; roomId: string | null }
  /**
   * Push what the frontend knows about the session controls. A patch: omitted
   * fields are left alone, so the store can report the play mode without
   * claiming to know the like state and vice versa.
   */
  | {
      type: "setSessionControls";
      controls: {
        playMode?: "normal" | "random" | "single";
        favourite?: boolean;
        canFavourite?: boolean;
      };
    }
  /**
   * Advance the play mode one step. An intent rather than a value, because the
   * caller may be a notification button that cannot know the current mode — the
   * backend holds it. Sent by the desktop system-control handler for the same
   * reason: one writer.
   */
  | { type: "setNextPlayMode" }
  /** Toggle the loaded track's like state. The backend performs the call. */
  | { type: "toggleFavourite" };
