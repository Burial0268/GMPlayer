/**
 * Inbound protocol: the `AudioThreadEvent` union and its payload types.
 *
 * Mirrors the Rust `AudioThreadEvent` enum. Events reach the frontend over two
 * transports — the primary Tauri `Channel` and the global-emit fallback — so
 * every envelope carries a `seq` for dedup across a transport switch.
 */
import type { SongData } from "./messages";
import type { NativePlannerStatus, TrackIdentity } from "./manifest";

export interface AudioThreadEventMessage<T> {
  callbackId: string;
  data: T | null;
  /**
   * Monotonic sequence number stamped by the Rust event forwarder. The
   * primary Tauri `Channel` and the global-emit fallback deliver the same
   * event with the same `seq`; subscribers use it to drop duplicates that
   * arrive via the secondary transport during a fallback transition
   * (otherwise state-flip dedup breaks on Pause → Seek → Resume bursts and
   * similar patterns).
   *
   * `0` (or missing) means the event was not stamped — fall back to the
   * legacy "no dedup" behavior in that case.
   */
  seq?: number;
}

export interface AudioQuality {
  bitrate: number;
  sampleRate: number;
  channels: number;
}

export interface DisplayAudioInfo {
  name: string;
  artist: string;
  album: string;
  lyric: string;
  coverMediaType: string;
  cover: number[] | null;
  comment: string;
  duration: number;
  position: number;
}

export type AutoMixNativeState =
  | "idle"
  | "preparing"
  | "waiting"
  | "crossfading"
  | "finishing"
  | "failed";

export interface AutoMixNativeStatus {
  state: AutoMixNativeState;
  enabled: boolean;
  transitionId?: number | null;
  currentIndex: number;
  nextIndex?: number | null;
  currentId?: string | null;
  nextId?: string | null;
  crossfadeStart?: number | null;
  crossfadeDuration?: number | null;
  error?: string | null;
}

export type AudioThreadEvent =
  | {
      type: "playPosition";
      data: {
        position: number;
        /**
         * Which timeline this position belongs to. A subscriber holding a
         * different epoch has not adopted this track yet and must drop the
         * packet rather than weigh it against the clock it still holds — a
         * fresh track's `0` is otherwise indistinguishable from a stale packet.
         */
        timelineEpoch?: number;
      };
    }
  | { type: "loadProgress"; data: { position: number } }
  | {
      type: "loadAudio";
      data: {
        musicId: string;
        musicInfo: DisplayAudioInfo;
        quality: AudioQuality;
        currentPlayIndex: number;
        loadRequestId?: number | null;
        /**
         * Stable identity of the loaded track, when the backend knows it.
         * `musicId` is `local:<cdn-url>` and changes on every re-resolve, so it
         * can never reconcile across a WebView reload — this can.
         */
        identity?: TrackIdentity | null;
        /** Timeline this load started — see `playPosition.timelineEpoch`. */
        timelineEpoch?: number;
      };
    }
  | {
      type: "loadingAudio";
      data: { musicId: string; currentPlayIndex: number; loadRequestId?: number | null };
    }
  | { type: "audioPlayFinished"; data: { musicId: string } }
  | {
      type: "syncStatus";
      data: {
        musicId: string;
        musicInfo: DisplayAudioInfo;
        isPlaying: boolean;
        duration: number;
        position: number;
        volume: number;
        loadPosition: number;
        playlist: SongData[];
        currentPlayIndex: number;
        playlistInited: boolean;
        quality: AudioQuality;
        /** Stable identity of the playing track — see `loadAudio.identity`. */
        identity?: TrackIdentity | null;
        /** Timeline this snapshot describes — see `playPosition.timelineEpoch`. */
        timelineEpoch?: number;
      };
    }
  | {
      type: "playListChanged";
      data: { playlist: SongData[]; currentPlayIndex: number };
    }
  | { type: "playStatus"; data: { isPlaying: boolean } }
  | { type: "seekCommitted"; data: { requestId?: number | null; position: number } }
  | {
      type: "seekFailed";
      data: { requestId?: number | null; position: number; error: string };
    }
  | {
      type: "loadError";
      data: { error: string; musicId?: string; loadRequestId?: number | null };
    }
  | { type: "playError"; data: { error: string } }
  | { type: "volumeChanged"; data: { volume: number } }
  | {
      type: "audioOutputChanged";
      data: {
        deviceName: string;
        isDefault: boolean;
        channels: number;
        sampleRate: number;
        sampleFormat: string;
      };
    }
  | { type: "audioOutputError"; data: { error: string; recoverable: boolean } }
  | { type: "fftData"; data: { data: number[] } }
  | { type: "lowFrequencyVolume"; data: { volume: number } }
  | { type: "automixStatus"; data: { status: AutoMixNativeStatus } }
  | {
      type: "automixAnalysisReady";
      data: { currentId: string; nextId: string; transitionId?: number | null };
    }
  | {
      type: "automixCrossfadeStarted";
      data: { fromId: string; toId: string; duration: number; transitionId?: number | null };
    }
  | {
      type: "automixCrossfadeComplete";
      data: {
        currentIndex: number;
        musicId?: string;
        position?: number;
        duration?: number;
        transitionId?: number | null;
      };
    }
  | { type: "automixError"; data: { error: string; recoverable: boolean } }
  | { type: "nativePlannerStatusChanged"; data: { status: NativePlannerStatus } }
  | {
      type: "nativePlannerAdvanced";
      data: {
        manifestRevision: number;
        identity: TrackIdentity;
        playlistIndex: number;
        musicId: string;
      };
    }
  | {
      type: "nativePlannerExhausted";
      data: { manifestRevision: number; attempted: number; reason: string };
    }
  | { type: "nowPlayingChanged"; data: { info: NowPlayingInfo } }
  | { type: "sessionControlsChanged"; data: { controls: SessionControls } };

/**
 * Session controls: state that is *not* audio state.
 *
 * Play mode and favourite are the user's own choices, but they are rendered
 * next to the transport on every surface and settable from every surface. The
 * backend holds the one copy all of them agree on — see
 * `src-tauri/crates/audio-backend/src/player/session_controls.rs`. A surface
 * sends an intent and adopts what comes back; it never writes its own copy.
 */
export interface SessionControls {
  playMode: "normal" | "random" | "single";
  /** Whether the loaded track is in the user's 我喜欢的音乐. */
  favourite: boolean;
  /** Whether toggling is possible at all (logged in, Netease track). */
  canFavourite: boolean;
}

/**
 * Resolved display metadata for the current track, merged by the backend from
 * the manifest entry (streamed tracks have no file tags) and the decoder's tag
 * read (local files).
 *
 * The OS media session is driven from Rust off this same event, so the frontend
 * does not push notifications any more — it only consumes this for UI.
 */
export interface NowPlayingInfo {
  hasTrack: boolean;
  identity: TrackIdentity | null;
  title: string;
  artist: string;
  album: string;
  artworkUrl: string | null;
  duration: number;
  position: number;
  isPlaying: boolean;
  playlistIndex: number;
  /** Session controls carried with the projection — see [`SessionControls`]. */
  controls: SessionControls;
}

export type AudioThreadEventCallback = (event: AudioThreadEvent, seq?: number) => void;
