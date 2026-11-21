/**
 * Inbound protocol: the `AudioThreadEvent` union and its payload types.
 *
 * Mirrors the Rust `AudioThreadEvent` enum. Every event arrives in an
 * `EventEnvelope` stamped by the backend's event bus; see `EventMeta`.
 */
import type { SongData } from "./messages";
import type {
  NativeFmSnapshot,
  NativePlannerStatus,
  NativeSessionSnapshot,
  PlaybackPhase,
  TrackIdentity,
} from "./manifest";

/**
 * What the backend's event bus stamped on an event. Mirrors the Rust
 * `EventEnvelope`; consumers order and gate on these instead of payload fields.
 */
export interface EventMeta {
  /**
   * The bus's total order. A `hello` is "every event up to its `seq`", so the
   * transport drops anything older that arrives after one. `0` marks an
   * analysis frame, which carries no state and is never ordered.
   */
  seq: number;
  /** The timeline the backend was on when it emitted the event. Only grows along the stream. */
  epoch: number;
  /**
   * The last command of this page the backend had taken up. An event with
   * `ack < K` was emitted before command `K` was processed — see `CommandTicket`.
   */
  ack: number;
}

export interface EventEnvelope extends EventMeta {
  data: AudioThreadEvent;
}

export interface AudioQuality {
  bitrate: number;
  sampleRate: number;
  channels: number;
  /** Lowercase decoder name (`flac`, `mp3`, `pcm_s24le`…); empty or absent when unknown. */
  codec?: string;
  /** Valid bits per sample for lossless/PCM codecs; absent for lossy ones. */
  bitsPerSample?: number | null;
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
  /** Where the outgoing song's outro begins; the transition state starts there. */
  outroStart?: number | null;
  error?: string | null;
}

export type AudioThreadEvent =
  /**
   * First on every subscription, and again in the stream when a stopped native
   * player was replaced (`bootId` changes and the snapshot is empty).
   * Everything delivered after it is newer than `snapshot`.
   */
  | { type: "hello"; data: { bootId: number; snapshot: NativeSessionSnapshot } }
  | { type: "personalFmChanged"; data: { session: NativeFmSnapshot | null } }
  | {
      type: "personalFmTrashResult";
      data: { sessionId: number; id: string; error: string | null };
    }
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
      /**
       * The one track-change edge: once per timeline that starts a track,
       * ahead of the `loadAudio` / `syncStatus` / `nativePlannerAdvanced` /
       * `automixCrossfadeComplete` describing the same change, and complete on
       * its own. `loadRequestId` is set only for a load this frontend asked for.
       */
      type: "trackChanged";
      data: {
        timelineEpoch: number;
        musicId: string;
        identity?: TrackIdentity | null;
        currentPlayIndex: number;
        musicInfo: DisplayAudioInfo;
        quality: AudioQuality;
        isPlaying: boolean;
        loadRequestId?: number | null;
      };
    }
  | {
      type: "loadingAudio";
      data: { musicId: string; currentPlayIndex: number; loadRequestId?: number | null };
    }
  | { type: "buffering"; data: { musicId: string; isBuffering: boolean } }
  | {
      type: "playbackPhaseChanged";
      data: { phase: PlaybackPhase; identity?: TrackIdentity | null; timelineEpoch: number };
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
  | { type: "outputLatency"; data: { latencyMs: number } }
  | { type: "audioOutputError"; data: { error: string; recoverable: boolean } }
  | { type: "fftData"; data: { data: number[] } }
  | { type: "lowFrequencyVolume"; data: { volume: number } }
  | { type: "lowFreqBins"; data: { bins: number[] } }
  /** The native backend's paced merge of the two above (~30 Hz). */
  | { type: "analysisFrame"; data: { fft: number[] | null; bins: number[] | null } }
  | { type: "automixStatus"; data: { status: AutoMixNativeStatus } }
  | {
      type: "automixAnalysisReady";
      data: { currentId: string; nextId: string; transitionId?: number | null };
    }
  | { type: "automixIncomingReady"; data: { transitionId?: number | null } }
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
      type: "nativeQueueOrderChanged";
      data: { manifestRevision: number; order: TrackIdentity[]; shuffleId?: string | null };
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
  /**
   * False while the backend has been told no mode by anyone: `playMode` is then its default,
   * not the user's choice, and must not be adopted.
   */
  playModeKnown: boolean;
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
  /** Cover of the planner's next track, for the OS session to prefetch. */
  nextArtworkUrl: string | null;
}

export type AudioThreadEventCallback = (event: AudioThreadEvent, meta: EventMeta) => void;
