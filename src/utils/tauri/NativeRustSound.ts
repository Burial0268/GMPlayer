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

import type { ISound, SoundEventCallback, SoundEventType } from "../AudioContext/types";
import { AudioTimelineSync, isTauri } from "./audioBridge";
import type { AudioQuality, AudioThreadEvent, DisplayAudioInfo, SongData } from "./audioBridge";
import {
  getAudioBackendTransport,
  isWasmAudioBackendAvailable,
  type AudioBackendTransport,
} from "./audioIpc";

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

export function isAudioBackendRuntimeAvailable(): boolean {
  return isTauri() || isWasmAudioBackendAvailable();
}

/** Resolved/rejected when the load completes — or rejected on timeout. */
type LoadPromise = {
  resolve: () => void;
  reject: (err: Error) => void;
  timeout: ReturnType<typeof setTimeout>;
};

type SyncPromise = {
  resolve: () => void;
  timeout: ReturnType<typeof setTimeout>;
};

type NativeLoadOptions = {
  allowInitialBackendAttach?: boolean;
};

export class NativeRustSound implements ISound {
  private _events: EventMap = {};
  private _onceEvents: EventMap = {};
  private _transport: AudioBackendTransport | null = null;
  private _unlistenTransport: (() => void) | null = null;

  private _path: string;
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
  private _playbackState: "stopped" | "playing" | "paused" | "ended" = "stopped";
  private _pendingPlayCommand: boolean = false;
  private _optimisticPlayback: boolean = isTauri();
  private _adoptNextBackendMusicId: boolean = false;
  private _nativeAutoMixSyncPending: boolean = false;
  private _allowInitialBackendAttach: boolean = false;
  private _backendTrackReady: boolean = false;
  private _nextSeekRequestId: number = 0;
  /** True once a queue window has been applied to the backend: track-end
   * transitions should then be attempted as backend-initiated advances (the
   * fallback timer restores the JS-driven path when no advance lands). Stays
   * true across multiple background advances — wake-up replays several
   * AudioPlayFinished/LoadAudio pairs back-to-back, faster than the async
   * prefill could re-arm a per-transition flag. */
  private _nativeAdvanceWindowApplied: boolean = false;
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

  constructor(src: string | string[]) {
    this._path = Array.isArray(src) ? src[0] : src;
    // Rust computes `local:<file_path>` (see types.rs::SongData::get_id);
    // the reference AMLL hashes the path, but our backend uses the raw
    // path so we mirror that here.
    this._expectedMusicId = `local:${this._path}`;
  }

  // ═════════════════════════════════════════════════════════════╗
  //  load() — register listeners, open track, await load event  ║
  // ═════════════════════════════════════════════════════════════╝

  async load(initialPosition?: number, options?: NativeLoadOptions): Promise<void> {
    if (this._loaded || this._destroyed) return;

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

    const initPos = this._normalizeSeekPosition(initialPosition ?? 0);
    this._backendTrackReady = false;
    this._timeline.reset(initPos);

    // Only the native/Tauri runtime can have a real backend that outlives the
    // current frontend controller. The Web/WASM runtime is page-local and uses a
    // singleton JS transport, so adopting its previous state after SoundManager
    // clears window.$player can bind a new NativeRustSound to a stale <audio>.
    const canAttachExistingBackend = isTauri() && options?.allowInitialBackendAttach === true;
    if (canAttachExistingBackend) {
      this._allowInitialBackendAttach = true;
      await this.requestStatusSync(400);
      this._allowInitialBackendAttach = false;
      if (this._state.musicId && this._state.duration > 0) {
        if (initPos > 0 && Math.abs(this._state.position - initPos) > 1) {
          this.seek(initPos);
        }
        this._loaded = true;
        this._armNoFFTWarning();
        this._emit("load");
        return;
      }
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
    const loadDone = new Promise<void>((resolve, reject) => {
      const timeout = setTimeout(() => {
        if (this._pendingLoad) {
          this._pendingLoad = null;
          reject(new Error(`Native audio load timeout after ${LOAD_TIMEOUT_MS}ms`));
        }
      }, LOAD_TIMEOUT_MS);
      this._pendingLoad = { resolve, reject, timeout };
    });

    // 3. Dispatch setPlaylist + jumpToSong / jumpToSongAt over the invoke
    //    control path. Load completion is driven by LoadAudio / LoadError
    //    events, not by invoke acks.
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
      };
      // `windowed: true` — a bare single-entry queue must stop at track end
      // instead of wrap-replaying itself; the real advance window arrives via
      // `applyNativeQueueWindow` once playback starts.
      this._sendCommand({ type: "setPlaylist", songs: [song], windowed: true });
      if (initPos > 0) {
        this._sendCommand({ type: "jumpToSongAt", songIndex: 0, position: initPos });
      } else {
        this._sendCommand({ type: "jumpToSong", songIndex: 0 });
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

    if (this._destroyed) return;

    if (isTauri()) {
      await this.requestStatusSync(500);
    }

    this._loaded = true;
    this._armNoFFTWarning();
    this._emit("load");
  }

  // ═════════════════════════════════════════════════════════════╗
  //  Transport: invoke control path                             ║
  // ═════════════════════════════════════════════════════════════╝

  private _sendCommand(msg: import("./audioBridge").AudioThreadMessage): boolean {
    const transport = this._transport ?? getAudioBackendTransport();
    this._transport = transport;
    const sentNow = transport.sendOrQueue(msg);
    if (!sentNow && IS_DEV) {
      console.warn("[NativeRustSound] audio control command queued until reconnect", msg.type);
    }
    return sentNow;
  }

  /**
   * Replace the backend playback queue with a prefill window: the current
   * track plus pre-resolved next tracks, carrying real frontend playlist
   * indices as `origOrder`. Arms the native-advance adoption path when the
   * window gives the backend a usable next step — either further entries, or
   * wrap-repeat when `windowed` is false (repeat-one / single-song list).
   */
  applyNativeQueueWindow(songs: SongData[], options: { windowed: boolean }): boolean {
    if (this._destroyed || songs.length === 0) return false;
    this._sendCommand({ type: "setPlaylist", songs, windowed: options.windowed });
    this._nativeAdvanceWindowApplied = true;
    // A usable next step exists when the window has further entries, or when
    // wrap-repeat applies (repeat-one / single-song list, windowed=false).
    return songs.length > 1 || !options.windowed;
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
    if (this._destroyed) return;

    if (seq !== undefined && seq > 0 && this._markSeqSeen(seq)) return;

    switch (evt.type) {
      case "syncStatus": {
        const d = evt.data;
        const expectedBefore = this._expectedMusicId;
        const expectingNativeAutoMixAdoption =
          this._adoptNextBackendMusicId || this._nativeAutoMixSyncPending;
        const allowInitialTauriAttach = isTauri() && this._allowInitialBackendAttach;
        if (
          !this._acceptMusicId(d.musicId, expectingNativeAutoMixAdoption || allowInitialTauriAttach)
        ) {
          if (this._allowInitialBackendAttach) {
            this._resolvePendingSyncs();
          }
          return;
        }
        const adoptedBackendTrack = !!d.musicId && d.musicId !== expectedBefore;
        const pendingNativeAutoMixIndex = this._state.currentPlayIndex;
        const acceptedPosition = this._acceptIncomingPosition(
          this._coerceIncomingPosition(d.position),
        );
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
        this._timeline.setDuration(d.duration);
        this._backendTrackReady = true;
        if (isTauri() && this._allowInitialBackendAttach && this._playbackState === "stopped") {
          this._playbackState = d.isPlaying ? "playing" : "paused";
          this._syncTimelineClock();
        }
        this._resolvePendingSyncs();
        const shouldNotifyNativeAutoMixSync = this._nativeAutoMixSyncPending
          ? adoptedBackendTrack || d.currentPlayIndex === pendingNativeAutoMixIndex
          : (expectingNativeAutoMixAdoption || this._isActiveController()) && adoptedBackendTrack;
        if (shouldNotifyNativeAutoMixSync) {
          this._nativeAutoMixSyncPending = false;
          window.dispatchEvent(
            new CustomEvent(NATIVE_AUTOMIX_SYNC_EVENT, {
              detail: {
                currentIndex: d.currentPlayIndex,
                musicId: d.musicId,
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
        const wasAdvancePending = this._nativeAdvancePending;
        if (this._acceptMusicId(evt.data.musicId)) {
          this._musicInfo = evt.data.musicInfo;
          this._quality = evt.data.quality;
          this._state.duration = evt.data.musicInfo.duration;
          this._timeline.setDuration(evt.data.musicInfo.duration);
          this._backendTrackReady = true;
          this._resolvePendingLoad();
          if (wasAdvancePending) {
            this._completeNativeAdvanceAdoption(
              evt.data.musicId,
              evt.data.currentPlayIndex,
              evt.data.musicInfo.duration,
            );
          }
        }
        break;
      }

      case "loadingAudio":
        if (this._nativeAdvancePending) {
          // Backend confirmed the advance and is resolving the next source —
          // give the download room before falling back to the JS path.
          this._armNativeAdvanceFallback(NATIVE_ADVANCE_LOAD_TIMEOUT_MS);
        }
        break;

      case "playPosition": {
        if (!this._backendTrackReady) break;
        this._acceptIncomingPosition(this._coerceIncomingPosition(evt.data.position));
        break;
      }

      case "playStatus": {
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
          if (isTauri() && this._nativeAdvanceWindowApplied && this._isActiveController()) {
            // The backend queue window holds the real next track and Rust
            // advances on its own (`NextSongGapless`) — even while this JS
            // runtime is frozen in the background. Suppress the legacy
            // 'end' → setPlaySongIndex teardown and adopt the backend
            // transition; the fallback timer re-emits 'end' if no advance
            // lands (window exhausted, expired URL, load failure).
            this._beginNativeAdvanceAdoption();
            break;
          }
          this._playbackState = "ended";
          this._setLocalPosition(this._state.duration);
          this._emit("end");
        } else if (this._isActiveController()) {
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
        this._pendingPlayCommand = false;
        if (evt.type === "loadError") {
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
        break;
    }
  }

  private _markSeqSeen(seq: number): boolean {
    if (this._seenEventSeq.has(seq)) return true;

    this._seenEventSeq.add(seq);
    this._seenEventSeqOrder.push(seq);
    while (this._seenEventSeqOrder.length > SEEN_EVENT_SEQ_LIMIT) {
      const oldSeq = this._seenEventSeqOrder.shift();
      if (oldSeq !== undefined) this._seenEventSeq.delete(oldSeq);
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
    const msg: import("./audioBridge").AudioThreadMessage =
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

  private _beginNativeAdvanceAdoption(): void {
    this._nativeAdvancePending = true;
    this._adoptNextBackendMusicId = true;
    this._armNativeAdvanceFallback(NATIVE_ADVANCE_START_TIMEOUT_MS);
  }

  private _completeNativeAdvanceAdoption(
    musicId: string,
    currentPlayIndex: number,
    duration: number,
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
    this._setLocalPosition(0);
    window.dispatchEvent(
      new CustomEvent(NATIVE_AUTOMIX_SYNC_EVENT, {
        detail: {
          currentIndex: currentPlayIndex,
          musicId: this._state.musicId,
          position: 0,
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
    if (this._destroyed) return this;
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
    if (this._destroyed) return this;
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
    if (this._destroyed) return this;
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
      if (!this._destroyed) {
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
    return this._musicInfo?.duration ?? this._state.duration;
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
    if (this._destroyed) return Promise.resolve();

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

  getEffectManager(): import("../AudioContext/AudioEffectManager").AudioEffectManager | null {
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
