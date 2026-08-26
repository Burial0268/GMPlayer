/**
 * NativeRustSound — an ISound implementation backed by the Tauri audio-backend.
 *
 * v6: Tauri Channel transport.
 *   - Events (Rust → frontend: FFT/status/position) flow over a
 *     `tauri::ipc::Channel` registered via `audio_subscribe_events`. It rides
 *     the webview's native IPC, so it works on WebView2 / WebKitGTK /
 *     WKWebView / Android WebView alike (the previous local WebSocket could
 *     not be consumed by WebKitGTK on Linux).
 *   - Playback commands go out over the `audio_send_msg` invoke — a separate
 *     IPC path from the event Channel, so play/pause/seek never queue behind
 *     FFT frames.
 *   - Play/pause/stop/seek are *optimistic*: the local `_playbackState`
 *     flips and the `play`/`pause` event fires synchronously, then the
 *     backend confirmation arrives and is de-duped.
 *   - Position is extrapolated client-side (last `playPosition` + elapsed
 *     wall time) so the seek-bar updates smoothly even though the Rust
 *     side only emits 1 Hz heartbeats.
 */

import type { ISound, SoundEventCallback, SoundEventType } from "../../AudioContext/types";
import { isTauri } from "../core/runtime";
import { AudioTimelineSync } from "./timeline";
import { sameTrackIdentity } from "./identity";
import type {
  AudioQuality,
  AudioThreadEvent,
  DisplayAudioInfo,
  NativePlannerStatus,
  SongData,
  TrackDisplay,
  TrackIdentity,
} from "./protocol";
import {
  getAudioBackendTransport,
  isWasmAudioBackendAvailable,
  type AudioBackendTransport,
} from "./transport";

const IS_DEV = import.meta.env?.DEV ?? false;
const LOAD_TIMEOUT_MS = 10_000;
const FFT_LOG_INTERVAL_MS = 1000;
const NO_FFT_WARN_MS = 5000;
const SEEN_EVENT_SEQ_LIMIT = 512;
const NATIVE_AUTOMIX_COMPLETE_EVENT = "gmplayer:native-automix-complete";
const NATIVE_AUTOMIX_SYNC_EVENT = "gmplayer:native-automix-sync";
const SEEK_REQUEST_ID_MODULO = 1000;
/** After AudioPlayFinished, how long to wait for the backend queue window to
 * confirm it is advancing (LoadingAudio) before falling back to the JS-driven
 * 'end' transition. The backend emits LoadingAudio immediately on advance. */
const NATIVE_ADVANCE_START_TIMEOUT_MS = 2500;
/** Once LoadingAudio confirmed the advance, how long the source download may
 * take before giving up and falling back to the JS-driven transition. */
const NATIVE_ADVANCE_LOAD_TIMEOUT_MS = 20_000;

/**
 * Observer for the backend's authoritative manifest revision.
 *
 * Registered by `NativeManifestPublisher` so it can keep its own counter ahead
 * of the backend's. Kept as a module-level hook rather than a direct import to
 * avoid a cycle (the publisher imports this module for the `instanceof` check).
 */
type PlannerRevisionObserver = (backendRevision: number) => void;
let plannerRevisionObserver: PlannerRevisionObserver | null = null;

export const setPlannerRevisionObserver = (observer: PlannerRevisionObserver | null): void => {
  plannerRevisionObserver = observer;
};

interface LocalState {
  musicId: string;
  position: number;
  duration: number;
  isPlaying: boolean;
  volume: number;
  playlist: SongData[];
  currentPlayIndex: number;
}

type EventMap = Record<string, SoundEventCallback[]>;

export function isNativeAudioBackendAvailable(): boolean {
  return isTauri();
}

/**
 * Tell the backend which track is about to load, before its URL is resolved.
 *
 * Module-level rather than a method because the sound for the new track does
 * not exist yet — it is created only after `resolveSongUrl` returns, and the
 * whole point is to update the OS media session before that round trip (plus
 * the download that follows) has happened. Sent through the shared transport,
 * so it reaches the same backend the outgoing sound is talking to.
 *
 * Best-effort: if the transport is not up there is nothing to announce to, and
 * the normal load path will publish the metadata anyway.
 */
export function announceNativeTrack(
  identity: TrackIdentity,
  display: TrackDisplay | undefined,
): void {
  if (!isTauri() || !display) return;
  try {
    getAudioBackendTransport().sendOrQueue({ type: "announceTrack", identity, display });
  } catch (err) {
    if (IS_DEV) console.warn("[NativeRustSound] announce failed", err);
  }
}

export function isAudioBackendRuntimeAvailable(): boolean {
  return isTauri() || isWasmAudioBackendAvailable();
}

/** Resolved/rejected when the load completes — or rejected on timeout. */
type LoadPromise = {
  resolve: () => void;
  reject: (err: Error) => void;
  timeout: ReturnType<typeof setTimeout>;
  requestId?: number;
};

type SyncPromise = {
  resolve: () => void;
  timeout: ReturnType<typeof setTimeout>;
};

type NativeLoadOptions = {
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
};

export class NativeRustSound implements ISound {
  private _events: EventMap = {};
  private _onceEvents: EventMap = {};
  private _transport: AudioBackendTransport | null = null;
  private _unlistenTransport: (() => void) | null = null;

  private _path: string;
  /**
   * Display metadata for this track, sent with the very first `setPlaylist`.
   *
   * Without it the backend's only name at load time is the temp file it
   * downloads this URL into — a random stem — so SMTC/MediaSession showed
   * garbage on every track change until a decoded tag caught up.
   */
  private _display?: TrackDisplay;
  private _volume: number = 1;
  private _muted: boolean = false;

  /** Computed musicId for this track (must match Rust's `SongData::get_id`). */
  private _expectedMusicId: string;

  /** Accumulated state from SyncStatus / PlayPosition / PlayStatus events. */
  private _state: LocalState = {
    musicId: "",
    position: 0,
    duration: 0,
    isPlaying: false,
    volume: 1,
    playlist: [],
    currentPlayIndex: 0,
  };

  /** Shared optimistic seek guard + extrapolated playback clock. */
  private _timeline: AudioTimelineSync = new AudioTimelineSync();

  /**
   * Recently processed event sequence ids. A single Channel delivers events
   * in order, but during a Channel → global-emit fallback transition the same
   * event can briefly arrive over both paths; the `seq` stamp lets us drop the
   * exact duplicate. Lower seq is still valid if it was not seen yet.
   */
  private _seenEventSeq: Set<number> = new Set();
  private _seenEventSeqOrder: number[] = [];
  /** Index of the oldest live entry in `_seenEventSeqOrder` — eviction moves
   * this head forward instead of `Array#shift`, which is O(window) per event
   * once the window is full (i.e. for every event after the first ~512). */
  private _seenEventSeqHead = 0;

  /** Track metadata from SyncStatus / LoadAudio. */
  private _musicInfo: DisplayAudioInfo | null = null;
  private _quality: AudioQuality | null = null;

  /** Raw FFT magnitudes from the `fftData` event. */
  private _fftData: number[] = [];
  private _frequencyData: Uint8Array<ArrayBuffer> = new Uint8Array(0);
  private _averageAmplitude: number = 0;
  /** Rust-computed low-frequency volume from the `lowFrequencyVolume`
   * event, derived from the same raw FFT frame as `fftData`. */
  private _lowFreqVolume: number = 0;
  private _analysisEnabled: boolean | null = null;
  private _fftEventsEnabled: boolean | null = null;

  private _loaded: boolean = false;
  private _destroyed: boolean = false;
  private _terminallyCleared: boolean = false;
  private _playbackState: "stopped" | "playing" | "paused" | "ended" = "stopped";
  private _pendingPlayCommand: boolean = false;
  private _optimisticPlayback: boolean = isTauri();
  private _adoptNextBackendMusicId: boolean = false;
  private _nativeAutoMixSyncPending: boolean = false;
  private _allowInitialBackendAttach: boolean = false;
  /** Identity to match when attaching to an already-playing backend track. */
  private _attachIdentity: TrackIdentity | null = null;
  private _backendTrackReady: boolean = false;
  private _nextLoadRequestId: number = 0;
  private _nextSeekRequestId: number = 0;
  /** True once a queue window has been applied to the backend: track-end
   * transitions should then be attempted as backend-initiated advances (the
   * fallback timer restores the JS-driven path when no advance lands). Stays
   * true across multiple background advances — wake-up replays several
   * AudioPlayFinished/LoadAudio pairs back-to-back, faster than the async
   * prefill could re-arm a per-transition flag. */
  private _nativeAdvanceWindowApplied: boolean = false;
  /** Mirrors `NativePlannerStatus.active`. Unlike the window flag this needs no
   * per-hop re-arming: once a manifest is published the planner owns every
   * advance until it is cleared or exhausted. */
  private _nativePlannerActive: boolean = false;
  private _nativePlannerStatus: NativePlannerStatus | null = null;
  /** Identity + index of the most recent backend-initiated planner advance,
   * retained until adoption consumes it. This is the only reliable key for
   * reconciling a planner advance: the backend resolves its own URLs, so the
   * `local:<url>` prefill registry cannot map them back to a song. */
  private _lastPlannerAdvance: {
    identity: TrackIdentity;
    playlistIndex: number;
  } | null = null;
  /**
   * Stable identity of the track the backend currently has loaded, as reported
   * on every `syncStatus` / `loadAudio`.
   *
   * Unlike `_lastPlannerAdvance` this is not consumed: a single backend track
   * change produces several adoption events (the load's own, then the sync that
   * follows it, then any audit sync), and only the first of them could read a
   * one-shot value. The later ones then fell back to the index — which is how a
   * stale AutoMix transition target dragged the store back onto a song the
   * backend had already left, and, once republished as a manifest cursor, took
   * the media session's metadata with it.
   */
  private _backendIdentity: TrackIdentity | null = null;
  /**
   * Which backend timeline this controller's clock is anchored to.
   *
   * The backend stamps every position anchor, load and status snapshot with a
   * monotonic epoch that it bumps only where playback restarts from a new
   * source. That single number answers the question this side used to guess at:
   * a position *behind* the one we hold is a stale packet within the same
   * epoch, and a fresh track's timeline in a new one. Guessing from the
   * magnitude meant a new track's `0` was rejected as a rewind and the clock
   * carried on extrapolating the retired track — the media-session "next"
   * button showed the previous track's elapsed time plus the new track's.
   *
   * `null` until the first stamped event: a backend too old to send one, or the
   * moment before the first sync, both fall back to the identity/id checks.
   */
  private _backendTimelineEpoch: number | null = null;
  /** True between AudioPlayFinished and the adoption of the backend-initiated
   * advance (LoadAudio for the next track) — the 'end' event is suppressed
   * while pending and re-emitted by the fallback if the advance never lands. */
  private _nativeAdvancePending: boolean = false;
  private _nativeAdvanceFallbackTimer: ReturnType<typeof setTimeout> | null = null;

  /** Pending load() promise so we can resolve from event handlers. */
  private _pendingLoad: LoadPromise | null = null;
  private _pendingSyncs: SyncPromise[] = [];

  // ── Diagnostics ─────────────────────────────────────────────────
  private _lastFFTLogAt: number = 0;
  private _fftReceived: boolean = false;
  private _noFFTWarnTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(src: string | string[], display?: TrackDisplay) {
    this._path = Array.isArray(src) ? src[0] : src;
    this._display = display;
    // Rust computes `local:<file_path>` (see types.rs::SongData::get_id);
    // the reference AMLL hashes the path, but our backend uses the raw
    // path so we mirror that here.
    this._expectedMusicId = `local:${this._path}`;
  }

  // ═════════════════════════════════════════════════════════════╗
  //  load() — register listeners, open track, await load event  ║
  // ═════════════════════════════════════════════════════════════╝

  async load(initialPosition?: number, options?: NativeLoadOptions): Promise<void> {
    if (this._loaded || this._destroyed || this._terminallyCleared) return;

    if (!isAudioBackendRuntimeAvailable()) {
      this._emit("loaderror", new Error("Audio backend runtime not available"));
      return;
    }

    // 1. Connect the transport (best-effort) and register listeners BEFORE
    //    sending any messages so we can't miss the LoadAudio event.
    const transport = getAudioBackendTransport();
    try {
      await transport.connect();
      this._transport = transport;
      this._unlistenTransport = transport.subscribe((evt, seq) => this._handleEvent(evt, seq));
    } catch (e) {
      const err = e instanceof Error ? e : new Error(String(e));
      this._emit("loaderror", err);
      return;
    }
    if (this._destroyed || this._terminallyCleared) return;

    const initPos = this._normalizeSeekPosition(initialPosition ?? 0);
    this._backendTrackReady = false;
    this._timeline.reset(initPos);
    this._attachIdentity = options?.attachIdentity ?? null;

    // Only the native/Tauri runtime can have a real backend that outlives the
    // current frontend controller. The Web/WASM runtime is page-local and uses a
    // singleton JS transport, so adopting its previous state after SoundManager
    // clears window.$player can bind a new NativeRustSound to a stale <audio>.
    const canAttachExistingBackend =
      isTauri() && (options?.allowInitialBackendAttach === true || this._attachIdentity !== null);
    if (canAttachExistingBackend) {
      this._allowInitialBackendAttach = true;
      await this.requestStatusSync(400);
      this._allowInitialBackendAttach = false;
      if (this._destroyed || this._terminallyCleared) return;
      if (this._state.musicId && this._state.duration > 0) {
        // An identity attach must NOT correct the position: the caller derived
        // `initPos` from the same backend snapshot, and the gap between reading
        // it and getting here is real elapsed playback. Seeking back to the
        // sampled value would rewind ~a second on every app resume.
        if (
          this._attachIdentity === null &&
          initPos > 0 &&
          Math.abs(this._state.position - initPos) > 1
        ) {
          this.seek(initPos);
        }
        this._loaded = true;
        this._armNoFFTWarning();
        this._emit("load");
        return;
      }
      // The backend is not on this track after all — fall through to a normal
      // load so the caller still gets audio.
      this._attachIdentity = null;
    }

    // Pre-seed local state from `initialPosition` so seekers (e.g. the
    // RAF time-loop reading `sound.seek()`) return the saved position
    // even before the first event arrives from Rust. This is what makes
    // the progress bar display the resumed position immediately on
    // startup, not jump from 0 once events catch up.
    if (initPos > 0) {
      this._setLocalPosition(initPos, true);
    }

    // 2. Set up the load-completion promise BEFORE dispatching messages
    //    so events that arrive between dispatch and `await` are caught.
    const loadRequestId = isTauri() ? this._newLoadRequestId() : undefined;
    const loadDone = new Promise<void>((resolve, reject) => {
      const timeout = setTimeout(() => {
        if (this._pendingLoad) {
          this._pendingLoad = null;
          reject(new Error(`Native audio load timeout after ${LOAD_TIMEOUT_MS}ms`));
        }
      }, LOAD_TIMEOUT_MS);
      this._pendingLoad = { resolve, reject, timeout, requestId: loadRequestId };
    });

    // 3. Tauri uses one atomic setPlaylist+play command so queue replacement
    //    cannot race the following jump. WASM keeps its local two-step path.
    //    Load completion is driven by LoadAudio / LoadError events, not by
    //    invoke acks.
    //
    //    `jumpToSongAt` bundles the initial-position seek into the load,
    //    avoiding a separate `seekAudio` round-trip. The Rust side opens
    //    the source pre-seeked via `decoder::open_source_with_fft_at`, so
    //    no race with `SyncStatus` reads stale position=0 before the seek
    //    propagates.
    try {
      const song: SongData = {
        type: "local",
        filePath: this._path,
        origOrder: 0,
        display: this._display,
      };
      // `windowed: true` — a bare single-entry queue must stop at track end
      // instead of wrap-replaying itself; the real advance window arrives via
      // `applyNativeQueueWindow` once playback starts.
      if (loadRequestId !== undefined) {
        this._sendCommand({
          type: "setPlaylist",
          songs: [song],
          windowed: true,
          playIndex: 0,
          initialPosition: initPos,
          loadRequestId,
        });
      } else {
        this._sendCommand({ type: "setPlaylist", songs: [song], windowed: true });
        if (initPos > 0) {
          this._sendCommand({ type: "jumpToSongAt", songIndex: 0, position: initPos });
        } else {
          this._sendCommand({ type: "jumpToSong", songIndex: 0 });
        }
      }
    } catch (e) {
      const err = e instanceof Error ? e : new Error(String(e));
      this._clearPendingLoad();
      this._emit("loaderror", err);
      return;
    }

    // 4. Wait for LoadAudio / LoadError event (or timeout).
    try {
      await loadDone;
    } catch (e) {
      const err = e instanceof Error ? e : new Error(String(e));
      this._emit("loaderror", err);
      return;
    }

    if (this._destroyed || this._terminallyCleared) return;

    if (isTauri()) {
      await this.requestStatusSync(500);
    }
    if (this._destroyed || this._terminallyCleared) return;

    this._loaded = true;
    this._armNoFFTWarning();
    this._emit("load");
  }

  // ═════════════════════════════════════════════════════════════╗
  //  Transport: invoke control path                             ║
  // ═════════════════════════════════════════════════════════════╝

  private _sendCommand(msg: import("./protocol").AudioThreadMessage): boolean {
    const transport = this._transport ?? getAudioBackendTransport();
    this._transport = transport;
    const sentNow = transport.sendOrQueue(msg);
    if (!sentNow && IS_DEV) {
      console.warn("[NativeRustSound] audio control command queued until reconnect", msg.type);
    }
    return sentNow;
  }

  /**
   * Publish the full playback manifest to the backend planner. Unlike
   * `applyNativeQueueWindow` this carries stable identities rather than
   * pre-resolved URLs, so the backend can advance to any track in the list —
   * and re-resolve expired links — without a live JS runtime.
   */
  setNativeManifest(manifest: import("./protocol").NativePlaybackManifest): void {
    if (this._destroyed || this._terminallyCleared) return;
    this._sendCommand({ type: "setNativeManifest", manifest });
  }

  /** Drop the backend manifest, returning advancement to the JS-driven path. */
  clearNativeManifest(revision: number): void {
    if (this._destroyed || this._terminallyCleared) return;
    this._sendCommand({ type: "clearNativeManifest", revision });
  }

  /** Push resolver endpoints/credentials (login, logout, setting changes). */
  setNativeResolverConfig(config: import("./protocol").NativeResolverConfig): void {
    if (this._destroyed || this._terminallyCleared) return;
    this._sendCommand({ type: "setNativeResolverConfig", config });
  }

  /**
   * Gate planner-driven advancement without dropping the manifest. Used for
   * modes whose next track only the server knows (personal FM, listen
   * together).
   */
  setNativePlannerEnabled(enabled: boolean): void {
    if (this._destroyed || this._terminallyCleared) return;
    this._sendCommand({ type: "setNativePlannerEnabled", enabled });
  }

  /**
   * Replace the backend playback queue with a prefill window: the current
   * track plus pre-resolved next tracks, carrying real frontend playlist
   * indices as `origOrder`. Arms the native-advance adoption path when the
   * window gives the backend a usable next step — either further entries, or
   * wrap-repeat when `windowed` is false (repeat-one / single-song list).
   */
  applyNativeQueueWindow(songs: SongData[], options: { windowed: boolean }): boolean {
    if (this._destroyed || this._terminallyCleared || songs.length === 0) return false;
    this._sendCommand({ type: "setPlaylist", songs, windowed: options.windowed });
    this._nativeAdvanceWindowApplied = true;
    // A usable next step exists when the window has further entries, or when
    // wrap-repeat applies (repeat-one / single-song list, windowed=false).
    return songs.length > 1 || !options.windowed;
  }

  /**
   * Hand the listen-together keepalive to the backend, or take it back with
   * `null`. Returns `false` when there is no native backend to hand it to
   * (web), so the caller can keep running its own timer.
   */
  setListenTogetherRoom(roomId: string | null): boolean {
    if (!isTauri() || this._destroyed || this._terminallyCleared) return false;
    this._sendCommand({ type: "setListenTogetherRoom", roomId });
    return true;
  }

  /**
   * Permanently stop this controller and clear the backend queue.
   *
   * Unlike `stop()`, this also removes every queued track so a native backend
   * that outlives the WebView cannot resume or advance after the frontend queue
   * has been cleared. The controller is terminal after this call and must not
   * be reused for another load.
   */
  clearPlaybackQueue(): void {
    if (this._destroyed || this._terminallyCleared) return;

    this._terminallyCleared = true;
    this._pendingPlayCommand = false;
    this._clearNativeAdvanceFallback();
    this._nativeAdvancePending = false;
    this._nativeAdvanceWindowApplied = false;
    this._adoptNextBackendMusicId = false;
    this._nativeAutoMixSyncPending = false;
    this._backendTrackReady = false;
    this._loaded = false;

    // Let an in-flight load() unwind without emitting a late load event.
    this._resolvePendingLoad();
    this._resolvePendingSyncs();

    this._sendCommand({ type: "pauseAudio" });
    this._sendCommand({ type: "setPlaylist", songs: [], windowed: true });

    this._playbackState = "stopped";
    this._state = {
      musicId: "",
      position: 0,
      duration: 0,
      isPlaying: false,
      volume: this._volume,
      playlist: [],
      currentPlayIndex: 0,
    };
    this._musicInfo = null;
    this._quality = null;
    this._timeline.reset(0, 0);
    this._clearFFTState();
    this._lowFreqVolume = 0;
  }

  setAnalysisEnabled(enabled: boolean): void {
    if (this._destroyed || this._analysisEnabled === enabled) return;
    this._analysisEnabled = enabled;
    if (!enabled) {
      this._clearFFTState();
      this._lowFreqVolume = 0;
    }
    this._sendCommand({ type: "setAnalysis", enabled });
  }

  setFFTEnabled(enabled: boolean): void {
    if (this._destroyed || this._fftEventsEnabled === enabled) return;
    this._fftEventsEnabled = enabled;
    if (!enabled) {
      this._clearFFTState();
    } else if (this._loaded && this._playbackState === "playing") {
      this._armNoFFTWarning();
    }
    this._sendCommand({ type: "setFFT", enabled });
  }

  private _clearFFTState(): void {
    this._fftData = [];
    if (this._frequencyData.length !== 0) {
      this._frequencyData = new Uint8Array(0);
    }
    this._averageAmplitude = 0;
    this._clearNoFFTWarning();
  }

  private _clearNoFFTWarning(): void {
    if (this._noFFTWarnTimer !== null) {
      clearTimeout(this._noFFTWarnTimer);
      this._noFFTWarnTimer = null;
    }
  }

  // ═════════════════════════════════════════════════════════════╗
  //  Event routing                                              ║
  // ═════════════════════════════════════════════════════════════╝

  private _handleEvent(evt: AudioThreadEvent, seq?: number): void {
    if (this._destroyed || this._terminallyCleared) return;

    if (seq !== undefined && seq > 0 && this._markSeqSeen(seq)) return;

    switch (evt.type) {
      case "syncStatus": {
        const d = evt.data;
        const expectedBefore = this._expectedMusicId;
        const expectingNativeAutoMixAdoption =
          this._adoptNextBackendMusicId || this._nativeAutoMixSyncPending;
        const allowInitialTauriAttach = isTauri() && this._allowInitialBackendAttach;
        // Initial attach is only safe when the surviving backend is on the
        // track this controller was built for. Silently adopting an unrelated
        // backend track would pair the new store metadata with stale audio from
        // the previous WebView session.
        if (allowInitialTauriAttach && !this._acceptInitialAttach(d.musicId, d.identity)) {
          this._resolvePendingSyncs();
          return;
        }
        if (!this._acceptMusicId(d.musicId, expectingNativeAutoMixAdoption)) {
          if (this._allowInitialBackendAttach) {
            this._resolvePendingSyncs();
          }
          return;
        }
        const adoptedBackendTrack = !!d.musicId && d.musicId !== expectedBefore;
        // The backend's own answer to "is the clock you hold still the right
        // one". Identity and `musicId` say *which song*, which is a different
        // question and one they each get wrong on some path: `musicId` is
        // `local:<url>` and changes on a re-resolve of the same track, and
        // identity is absent whenever the backend was not told one.
        const timelineChanged = this._isNewTimeline(d.timelineEpoch);
        this._recordTimelineEpoch(d.timelineEpoch);
        const backendTrackChanged =
          timelineChanged || adoptedBackendTrack || this._isBackendTrackChange(d.identity);
        const pendingNativeAutoMixIndex = this._state.currentPlayIndex;
        const acceptedPosition = backendTrackChanged
          ? this._beginTrackTimeline(d.position, d.duration)
          : this._acceptIncomingPosition(this._coerceIncomingPosition(d.position));
        this._state = {
          musicId: d.musicId,
          position: acceptedPosition,
          duration: d.duration,
          isPlaying: d.isPlaying,
          volume: d.volume,
          playlist: d.playlist,
          currentPlayIndex: d.currentPlayIndex,
        };
        this._musicInfo = d.musicInfo
          ? { ...d.musicInfo, position: acceptedPosition }
          : d.musicInfo;
        this._quality = d.quality;
        this._backendIdentity = d.identity ?? null;
        this._timeline.setDuration(d.duration);
        this._backendTrackReady = true;
        if (isTauri() && this._allowInitialBackendAttach && this._playbackState === "stopped") {
          this._playbackState = d.isPlaying ? "playing" : "paused";
          this._syncTimelineClock();
        }
        this._resolvePendingSyncs();
        const shouldNotifyNativeAutoMixSync =
          // A boot attach already reconciled the store from the session
          // snapshot before this controller existed; re-announcing it as a
          // backend-initiated transition would re-enter the adoption path.
          !allowInitialTauriAttach &&
          (this._nativeAutoMixSyncPending
            ? adoptedBackendTrack || d.currentPlayIndex === pendingNativeAutoMixIndex
            : (expectingNativeAutoMixAdoption || this._isActiveController()) &&
              adoptedBackendTrack);
        if (shouldNotifyNativeAutoMixSync) {
          this._nativeAutoMixSyncPending = false;
          window.dispatchEvent(
            new CustomEvent(NATIVE_AUTOMIX_SYNC_EVENT, {
              detail: {
                currentIndex: d.currentPlayIndex,
                musicId: d.musicId,
                identity: this._backendIdentity,
                position: acceptedPosition,
                duration: d.duration,
              },
            }),
          );
        }
        // Outside the Tauri initial re-attach path above, do not update
        // `_playbackState` from syncStatus. State transitions belong to
        // `PlayStatus` events only. SyncStatus can be emitted while a follow-on
        // ResumeAudio is queued; applying it generally would revert optimistic
        // playback and cause duplicate play notifications.
        break;
      }

      case "loadAudio": {
        if (!this._matchesPendingLoadRequest(evt.data.loadRequestId)) break;
        const wasAdvancePending = this._nativeAdvancePending;
        // A load this frontend never asked for (no `loadRequestId`) is the
        // backend starting a track by itself: a planner advance driven by the
        // OS media-session buttons, a natural end, a native AutoMix hand-off,
        // or a repeat-one restart. It has just anchored its own clock at
        // `musicInfo.position`, and that anchor is the authority — this side
        // only extrapolates between anchors, so it adopts rather than
        // reconciles. Running it through the intra-track reconciler instead is
        // what made the frontend read `old elapsed + new elapsed`: a fresh
        // track's 0 looks exactly like a stale pre-seek packet.
        const requestedByFrontend =
          evt.data.loadRequestId !== undefined && evt.data.loadRequestId !== null;
        const backendInitiated = isTauri() && !requestedByFrontend && this._isActiveController();
        // The load starts a new timeline, and the backend says so outright.
        // Everything below is the fallback for a backend that did not stamp
        // one: its own arming event (`NativePlannerAdvanced`) is emitted only
        // *after* this one and after the `SyncStatus` behind it, so waiting for
        // the flag dropped both authoritative snapshots of the new track and
        // left the next periodic audit sync — up to two seconds later — to
        // reconcile a track that was already playing.
        const timelineChanged = this._isNewTimeline(evt.data.timelineEpoch);
        const trackChanged =
          backendInitiated &&
          (timelineChanged ||
            this._isBackendTrackChange(evt.data.identity) ||
            (!!evt.data.musicId && evt.data.musicId !== this._expectedMusicId));
        if (trackChanged) this._adoptNextBackendMusicId = true;
        if (this._acceptMusicId(evt.data.musicId)) {
          this._recordTimelineEpoch(evt.data.timelineEpoch);
          this._musicInfo = evt.data.musicInfo;
          this._quality = evt.data.quality;
          this._backendIdentity = evt.data.identity ?? null;
          this._state.duration = evt.data.musicInfo.duration;
          this._timeline.setDuration(evt.data.musicInfo.duration);
          this._backendTrackReady = true;
          this._resolvePendingLoad();
          if (wasAdvancePending || trackChanged) {
            this._completeNativeAdvanceAdoption(
              evt.data.musicId,
              evt.data.currentPlayIndex,
              evt.data.musicInfo.duration,
              evt.data.musicInfo.position,
            );
          } else if (backendInitiated) {
            // Same track, and the backend restarted it (repeat-one) or reopened
            // the decoder behind it (output-device rebuild). No store adoption
            // is due, but the clock still has to follow the new anchor.
            this._beginTrackTimeline(evt.data.musicInfo.position, evt.data.musicInfo.duration);
          }
        }
        break;
      }

      case "loadingAudio":
        if (!this._matchesPendingLoadRequest(evt.data.loadRequestId)) break;
        if (this._nativeAdvancePending) {
          // Backend confirmed the advance and is resolving the next source —
          // give the download room before falling back to the JS path.
          this._armNativeAdvanceFallback(NATIVE_ADVANCE_LOAD_TIMEOUT_MS);
        }
        break;

      case "playPosition": {
        if (!this._backendTrackReady) break;
        // Authoritative, but only about the timeline it names. One that is not
        // ours belongs to a track the load/status behind it will anchor us to.
        if (this._isForeignTimeline(evt.data.timelineEpoch)) break;
        this._acceptIncomingPosition(this._coerceIncomingPosition(evt.data.position));
        break;
      }

      case "playStatus": {
        if (!this._backendTrackReady) break;
        const wantPlaying = evt.data.isPlaying;
        this._pendingPlayCommand = false;
        this._state.isPlaying = wantPlaying;
        const isCurrentlyPlaying = this._playbackState === "playing";
        if (wantPlaying === isCurrentlyPlaying) {
          this._syncTimelineClock();
          // Already in this state (likely from an optimistic flip) —
          // don't re-emit; consumers would see duplicate play/pause.
          break;
        }
        if (wantPlaying) {
          this._playbackState = "playing";
          this._syncTimelineClock();
          this._emit("play");
        } else {
          this._playbackState = "paused";
          this._syncTimelineClock();
          this._emit("pause");
        }
        break;
      }

      case "audioPlayFinished": {
        this._pendingPlayCommand = false;
        if (evt.data.musicId === this._expectedMusicId) {
          if (
            isTauri() &&
            (this._nativePlannerActive || this._nativeAdvanceWindowApplied) &&
            this._isActiveController()
          ) {
            // The backend advances on its own — even while this JS runtime is
            // frozen in the background. With a manifest published the planner
            // can reach any track and re-resolve expired URLs; without one the
            // bounded queue window covers the next hop. Either way, suppress
            // the legacy 'end' → setPlaySongIndex teardown and adopt the
            // backend transition. The fallback timer re-emits 'end' if no
            // advance lands (exhausted, expired URL, load failure).
            this._beginNativeAdvanceAdoption();
            break;
          }
          this._playbackState = "ended";
          this._setLocalPosition(this._state.duration);
          this._emit("end");
        } else if (
          this._isActiveController() &&
          (this._nativeAdvancePending ||
            this._adoptNextBackendMusicId ||
            this._nativeAutoMixSyncPending)
        ) {
          this._adoptNextBackendMusicId = true;
          this._nativeAutoMixSyncPending = true;
          void this.requestStatusSync().then(() => {
            if (this._destroyed) return;
            this._playbackState = "ended";
            this._setLocalPosition(this._state.duration);
            this._emit("end");
          });
        }
        break;
      }

      case "volumeChanged": {
        this._state.volume = evt.data.volume;
        break;
      }

      case "nativePlannerStatusChanged": {
        const status = evt.data;
        this._nativePlannerStatus = status.status;
        const planner = status.status;
        // The planner can drive advancement only when it holds a live manifest,
        // is allowed to pick the next track, and has not given up on it.
        // `enabled` matters because server-driven modes (personal FM, listen
        // together) publish a manifest but gate advancement off — treating that
        // as "active" would suppress the JS-driven `end` transition and stall
        // every track change behind the adoption fallback timer.
        this._nativePlannerActive =
          planner.manifestRevision > 0 && planner.enabled !== false && !planner.exhausted;
        // Keep our revision counter ahead of the backend's. Without this a
        // WebView reload (module state resets, backend does not) would leave
        // every subsequent publish rejected as stale.
        plannerRevisionObserver?.(planner.manifestRevision);
        break;
      }

      case "nativePlannerAdvanced": {
        // The backend picked and started the next track by itself. Adoption
        // still runs through the existing loadAudio/syncStatus path — this
        // event carries the stable identity that path cannot infer from a
        // re-resolved CDN URL, so retain it: the `local:<url>` prefill
        // registry can never match a URL the Rust resolver produced, which
        // would otherwise leave the reported index as the only (unverified)
        // reconciliation key.
        this._adoptNextBackendMusicId = true;
        this._nativeAutoMixSyncPending = true;
        this._state.currentPlayIndex = evt.data.playlistIndex;
        this._lastPlannerAdvance = {
          identity: evt.data.identity,
          playlistIndex: evt.data.playlistIndex,
        };
        break;
      }

      case "nativePlannerExhausted": {
        // Every candidate failed. Stop suppressing 'end' so the JS-driven path
        // takes over and the user sees a real stop rather than silence.
        this._nativePlannerActive = false;
        if (IS_DEV) {
          console.warn(
            `[NativeRustSound] planner exhausted after ${evt.data.attempted} attempts: ${evt.data.reason}`,
          );
        }
        break;
      }

      case "automixCrossfadeComplete": {
        this._adoptNextBackendMusicId = true;
        this._nativeAutoMixSyncPending = true;
        this._state.currentPlayIndex = evt.data.currentIndex;
        if (evt.data.musicId) {
          if (this._acceptMusicId(evt.data.musicId)) {
            this._state.musicId = evt.data.musicId;
          }
          this._adoptNextBackendMusicId = false;
        }
        if (typeof evt.data.duration === "number" && evt.data.duration > 0) {
          this._state.duration = evt.data.duration;
          this._timeline.setDuration(evt.data.duration);
          if (this._musicInfo) {
            this._musicInfo = { ...this._musicInfo, duration: evt.data.duration };
          }
        }
        if (typeof evt.data.position === "number" && evt.data.position >= 0) {
          this._setLocalPosition(evt.data.position);
        }
        this._sendCommand({ type: "syncStatus" });
        window.dispatchEvent(
          new CustomEvent(NATIVE_AUTOMIX_COMPLETE_EVENT, {
            detail: {
              currentIndex: evt.data.currentIndex,
              musicId: evt.data.musicId,
              position: evt.data.position,
              duration: evt.data.duration,
              transitionId: evt.data.transitionId,
            },
          }),
        );
        break;
      }

      case "automixCrossfadeStarted":
        this._adoptNextBackendMusicId = true;
        break;

      case "automixError":
        this._adoptNextBackendMusicId = false;
        this._nativeAutoMixSyncPending = false;
        break;

      case "fftData": {
        this._fftData = evt.data.data;
        let sum = 0;
        for (let i = 0; i < this._fftData.length; i++) {
          sum += this._fftData[i];
        }
        this._averageAmplitude = this._fftData.length > 0 ? sum / this._fftData.length : 0;
        this._fftReceived = true;
        this._clearNoFFTWarning();
        if (IS_DEV) {
          const now = Date.now();
          if (now - this._lastFFTLogAt > FFT_LOG_INTERVAL_MS) {
            this._lastFFTLogAt = now;
            const first = this._fftData[0];
            console.debug(
              `[FFT recv] len=${this._fftData.length} first=${first !== undefined ? first.toFixed(4) : "?"}`,
            );
          }
        }
        break;
      }

      case "lowFrequencyVolume": {
        this._lowFreqVolume = evt.data.volume;
        break;
      }

      case "audioOutputChanged":
        break;

      case "audioOutputError":
        console.warn("[NativeRustSound] audio output error:", evt.data.error);
        break;

      case "seekCommitted":
        this._handleSeekCommitted(evt.data.requestId, evt.data.position);
        break;

      case "seekFailed":
        this._handleSeekFailed(evt.data.requestId, evt.data.position, evt.data.error);
        break;

      case "playError":
      case "loadError": {
        const err = new Error(evt.data.error);
        if (evt.type === "loadError") {
          if (!this._matchesPendingLoadRequest(evt.data.loadRequestId)) break;
          this._pendingPlayCommand = false;
          if (this._nativeAdvancePending) {
            // The prefilled next source failed to load (e.g. expired URL) —
            // hand the transition back to the JS-driven path.
            this._abandonNativeAdvanceAdoption();
            break;
          }
          if (this._pendingLoad) {
            this._rejectPendingLoad(err);
          } else {
            this._emit("loaderror", err);
          }
        } else {
          this._pendingPlayCommand = false;
          const wasPlaying = this._playbackState === "playing";
          this._playbackState = "paused";
          this._state.isPlaying = false;
          if (wasPlaying) {
            this._emit("pause");
          }
          this._emit("playerror", err);
        }
        break;
      }

      case "playListChanged":
      case "loadProgress":
      case "automixStatus":
      case "automixAnalysisReady":
      // The OS media session is driven from Rust off this event; nothing for
      // this controller to do with it.
      case "nowPlayingChanged":
        break;
    }
  }

  private _markSeqSeen(seq: number): boolean {
    if (this._seenEventSeq.has(seq)) return true;

    this._seenEventSeq.add(seq);
    this._seenEventSeqOrder.push(seq);
    while (this._seenEventSeqOrder.length - this._seenEventSeqHead > SEEN_EVENT_SEQ_LIMIT) {
      const oldSeq = this._seenEventSeqOrder[this._seenEventSeqHead];
      this._seenEventSeqHead++;
      if (oldSeq !== undefined) this._seenEventSeq.delete(oldSeq);
    }
    // Drop the consumed prefix once it dominates the array so the backing
    // storage stays bounded at ~2× the dedup window.
    if (this._seenEventSeqHead >= SEEN_EVENT_SEQ_LIMIT) {
      this._seenEventSeqOrder = this._seenEventSeqOrder.slice(this._seenEventSeqHead);
      this._seenEventSeqHead = 0;
    }
    return false;
  }

  private _coerceIncomingPosition(position: number): number {
    if (!Number.isFinite(position) || position <= 0) return 0;
    const duration = this.duration();
    return duration > 0 ? Math.min(position, duration) : position;
  }

  private _normalizeSeekPosition(position: number): number {
    if (!Number.isFinite(position) || position <= 0) return 0;
    const duration = this.duration();
    return duration > 0 ? Math.min(position, duration) : position;
  }

  private _newSeekRequestId(): number {
    this._nextSeekRequestId = (this._nextSeekRequestId + 1) % SEEK_REQUEST_ID_MODULO;
    return Date.now() * SEEK_REQUEST_ID_MODULO + this._nextSeekRequestId;
  }

  private _newLoadRequestId(): number {
    this._nextLoadRequestId = (this._nextLoadRequestId + 1) % SEEK_REQUEST_ID_MODULO;
    return Date.now() * SEEK_REQUEST_ID_MODULO + this._nextLoadRequestId;
  }

  private _matchesPendingLoadRequest(requestId: number | null | undefined): boolean {
    const pendingRequestId = this._pendingLoad?.requestId;
    if (this._pendingLoad) {
      return pendingRequestId === undefined
        ? requestId === undefined || requestId === null
        : requestId === pendingRequestId;
    }
    return requestId === undefined || requestId === null;
  }

  private _syncTimelineClock(): void {
    this._timeline.setDuration(this.duration());
    this._timeline.setPlaybackState(this._playbackState === "playing", this._pendingPlayCommand);
  }

  private _applyTimelinePosition(position: number): number {
    const nextPosition = this._normalizeSeekPosition(position);
    this._state.position = nextPosition;
    if (this._musicInfo) {
      this._musicInfo = { ...this._musicInfo, position: nextPosition };
    }
    return nextPosition;
  }

  private _setLocalPosition(position: number, guardSeek = false, requestId?: number): number {
    this._syncTimelineClock();
    const nextPosition = this._timeline.setLocalPosition(position, {
      guardSeek,
      requestId,
      atomicSeek: requestId !== undefined && isTauri(),
    });
    return this._applyTimelinePosition(nextPosition);
  }

  private _acceptIncomingPosition(position: number): number {
    this._syncTimelineClock();
    return this._applyTimelinePosition(this._timeline.acceptIncomingPosition(position));
  }

  /**
   * Hard-anchor the local clock on a track the backend has just loaded.
   *
   * The backend owns the timeline; this side extrapolates between its anchors.
   * A track boundary invalidates every reconciliation guard the timeline holds
   * (see `AudioTimelineSync.beginTrack`), so the reported position is taken
   * verbatim instead of being weighed against the position the *previous* track
   * had reached.
   */
  private _beginTrackTimeline(position: number, duration: number): number {
    const nextDuration = duration > 0 ? duration : this.duration();
    if (nextDuration > 0) this._state.duration = nextDuration;
    this._syncTimelineClock();
    // A zero is the absence of an answer rather than one — a republished-empty
    // `musicInfo` must not blank the total the timeline already holds, or the UI
    // shows 0:00 while the position keeps ticking.
    const anchored =
      nextDuration > 0
        ? this._timeline.beginTrack(position, nextDuration)
        : this._timeline.beginTrack(position);
    return this._applyTimelinePosition(anchored);
  }

  /**
   * Whether `identity` names a track other than the one this controller is
   * anchored to.
   *
   * This is the authoritative signal for a backend-initiated track change and
   * needs nothing armed in advance, which is the point: the backend arms the
   * frontend's adoption flags *after* it has already published the new track's
   * load and status. A `null` on either side is not an answer — the id-based
   * comparison covers those.
   */
  private _isBackendTrackChange(identity: TrackIdentity | null | undefined): boolean {
    if (!identity || !this._backendIdentity) return false;
    return !sameTrackIdentity(identity, this._backendIdentity);
  }

  /**
   * Whether `epoch` retires the timeline this controller is anchored to.
   *
   * Pure — recording is [`_recordTimelineEpoch`], and the two are separate
   * because an event may still be rejected after this is asked (a controller
   * that is no longer driving playback, a load that is not ours). Adopting the
   * epoch of an event we then ignore would make us accept that track's position
   * packets while still holding the previous track's clock.
   *
   * A first observation is not a change: a controller that has just attached
   * has no timeline of its own to retire.
   */
  private _isNewTimeline(epoch: number | undefined): boolean {
    if (typeof epoch !== "number" || !Number.isFinite(epoch)) return false;
    return this._backendTimelineEpoch !== null && this._backendTimelineEpoch !== epoch;
  }

  private _recordTimelineEpoch(epoch: number | undefined): void {
    if (typeof epoch !== "number" || !Number.isFinite(epoch)) return;
    this._backendTimelineEpoch = epoch;
  }

  /**
   * Whether a stamped position packet describes a timeline we have not adopted.
   *
   * Dropping is the only correct answer: the packet is authoritative about a
   * track this controller knows nothing about yet, and the `loadAudio` /
   * `syncStatus` carrying the same epoch — which does describe it — is already
   * behind it in the stream.
   */
  private _isForeignTimeline(epoch: number | undefined): boolean {
    return this._isNewTimeline(epoch);
  }

  private _handleSeekCommitted(requestId: number | null | undefined, position: number): void {
    this._syncTimelineClock();
    const nextPosition = this._timeline.commitSeek(requestId, position);
    if (nextPosition === null) return;
    this._applyTimelinePosition(nextPosition);
  }

  private _handleSeekFailed(
    requestId: number | null | undefined,
    position: number,
    error: string,
  ): void {
    this._syncTimelineClock();
    if (!this._timeline.rejectSeek(requestId)) return;
    if (IS_DEV) {
      console.warn("[NativeRustSound] native seek failed", { requestId, position, error });
    }
    void this.requestStatusSync(500);
  }

  private _issueSeek(position: number): boolean {
    const requestId = isTauri() ? this._newSeekRequestId() : undefined;
    const msg: import("./protocol").AudioThreadMessage =
      requestId !== undefined
        ? { type: "seekAudio", position, requestId, expectedMusicId: this._expectedMusicId }
        : { type: "seekAudio", position, expectedMusicId: this._expectedMusicId };
    const sentNow = this._sendCommand(msg);
    this._setLocalPosition(position, true, requestId);
    return sentNow;
  }

  private _isActiveController(): boolean {
    return window.$player === this;
  }

  isDestroyed(): boolean {
    return this._destroyed;
  }

  /**
   * Decide whether a `syncStatus` describes the track this controller should
   * attach to at startup.
   *
   * With an `attachIdentity` the match is on stable identity and the backend's
   * `musicId` is adopted wholesale — that is the only comparison that survives
   * a WebView reload, because the frontend re-resolves a different CDN URL
   * every session. Without one, fall back to the exact-source check.
   */
  private _acceptInitialAttach(musicId: string, identity?: TrackIdentity | null): boolean {
    if (this._attachIdentity) {
      if (!sameTrackIdentity(identity, this._attachIdentity)) return false;
      this._adoptMusicId(musicId);
      return true;
    }
    return musicId === this._expectedMusicId;
  }

  private _acceptMusicId(musicId: string, allowBackendAdoption = false): boolean {
    if (!musicId || musicId === this._expectedMusicId) return true;
    if (!this._adoptNextBackendMusicId && !allowBackendAdoption) return false;

    this._adoptMusicId(musicId);
    return true;
  }

  private _adoptMusicId(musicId: string): void {
    if (!musicId) return;
    this._expectedMusicId = musicId;
    this._adoptNextBackendMusicId = false;
    if (musicId.startsWith("local:")) {
      this._path = musicId.slice("local:".length);
    }
  }

  // ── Native queue-window advance adoption ──────────────────────

  /**
   * Whether the backend planner currently owns advancement. Mirrors
   * `NativePlannerStatus.active` and is the wider replacement for
   * `_nativeAdvanceWindowApplied`: the planner covers the whole list, so it
   * stays true across every hop instead of needing a re-arm per window.
   */
  isNativePlannerActive(): boolean {
    return this._nativePlannerActive;
  }

  /** Last planner status the backend reported, or null before the first event. */
  getNativePlannerStatus(): NativePlannerStatus | null {
    return this._nativePlannerStatus;
  }

  /**
   * Take the pending planner-advance identity, clearing it.
   *
   * Consuming rather than peeking is deliberate: a retained identity must not
   * be reused to reconcile a *later* transition that the planner did not
   * drive (manual selection, AutoMix, JS-driven end), which would reintroduce
   * the same displayed-vs-playing mismatch in the opposite direction.
   */
  takePlannerAdvance(): { identity: TrackIdentity; playlistIndex: number } | null {
    const advance = this._lastPlannerAdvance;
    this._lastPlannerAdvance = null;
    return advance;
  }

  private _beginNativeAdvanceAdoption(): void {
    this._nativeAdvancePending = true;
    this._adoptNextBackendMusicId = true;
    this._armNativeAdvanceFallback(NATIVE_ADVANCE_START_TIMEOUT_MS);
  }

  private _completeNativeAdvanceAdoption(
    musicId: string,
    currentPlayIndex: number,
    duration: number,
    position = 0,
  ): void {
    this._nativeAdvancePending = false;
    // Repeat-one adoption keeps the same musicId, so `_acceptMusicId`'s
    // early-return never consumed the adopt flag — clear it explicitly.
    this._adoptNextBackendMusicId = false;
    this._clearNativeAdvanceFallback();
    this._state.musicId = musicId || this._expectedMusicId;
    this._state.currentPlayIndex = currentPlayIndex;
    if (duration > 0) {
      this._state.duration = duration;
      this._timeline.setDuration(duration);
    }
    // The backend's own anchor for the adopted track, not a blind zero: a
    // planner advance starts at 0, but the same funnel serves hand-offs that
    // start elsewhere, and the store is seeded from what this reports.
    const startPosition = this._beginTrackTimeline(position, duration);
    window.dispatchEvent(
      new CustomEvent(NATIVE_AUTOMIX_SYNC_EVENT, {
        detail: {
          currentIndex: currentPlayIndex,
          musicId: this._state.musicId,
          identity: this._backendIdentity,
          position: startPosition,
          duration: this.duration(),
        },
      }),
    );
  }

  private _abandonNativeAdvanceAdoption(): void {
    if (!this._nativeAdvancePending) return;
    this._nativeAdvancePending = false;
    this._adoptNextBackendMusicId = false;
    this._clearNativeAdvanceFallback();
    this._playbackState = "ended";
    this._setLocalPosition(this._state.duration);
    this._emit("end");
  }

  private _armNativeAdvanceFallback(timeoutMs: number): void {
    this._clearNativeAdvanceFallback();
    this._nativeAdvanceFallbackTimer = setTimeout(() => {
      this._nativeAdvanceFallbackTimer = null;
      this._abandonNativeAdvanceAdoption();
    }, timeoutMs);
  }

  private _clearNativeAdvanceFallback(): void {
    if (this._nativeAdvanceFallbackTimer !== null) {
      clearTimeout(this._nativeAdvanceFallbackTimer);
      this._nativeAdvanceFallbackTimer = null;
    }
  }

  private _resolvePendingLoad(): void {
    if (!this._pendingLoad) return;
    clearTimeout(this._pendingLoad.timeout);
    this._pendingLoad.resolve();
    this._pendingLoad = null;
  }

  private _rejectPendingLoad(err: Error): void {
    if (!this._pendingLoad) return;
    clearTimeout(this._pendingLoad.timeout);
    this._pendingLoad.reject(err);
    this._pendingLoad = null;
  }

  private _clearPendingLoad(): void {
    if (!this._pendingLoad) return;
    clearTimeout(this._pendingLoad.timeout);
    this._pendingLoad = null;
  }

  private _resolvePendingSyncs(): void {
    if (this._pendingSyncs.length === 0) return;
    const pending = this._pendingSyncs.splice(0);
    for (const sync of pending) {
      clearTimeout(sync.timeout);
      sync.resolve();
    }
  }

  private _armNoFFTWarning(): void {
    if (!IS_DEV) return;
    if (this._analysisEnabled === false || this._fftEventsEnabled === false) return;
    // Web/WASM does not use the native Channel/CPAL FFT event stream. It may
    // add analysis later through a WASM decoder/PCM path, but absence of
    // `fftData` there is not a transport/backend failure.
    if (!isTauri()) return;
    this._clearNoFFTWarning();
    this._noFFTWarnTimer = setTimeout(() => {
      this._noFFTWarnTimer = null;
      if (!this._fftReceived && this._playbackState === "playing") {
        console.warn(
          `[NativeRustSound] No fftData event received within ${NO_FFT_WARN_MS}ms of load. ` +
            `Check Rust logs for "FFT broadcast: ... empty ticks" or the event Channel state.`,
        );
      }
    }, NO_FFT_WARN_MS);
  }

  // ═════════════════════════════════════════════════════════════╗
  //  ISound interface                                           ║
  // ═════════════════════════════════════════════════════════════╝

  playing(): boolean {
    return this._playbackState === "playing" || this._pendingPlayCommand;
  }

  play(): this {
    if (this._destroyed || this._terminallyCleared) return this;
    if (this._playbackState === "playing" || this._pendingPlayCommand) return this;
    this._pendingPlayCommand = true;
    this._sendCommand({ type: "resumeAudio" });
    if (this._optimisticPlayback && this._playbackState !== "playing") {
      this._playbackState = "playing";
      this._state.isPlaying = true;
      this._syncTimelineClock();
      // Defer via microtask so `sound.play(); sound.once("play", ...)` —
      // the pattern PlayerFunctions.fadePlayOrPause uses — has time to
      // register the listener before the event fires. Without this the
      // fade-in callback never runs and volume stays at 0.
      this._emitDeferred("play");
    }
    return this;
  }

  pause(): this {
    if (this._destroyed || this._terminallyCleared) return this;
    this._pendingPlayCommand = false;
    this._sendCommand({ type: "pauseAudio" });
    if (this._optimisticPlayback && this._playbackState !== "paused") {
      this._playbackState = "paused";
      this._state.isPlaying = false;
      this._syncTimelineClock();
      this._emitDeferred("pause");
    }
    return this;
  }

  stop(): this {
    if (this._destroyed || this._terminallyCleared) return this;
    this._pendingPlayCommand = false;
    this._sendCommand({ type: "pauseAudio" });
    this._issueSeek(0);
    this._state.isPlaying = false;
    if (this._optimisticPlayback && this._playbackState !== "stopped") {
      this._playbackState = "stopped";
      this._syncTimelineClock();
      this._emitDeferred("pause");
    }
    return this;
  }

  seek(pos?: number): number | this {
    if (pos !== undefined) {
      if (!this._destroyed && !this._terminallyCleared) {
        const position = this._normalizeSeekPosition(pos);
        const sentNow = this._issueSeek(position);
        if (isTauri() && sentNow) {
          window.setTimeout(() => {
            void this.requestStatusSync(300);
          }, 0);
        }
      }
      return this;
    }
    this._syncTimelineClock();
    const position = this._timeline.readPosition();
    this._state.position = position;
    return position;
  }

  duration(): number {
    // `??` would let a *zero* duration win, and zero is the absence of an
    // answer rather than one: `musicInfo` arrives empty whenever the backend
    // republishes between tracks, and a 0 there shadowed a perfectly good
    // `_state.duration` — the UI then showed 0:00 total while the position
    // kept ticking, because the timeline extrapolates independently.
    const reported = this._musicInfo?.duration;
    return typeof reported === "number" && reported > 0 ? reported : this._state.duration;
  }

  volume(vol?: number): number | this {
    if (vol !== undefined) {
      this._volume = Math.max(0, Math.min(1, vol));
      if (!this._muted && !this._destroyed) {
        this._sendCommand({ type: "setVolume", volume: this._volume });
      }
      return this;
    }
    return this._volume;
  }

  fade(_from: number, to: number, _duration: number): this {
    // The native backend doesn't expose a sample-accurate ramp yet — set
    // the target volume immediately. Emit "fade" via microtask so the
    // `sound.fade(...); sound.once("fade", () => sound.pause())` pattern
    // (PlayerFunctions.fadePlayOrPause) has time to register the listener
    // before the event fires.
    this.volume(to);
    this._emitDeferred("fade");
    return this;
  }

  // ── Events ────────────────────────────────────────────────────

  on(event: SoundEventType, callback: SoundEventCallback): this {
    (this._events[event] ??= []).push(callback);
    return this;
  }

  once(event: SoundEventType, callback: SoundEventCallback): this {
    (this._onceEvents[event] ??= []).push(callback);
    return this;
  }

  off(event: SoundEventType, callback?: SoundEventCallback): this {
    if (!callback) {
      delete this._events[event];
      delete this._onceEvents[event];
    } else {
      this._events[event] = (this._events[event] ?? []).filter((cb) => cb !== callback);
      this._onceEvents[event] = (this._onceEvents[event] ?? []).filter((cb) => cb !== callback);
    }
    return this;
  }

  private _emit(event: SoundEventType, ...args: unknown[]): void {
    for (const cb of this._events[event] ?? []) {
      try {
        cb(...args);
      } catch {
        /* ignore */
      }
    }
    const once = this._onceEvents[event] ?? [];
    delete this._onceEvents[event];
    for (const cb of once) {
      try {
        cb(...args);
      } catch {
        /* ignore */
      }
    }
  }

  /**
   * Schedule an emit for the next microtask. Used by `play`/`pause`/
   * `stop`/`fade` because their callers (e.g. fadePlayOrPause) register
   * `once()` listeners IMMEDIATELY AFTER calling the method:
   *   sound.play();
   *   sound.once("play", () => sound.fade(0, vol, 300));
   * If we emit synchronously inside `play()`, the once() handler hasn't
   * been registered yet and is silently dropped — leaving volume at 0
   * after a pause→play cycle. Microtask defers just enough.
   */
  private _emitDeferred(event: SoundEventType, ...args: unknown[]): void {
    queueMicrotask(() => {
      if (this._destroyed) return;
      this._emit(event, ...args);
    });
  }

  // ── FFT / spectrum accessors ──────────────────────────────────

  getFrequencyData(): Uint8Array<ArrayBuffer> {
    const raw = this._fftData;
    if (raw.length === 0) {
      if (this._frequencyData.length !== 0) {
        this._frequencyData = new Uint8Array(0);
      }
      return this._frequencyData;
    }
    if (this._frequencyData.length !== raw.length) {
      this._frequencyData = new Uint8Array(raw.length);
    }
    let max = 0;
    for (const v of raw) if (v > max) max = v;
    if (max <= 0) {
      this._frequencyData.fill(0);
      return this._frequencyData;
    }
    for (let i = 0; i < raw.length; i++) {
      this._frequencyData[i] = Math.min(255, Math.round((raw[i] / max) * 255));
    }
    return this._frequencyData;
  }

  getFFTData(): number[] {
    return this._fftData;
  }

  getLowFrequencyVolume(): number {
    // Rust computes this from raw FFT bins; the frontend only reads the latest
    // value and does not re-run FFT-derived lowfreq work per RAF.
    return this._lowFreqVolume;
  }

  getAverageAmplitude(): number {
    return this._averageAmplitude;
  }

  getAudioQuality(): AudioQuality | null {
    return this._quality ? { ...this._quality } : null;
  }

  getSourceUrl(): string {
    return this._path;
  }

  requestStatusSync(timeoutMs = 500): Promise<void> {
    if (this._destroyed || this._terminallyCleared) return Promise.resolve();

    return new Promise((resolve) => {
      const timeout = setTimeout(() => {
        this._pendingSyncs = this._pendingSyncs.filter((sync) => sync.resolve !== resolve);
        resolve();
      }, timeoutMs);
      this._pendingSyncs.push({ resolve, timeout });
      this._sendCommand({ type: "syncStatus" });
    });
  }

  getGainNode(): GainNode | null {
    return this._transport?.getGainNode?.() ?? null;
  }

  async ensureAudioGraph(): Promise<boolean> {
    return (await this._transport?.ensureAudioGraph?.()) ?? false;
  }

  getEffectManager(): import("../../AudioContext/AudioEffectManager").AudioEffectManager | null {
    return null;
  }

  // ── Cleanup ───────────────────────────────────────────────────

  unload(): void {
    if (this._destroyed) return;
    const wasActiveController = this._isActiveController();
    this._destroyed = true;
    this._clearPendingLoad();
    for (const sync of this._pendingSyncs.splice(0)) {
      clearTimeout(sync.timeout);
      sync.resolve();
    }
    this._clearNativeAdvanceFallback();
    this._nativeAdvancePending = false;
    this._nativeAdvanceWindowApplied = false;
    this._nativePlannerActive = false;
    this._lastPlannerAdvance = null;
    this._backendIdentity = null;
    this._backendTimelineEpoch = null;
    this._clearNoFFTWarning();
    if (this._unlistenTransport) {
      try {
        this._unlistenTransport();
      } catch {
        /* ignore */
      }
      this._unlistenTransport = null;
    }
    this._clearFFTState();
    this._events = {};
    this._onceEvents = {};
    this._seenEventSeq.clear();
    this._seenEventSeqOrder.length = 0;
    this._seenEventSeqHead = 0;
    this._state.playlist = [];
    this._musicInfo = null;
    this._quality = null;
    this._lowFreqVolume = 0;
    this._analysisEnabled = false;
    this._fftEventsEnabled = false;
    this._pendingPlayCommand = false;
    if (wasActiveController) {
      this._sendCommand({ type: "setFFT", enabled: false });
      this._sendCommand({ type: "setAnalysis", enabled: false });
      this._sendCommand(isTauri() ? { type: "pauseAudio" } : { type: "close" });
    }
  }
}
