/**
 * NativeRustSound — an ISound implementation backed by the Tauri audio-backend.
 *
 * v5: Local WebSocket primary transport.
 *   - Playback commands go via a dedicated control WebSocket. Falling back
 *     to Tauri invoke breaks realtime controls because it can queue behind
 *     unrelated IPC work.
 *   - Events flow over a separate event WebSocket. FFT/event backpressure
 *     must never share transport state with play/pause/seek commands.
 *   - Play/pause/stop/seek are *optimistic*: the local `_playbackState`
 *     flips and the `play`/`pause` event fires synchronously, then the
 *     server confirmation arrives and is de-duped.
 *   - Position is extrapolated client-side (last `playPosition` + elapsed
 *     wall time) so the seek-bar updates smoothly even though the Rust
 *     side only emits 1 Hz heartbeats.
 */

import type { ISound, SoundEventCallback, SoundEventType } from "../AudioContext/types";
import { isTauri } from "./audioBridge";
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

  /** Anchor for client-side position extrapolation between 1 Hz heartbeats. */
  private _lastPositionEvent: { position: number; receivedAt: number } | null = null;

  /**
   * Recently processed event sequence ids. Priority status events are sent on
   * the control socket while FFT/visual events are sent on the event socket, so
   * cross-socket delivery can be out of order after background throttling. We
   * only drop the exact same seq twice; lower seq is still valid if it was not
   * seen yet.
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

  private _loaded: boolean = false;
  private _destroyed: boolean = false;
  private _playbackState: "stopped" | "playing" | "paused" | "ended" = "stopped";
  private _optimisticPlayback: boolean = isTauri();
  private _adoptNextBackendMusicId: boolean = false;
  private _nativeAutoMixSyncPending: boolean = false;
  private _allowInitialBackendAttach: boolean = false;

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

  async load(initialPosition?: number): Promise<void> {
    if (this._loaded || this._destroyed) return;

    if (!isAudioBackendRuntimeAvailable()) {
      this._emit("loaderror", new Error("Audio backend runtime not available"));
      return;
    }

    // 1. Open the WebSocket (best-effort) and register listeners BEFORE
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

    const canAttachExistingBackend = !window.$player;
    if (canAttachExistingBackend) {
      this._allowInitialBackendAttach = true;
      await this.requestStatusSync(400);
      this._allowInitialBackendAttach = false;
      if (this._state.musicId && this._state.duration > 0) {
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
    const initPos = initialPosition !== undefined && initialPosition > 0 ? initialPosition : 0;
    if (initPos > 0) {
      this._state.position = initPos;
      this._lastPositionEvent = { position: initPos, receivedAt: Date.now() };
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

    // 3. Dispatch setPlaylist + jumpToSong / jumpToSongAt over WebSocket.
    //    Load completion is driven by LoadAudio / LoadError events, not by
    //    invoke acks, so this stays on the realtime IPC path.
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
      this._sendCommand({ type: "setPlaylist", songs: [song] });
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

    this._loaded = true;
    this._armNoFFTWarning();
    this._emit("load");
  }

  // ═════════════════════════════════════════════════════════════╗
  //  Transport: WebSocket-only control path                     ║
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
        if (
          !this._acceptMusicId(
            d.musicId,
            this._isActiveController() || this._allowInitialBackendAttach,
          )
        )
          return;
        const adoptedBackendTrack = !!d.musicId && d.musicId !== expectedBefore;
        const pendingNativeAutoMixIndex = this._state.currentPlayIndex;
        this._state = {
          musicId: d.musicId,
          position: d.position,
          duration: d.duration,
          isPlaying: d.isPlaying,
          volume: d.volume,
          playlist: d.playlist,
          currentPlayIndex: d.currentPlayIndex,
        };
        this._musicInfo = d.musicInfo;
        this._quality = d.quality;
        this._lastPositionEvent = { position: d.position, receivedAt: Date.now() };
        this._resolvePendingSyncs();
        const shouldNotifyNativeAutoMixSync = this._nativeAutoMixSyncPending
          ? adoptedBackendTrack || d.currentPlayIndex === pendingNativeAutoMixIndex
          : (expectingNativeAutoMixAdoption ||
              this._isActiveController() ||
              this._allowInitialBackendAttach) &&
            adoptedBackendTrack;
        if (shouldNotifyNativeAutoMixSync) {
          this._nativeAutoMixSyncPending = false;
          window.dispatchEvent(
            new CustomEvent(NATIVE_AUTOMIX_SYNC_EVENT, {
              detail: {
                currentIndex: d.currentPlayIndex,
                musicId: d.musicId,
                position: d.position,
                duration: d.duration,
              },
            }),
          );
        }
        // NOTE: do NOT update `_playbackState` from syncStatus. State
        // transitions belong to `PlayStatus` events only. SyncStatus is
        // emitted by Rust at points like `start_playing_song` end —
        // where `sink.is_paused()` may still report `true` for a tick
        // even though a follow-on `ResumeAudio` is already queued.
        // Updating state here would revert an optimistic `"playing"`
        // back to `"paused"`, and the eventual `PlayStatus(true)` would
        // then re-emit `"play"`, causing a duplicate toast.
        break;
      }

      case "loadAudio": {
        if (this._acceptMusicId(evt.data.musicId)) {
          this._musicInfo = evt.data.musicInfo;
          this._quality = evt.data.quality;
          this._state.duration = evt.data.musicInfo.duration;
          this._resolvePendingLoad();
        }
        break;
      }

      case "loadingAudio":
        break;

      case "playPosition": {
        this._state.position = evt.data.position;
        this._lastPositionEvent = { position: evt.data.position, receivedAt: Date.now() };
        break;
      }

      case "playStatus": {
        const wantPlaying = evt.data.isPlaying;
        this._state.isPlaying = wantPlaying;
        const isCurrentlyPlaying = this._playbackState === "playing";
        if (wantPlaying === isCurrentlyPlaying) {
          // Already in this state (likely from an optimistic flip) —
          // don't re-emit; consumers would see duplicate play/pause.
          break;
        }
        if (wantPlaying) {
          this._playbackState = "playing";
          this._emit("play");
        } else {
          this._playbackState = "paused";
          this._emit("pause");
        }
        break;
      }

      case "audioPlayFinished": {
        if (evt.data.musicId === this._expectedMusicId) {
          this._playbackState = "ended";
          this._state.position = this._state.duration;
          this._emit("end");
        } else if (this._isActiveController()) {
          this._adoptNextBackendMusicId = true;
          this._nativeAutoMixSyncPending = true;
          void this.requestStatusSync().then(() => {
            if (this._destroyed) return;
            this._playbackState = "ended";
            this._state.position = this._state.duration;
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
          if (this._musicInfo) {
            this._musicInfo = { ...this._musicInfo, duration: evt.data.duration };
          }
        }
        if (typeof evt.data.position === "number" && evt.data.position >= 0) {
          this._state.position = evt.data.position;
          this._lastPositionEvent = { position: evt.data.position, receivedAt: Date.now() };
          if (this._musicInfo) {
            this._musicInfo = { ...this._musicInfo, position: evt.data.position };
          }
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
        if (this._noFFTWarnTimer !== null) {
          clearTimeout(this._noFFTWarnTimer);
          this._noFFTWarnTimer = null;
        }
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

      case "playError":
      case "loadError": {
        const err = new Error(evt.data.error);
        if (evt.type === "loadError") {
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

  private _isActiveController(): boolean {
    return window.$player === this;
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
    // Web/WASM does not use the native WS/CPAL FFT event stream. It may add
    // analysis later through a WASM decoder/PCM path, but absence of `fftData`
    // there is not a WebSocket/backend failure.
    if (!isTauri()) return;
    if (this._noFFTWarnTimer !== null) clearTimeout(this._noFFTWarnTimer);
    this._noFFTWarnTimer = setTimeout(() => {
      this._noFFTWarnTimer = null;
      if (!this._fftReceived && this._playbackState === "playing") {
        console.warn(
          `[NativeRustSound] No fftData event received within ${NO_FFT_WARN_MS}ms of load. ` +
            `Check Rust logs for "FFT broadcast: ... empty ticks" or WS connection state.`,
        );
      }
    }, NO_FFT_WARN_MS);
  }

  // ═════════════════════════════════════════════════════════════╗
  //  ISound interface                                           ║
  // ═════════════════════════════════════════════════════════════╝

  playing(): boolean {
    return this._playbackState === "playing";
  }

  play(): this {
    if (this._destroyed) return this;
    this._sendCommand({ type: "resumeAudio" });
    if (this._optimisticPlayback && this._playbackState !== "playing") {
      this._playbackState = "playing";
      this._state.isPlaying = true;
      this._lastPositionEvent = { position: this._state.position, receivedAt: Date.now() };
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
    this._sendCommand({ type: "pauseAudio" });
    if (this._optimisticPlayback && this._playbackState !== "paused") {
      this._playbackState = "paused";
      this._state.isPlaying = false;
      this._lastPositionEvent = { position: this._state.position, receivedAt: Date.now() };
      this._emitDeferred("pause");
    }
    return this;
  }

  stop(): this {
    if (this._destroyed) return this;
    this._sendCommand({ type: "pauseAudio" });
    this._sendCommand({ type: "seekAudio", position: 0 });
    this._state.position = 0;
    this._state.isPlaying = false;
    this._lastPositionEvent = { position: 0, receivedAt: Date.now() };
    if (this._optimisticPlayback && this._playbackState !== "stopped") {
      this._playbackState = "stopped";
      this._emitDeferred("pause");
    }
    return this;
  }

  seek(pos?: number): number | this {
    if (pos !== undefined) {
      if (!this._destroyed) {
        this._sendCommand({ type: "seekAudio", position: pos });
        this._state.position = pos;
        this._lastPositionEvent = { position: pos, receivedAt: Date.now() };
      }
      return this;
    }
    // Getter: extrapolate position client-side. Rust emits PlayPosition
    // once on state changes / seeks and again every 1 s — between those,
    // we add elapsed wall time so the seek-bar / time display stay smooth.
    if (this._playbackState === "playing" && this._lastPositionEvent) {
      const elapsed = (Date.now() - this._lastPositionEvent.receivedAt) / 1000;
      const duration = this._musicInfo?.duration ?? this._state.duration;
      const extrapolated = this._lastPositionEvent.position + elapsed;
      return duration > 0 ? Math.min(extrapolated, duration) : extrapolated;
    }
    return this._state.position;
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
    this._destroyed = true;
    this._clearPendingLoad();
    for (const sync of this._pendingSyncs.splice(0)) {
      clearTimeout(sync.timeout);
      sync.resolve();
    }
    if (this._noFFTWarnTimer !== null) {
      clearTimeout(this._noFFTWarnTimer);
      this._noFFTWarnTimer = null;
    }
    if (this._unlistenTransport) {
      try {
        this._unlistenTransport();
      } catch {
        /* ignore */
      }
      this._unlistenTransport = null;
    }
    this._fftData = [];
    this._frequencyData = new Uint8Array(0);
    this._averageAmplitude = 0;
    this._lowFreqVolume = 0;
    this._sendCommand({ type: "pauseAudio" });
  }
}
