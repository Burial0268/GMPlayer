import type {
  AudioThreadEvent,
  AudioThreadEventCallback,
  AudioThreadEventMessage,
  AudioThreadMessage,
} from "./audioBridge";
import { audioSendMsg, isTauri, listenPlayerEvents } from "./audioBridge";
import { getAudioWs } from "./audioWs";

const WASM_ANALYSIS_INTERVAL_MS = 66;
// Drive the UI position from the real <audio> clock at a few Hz. The element
// clock is the ground truth and lives on this thread, so we dispatch locally
// instead of round-tripping every heartbeat through the worker. A fast cadence
// keeps NativeRustSound's wall-clock extrapolation from outrunning real
// playback (which is what made the timeline rewind at the start of a track).
const WASM_POSITION_DISPATCH_INTERVAL_MS = 200;
// The worker only needs its internal position for occasional syncStatus/state
// reads, so refresh it lazily rather than on every dispatch tick.
const WASM_WORKER_POSITION_SYNC_INTERVAL_MS = 1000;

export interface AudioBackendTransport {
  connect(): Promise<void>;
  subscribe(listener: AudioThreadEventCallback): () => void;
  sendOrQueue(msg: AudioThreadMessage): boolean;
  getGainNode?(): GainNode | null;
  ensureAudioGraph?(): Promise<boolean>;
  shutdown?(): void;
}

export function isWasmAudioBackendAvailable(): boolean {
  return (
    typeof window !== "undefined" && typeof Audio !== "undefined" && typeof Worker !== "undefined"
  );
}

class TauriInvokeAudioIpc implements AudioBackendTransport {
  private _listeners: Set<AudioThreadEventCallback> = new Set();
  private _unlisten: (() => void) | null = null;
  private _connectPromise: Promise<void> | null = null;

  connect(): Promise<void> {
    if (this._unlisten) return Promise.resolve();
    if (this._connectPromise) return this._connectPromise;

    this._connectPromise = listenPlayerEvents((event, seq) => {
      this._dispatch(event, seq);
    })
      .then((unlisten) => {
        this._unlisten = unlisten;
      })
      .finally(() => {
        this._connectPromise = null;
      });

    return this._connectPromise;
  }

  subscribe(listener: AudioThreadEventCallback): () => void {
    this._listeners.add(listener);
    return () => {
      this._listeners.delete(listener);
    };
  }

  sendOrQueue(msg: AudioThreadMessage): boolean {
    void audioSendMsg(msg);
    return true;
  }

  shutdown(): void {
    if (this._unlisten) {
      this._unlisten();
      this._unlisten = null;
    }
    this._listeners.clear();
  }

  private _dispatch(event: AudioThreadEvent, seq?: number): void {
    for (const listener of this._listeners) {
      try {
        listener(event, seq);
      } catch {
        /* listener errors should not break dispatch */
      }
    }
  }
}

class TauriHybridAudioIpc implements AudioBackendTransport {
  private _active: AudioBackendTransport | null = null;
  private _fallback: TauriInvokeAudioIpc | null = null;

  async connect(): Promise<void> {
    if (this._active) {
      await this._active.connect();
      return;
    }

    const ws = getAudioWs();
    try {
      await ws.connect();
      this._active = ws;
      return;
    } catch (err) {
      console.warn(
        "[audioIpc] WebSocket audio transport unavailable, falling back to Tauri IPC",
        err,
      );
    }

    this._fallback = new TauriInvokeAudioIpc();
    await this._fallback.connect();
    this._active = this._fallback;
  }

  subscribe(listener: AudioThreadEventCallback): () => void {
    if (!this._active) {
      throw new Error("Audio backend transport is not connected");
    }
    return this._active.subscribe(listener);
  }

  sendOrQueue(msg: AudioThreadMessage): boolean {
    if (!this._active) {
      void this.connect().then(() => this._active?.sendOrQueue(msg));
      return false;
    }
    return this._active.sendOrQueue(msg);
  }

  shutdown(): void {
    this._active?.shutdown?.();
    this._active = null;
    this._fallback = null;
  }
}

type WasmEffect =
  | {
      type: "loadTrack";
      src: string;
      initialPosition: number;
      musicId: string;
      currentPlayIndex: number;
    }
  | { type: "play" }
  | { type: "pause" }
  | { type: "seek"; position: number }
  | { type: "setVolume"; volume: number }
  | { type: "setOutputDevice"; name: string }
  | { type: "close" };

interface WasmReply {
  events?: AudioThreadEventMessage<AudioThreadEvent>[];
  effects?: WasmEffect[];
  error?: string;
}

type WasmBackendWorkerRequest =
  | { id: number; type: "init" }
  | {
      id: number;
      type: "sendMessage";
      envelope: Omit<AudioThreadEventMessage<AudioThreadMessage>, "seq">;
    }
  | {
      id: number;
      type: "loadAnalysisBytes";
      bytes: Uint8Array;
      extension: string;
      musicId: string;
    }
  | { id: number; type: "processAnalysisFrame"; position: number; deltaMs: number }
  | {
      id: number;
      type: "applyLoadedTrack";
      duration: number;
      sampleRate: number;
      channels: number;
      bitrate: number;
    }
  | { id: number; type: "applyPlaybackState"; isPlaying: boolean }
  | { id: number; type: "applyPlayPosition"; position: number }
  | { id: number; type: "applyPlaybackFinished" }
  | { id: number; type: "applyVolume"; volume: number }
  | { id: number; type: "applyLoadError"; error: string }
  | { id: number; type: "applyPlayError"; error: string }
  | { id: number; type: "syncStatus" }
  | { id: number; type: "state" }
  | { id: number; type: "shutdown" };

type WasmBackendWorkerResponse =
  | { id: number; ok: true; reply?: WasmReply; stateJson?: string }
  | { id: number; ok: false; error: string };

type WasmBackendWorkerCall = {
  resolve: (value: WasmReply | string | undefined) => void;
  reject: (err: Error) => void;
};

type WasmBackendWorkerRequestPayload = {
  type: WasmBackendWorkerRequest["type"];
  [key: string]: unknown;
};

class WasmAudioBackendWorkerHost {
  private _worker: Worker;
  private _nextRequestId = 0;
  private _pending = new Map<number, WasmBackendWorkerCall>();

  constructor() {
    this._worker = new Worker(new URL("./audioBackendWorker.ts", import.meta.url), {
      type: "module",
      name: "gmplayer-audio-backend",
    });
    this._worker.onmessage = (event: MessageEvent<WasmBackendWorkerResponse>) => {
      this._handleMessage(event.data);
    };
    this._worker.onerror = (event) => {
      this._rejectAll(new Error(event.message || "WASM audio backend worker error"));
    };
    this._worker.onmessageerror = () => {
      this._rejectAll(new Error("WASM audio backend worker message error"));
    };
  }

  async init(): Promise<void> {
    await this._callReply({ type: "init" });
  }

  sendMessage(
    envelope: Omit<AudioThreadEventMessage<AudioThreadMessage>, "seq">,
  ): Promise<WasmReply> {
    return this._callReply({ type: "sendMessage", envelope });
  }

  loadAnalysisBytes(bytes: Uint8Array, extension: string, musicId: string): Promise<WasmReply> {
    return this._callReply({ type: "loadAnalysisBytes", bytes, extension, musicId }, [
      bytes.buffer,
    ]);
  }

  processAnalysisFrame(position: number, deltaMs: number): Promise<WasmReply> {
    return this._callReply({ type: "processAnalysisFrame", position, deltaMs });
  }

  applyLoadedTrack(
    duration: number,
    sampleRate: number,
    channels: number,
    bitrate: number,
  ): Promise<WasmReply> {
    return this._callReply({
      type: "applyLoadedTrack",
      duration,
      sampleRate,
      channels,
      bitrate,
    });
  }

  applyPlaybackState(isPlaying: boolean): Promise<WasmReply> {
    return this._callReply({ type: "applyPlaybackState", isPlaying });
  }

  applyPlayPosition(position: number): Promise<WasmReply> {
    return this._callReply({ type: "applyPlayPosition", position });
  }

  applyPlaybackFinished(): Promise<WasmReply> {
    return this._callReply({ type: "applyPlaybackFinished" });
  }

  applyVolume(volume: number): Promise<WasmReply> {
    return this._callReply({ type: "applyVolume", volume });
  }

  applyLoadError(error: string): Promise<WasmReply> {
    return this._callReply({ type: "applyLoadError", error });
  }

  applyPlayError(error: string): Promise<WasmReply> {
    return this._callReply({ type: "applyPlayError", error });
  }

  syncStatus(): Promise<WasmReply> {
    return this._callReply({ type: "syncStatus" });
  }

  async stateJson(): Promise<string> {
    const value = await this._call({ type: "state" });
    return typeof value === "string" ? value : "{}";
  }

  shutdown(): void {
    this._rejectAll(new Error("WASM audio backend worker shut down"));
    try {
      this._worker.postMessage({ id: this._newRequestId(), type: "shutdown" });
    } catch {
      /* worker may already be gone */
    }
    this._worker.terminate();
  }

  private async _callReply(
    request: WasmBackendWorkerRequestPayload,
    transfer?: Transferable[],
  ): Promise<WasmReply> {
    const value = await this._call(request, transfer);
    return value && typeof value === "object" ? (value as WasmReply) : {};
  }

  private _call(
    request: WasmBackendWorkerRequestPayload,
    transfer?: Transferable[],
  ): Promise<WasmReply | string | undefined> {
    const id = this._newRequestId();
    const message = { id, ...request } as WasmBackendWorkerRequest;
    return new Promise((resolve, reject) => {
      this._pending.set(id, { resolve, reject });
      try {
        this._worker.postMessage(message, transfer ?? []);
      } catch (err) {
        this._pending.delete(id);
        reject(err instanceof Error ? err : new Error(String(err)));
      }
    });
  }

  private _handleMessage(response: WasmBackendWorkerResponse): void {
    const pending = this._pending.get(response.id);
    if (!pending) return;
    this._pending.delete(response.id);

    if ("error" in response) {
      pending.reject(new Error(response.error));
      return;
    }
    pending.resolve(response.stateJson ?? response.reply);
  }

  private _rejectAll(err: Error): void {
    const pending = Array.from(this._pending.values());
    this._pending.clear();
    for (const call of pending) {
      call.reject(err);
    }
  }

  private _newRequestId(): number {
    this._nextRequestId = (this._nextRequestId + 1) >>> 0;
    return this._nextRequestId || this._newRequestId();
  }
}

type WasmAnalysisWorkerRequest =
  | {
      id: number;
      type: "load";
      src: string;
      musicId: string;
      currentPlayIndex: number;
      initialPosition: number;
    }
  | { id: number; type: "process"; position: number; deltaMs: number }
  | { id: number; type: "setFreqRange"; fromFreq: number; toFreq: number }
  | { id: number; type: "setFFT"; enabled: boolean }
  | { id: number; type: "shutdown" };

type WasmAnalysisWorkerResponse =
  | { id: number; ok: true; reply?: WasmReply }
  | { id: number; ok: false; error: string };

type WasmAnalysisWorkerCall = {
  resolve: (value: WasmReply | undefined) => void;
  reject: (err: Error) => void;
};

type WasmAnalysisWorkerRequestPayload = {
  type: WasmAnalysisWorkerRequest["type"];
  [key: string]: unknown;
};

class WasmAudioAnalysisWorkerHost {
  private _worker: Worker;
  private _nextRequestId = 0;
  private _pending = new Map<number, WasmAnalysisWorkerCall>();

  constructor() {
    this._worker = new Worker(new URL("./audioAnalysisWorker.ts", import.meta.url), {
      type: "module",
      name: "gmplayer-audio-analysis",
    });
    this._worker.onmessage = (event: MessageEvent<WasmAnalysisWorkerResponse>) => {
      this._handleMessage(event.data);
    };
    this._worker.onerror = (event) => {
      this._rejectAll(new Error(event.message || "WASM audio analysis worker error"));
    };
    this._worker.onmessageerror = () => {
      this._rejectAll(new Error("WASM audio analysis worker message error"));
    };
  }

  load(
    src: string,
    musicId: string,
    currentPlayIndex: number,
    initialPosition: number,
  ): Promise<WasmReply> {
    return this._callReply({
      type: "load",
      src,
      musicId,
      currentPlayIndex,
      initialPosition,
    });
  }

  processFrame(position: number, deltaMs: number): Promise<WasmReply> {
    return this._callReply({ type: "process", position, deltaMs });
  }

  setFreqRange(fromFreq: number, toFreq: number): Promise<WasmReply> {
    return this._callReply({ type: "setFreqRange", fromFreq, toFreq });
  }

  setFFT(enabled: boolean): Promise<WasmReply> {
    return this._callReply({ type: "setFFT", enabled });
  }

  shutdown(): void {
    this._rejectAll(new Error("WASM audio analysis worker shut down"));
    try {
      this._worker.postMessage({ id: this._newRequestId(), type: "shutdown" });
    } catch {
      /* worker may already be gone */
    }
    this._worker.terminate();
  }

  private async _callReply(request: WasmAnalysisWorkerRequestPayload): Promise<WasmReply> {
    const value = await this._call(request);
    return value && typeof value === "object" ? value : {};
  }

  private _call(request: WasmAnalysisWorkerRequestPayload): Promise<WasmReply | undefined> {
    const id = this._newRequestId();
    const message = { id, ...request } as WasmAnalysisWorkerRequest;
    return new Promise((resolve, reject) => {
      this._pending.set(id, { resolve, reject });
      try {
        this._worker.postMessage(message);
      } catch (err) {
        this._pending.delete(id);
        reject(err instanceof Error ? err : new Error(String(err)));
      }
    });
  }

  private _handleMessage(response: WasmAnalysisWorkerResponse): void {
    const pending = this._pending.get(response.id);
    if (!pending) return;
    this._pending.delete(response.id);

    if ("error" in response) {
      pending.reject(new Error(response.error));
      return;
    }
    pending.resolve(response.reply);
  }

  private _rejectAll(err: Error): void {
    const pending = Array.from(this._pending.values());
    this._pending.clear();
    for (const call of pending) {
      call.reject(err);
    }
  }

  private _newRequestId(): number {
    this._nextRequestId = (this._nextRequestId + 1) >>> 0;
    return this._nextRequestId || this._newRequestId();
  }
}

class WasmAudioBackendIpc implements AudioBackendTransport {
  private _listeners: Set<AudioThreadEventCallback> = new Set();
  private _backend: WasmAudioBackendWorkerHost | null = null;
  private _analysisWorker: WasmAudioAnalysisWorkerHost | null = null;
  private _connectPromise: Promise<void> | null = null;
  private _audio: HTMLAudioElement | null = null;
  private _boundAudio: HTMLAudioElement | null = null;
  private _syncingElementVolume = false;
  private _positionTimer: ReturnType<typeof setInterval> | null = null;
  private _lastWorkerPositionSyncAt = 0;
  private _analysisTimer: ReturnType<typeof setInterval> | null = null;
  private _analysisFrameInFlight = false;
  private _analysisEnabled = true;
  private _analysisFftEnabled = true;
  private _analysisFreqRange: { fromFreq: number; toFreq: number } | null = null;
  private _analysisGeneration = 0;
  private _analysisReady = false;
  private _lastAnalysisAt = 0;
  private _currentVolume = 1;
  private _outputDeviceName = "";
  private _isClosing = false;
  private _playPromise: Promise<void> | null = null;

  connect(): Promise<void> {
    if (this._backend) return Promise.resolve();
    if (this._connectPromise) return this._connectPromise;

    const backend = new WasmAudioBackendWorkerHost();
    this._backend = backend;
    this._connectPromise = backend
      .init()
      .catch((err) => {
        if (this._backend === backend) {
          backend.shutdown();
          this._backend = null;
        }
        throw err;
      })
      .finally(() => {
        this._connectPromise = null;
      });

    return this._connectPromise;
  }

  subscribe(listener: AudioThreadEventCallback): () => void {
    this._listeners.add(listener);
    return () => {
      this._listeners.delete(listener);
    };
  }

  sendOrQueue(msg: AudioThreadMessage): boolean {
    this._mirrorAnalysisControl(msg);
    void this.connect()
      .then(() => {
        const backend = this._backend;
        if (!backend) return;
        return backend
          .sendMessage({
            callbackId: this._newCallbackId(),
            data: msg,
          })
          .then((reply) => {
            if (this._backend === backend) {
              this._handleReply(reply);
            }
          });
      })
      .catch((err) => {
        console.warn("[audioIpc] WASM audio backend command failed", err);
      });
    return this._backend !== null;
  }

  getGainNode(): GainNode | null {
    return null;
  }

  async ensureAudioGraph(): Promise<boolean> {
    return false;
  }

  shutdown(): void {
    this._cancelAnalysis();
    this._stopPositionTimer();
    this._releaseAudio();
    this._playPromise = null;
    this._backend?.shutdown();
    this._backend = null;
    this._listeners.clear();
  }

  private _handleReply(reply: WasmReply, preserveSeq = true): void {
    if (reply.error) {
      console.warn("[audioIpc] WASM audio backend:", reply.error);
    }
    for (const event of reply.events ?? []) {
      if (event.data) {
        this._dispatch(event.data, preserveSeq ? event.seq : undefined);
      }
    }
    for (const effect of reply.effects ?? []) {
      this._applyEffect(effect);
    }
  }

  private _mirrorAnalysisControl(msg: AudioThreadMessage): void {
    switch (msg.type) {
      case "setAnalysis":
        this._analysisEnabled = msg.enabled;
        if (!msg.enabled) {
          this._stopAnalysisTimer();
        } else if (
          this._analysisReady &&
          this._audio &&
          !this._audio.paused &&
          !this._audio.ended
        ) {
          this._startAnalysisTimer();
        }
        break;
      case "setFFT":
        this._analysisFftEnabled = msg.enabled;
        this._handleAnalysisReplyPromise(this._analysisWorker?.setFFT(msg.enabled));
        break;
      case "setFFTRange":
        this._analysisFreqRange = {
          fromFreq: msg.fromFreq,
          toFreq: msg.toFreq,
        };
        this._handleAnalysisReplyPromise(
          this._analysisWorker?.setFreqRange(msg.fromFreq, msg.toFreq),
        );
        break;
    }
  }

  private _applyEffect(effect: WasmEffect): void {
    switch (effect.type) {
      case "loadTrack":
        this._loadTrack(effect);
        break;
      case "play":
        this._play();
        break;
      case "pause":
        this._playPromise = null;
        this._audio?.pause();
        this._stopAnalysisTimer();
        break;
      case "seek":
        this._seek(effect.position);
        break;
      case "setVolume":
        this._currentVolume = Math.max(0, Math.min(1, effect.volume));
        this._applyOutputVolume();
        break;
      case "setOutputDevice":
        this._setOutputDevice(effect.name);
        break;
      case "close":
        this._cancelAnalysis();
        this._stopPositionTimer();
        this._releaseAudio();
        break;
    }
  }

  private _loadTrack(effect: Extract<WasmEffect, { type: "loadTrack" }>): void {
    const generation = this._resetAnalysis();
    this._stopPositionTimer();
    this._releaseAudio();
    this._playPromise = null;

    const audio = new Audio();
    // Playback does not read PCM from this element. Setting crossOrigin here
    // forces CORS validation and breaks otherwise playable remote media URLs.
    // WASM analysis fetches bytes separately and can use the proxy fallback.
    audio.preload = "auto";
    audio.src = effect.src;
    this._setElementVolumeForCurrentGraph(audio);
    this._audio = audio;
    if (this._outputDeviceName) {
      this._setOutputDevice(this._outputDeviceName);
    }
    this._bindAudio(audio, effect.initialPosition);
    audio.load();

    void this._loadAnalysis(
      effect.src,
      effect.musicId,
      effect.currentPlayIndex,
      effect.initialPosition,
      generation,
    );
  }

  private _bindAudio(audio: HTMLAudioElement, initialPosition: number): void {
    this._boundAudio = audio;

    audio.addEventListener("loadedmetadata", () => {
      if (this._audio !== audio) return;
      const duration = Number.isFinite(audio.duration) ? audio.duration : 0;
      if (initialPosition > 0 && Number.isFinite(initialPosition)) {
        this._seek(initialPosition);
      }
      this._handleReplyPromise(this._backend?.applyLoadedTrack(duration, 44100, 2, 0));
    });

    audio.addEventListener("play", () => {
      if (this._audio !== audio) return;
      this._playPromise = null;
      this._handleReplyPromise(this._backend?.applyPlaybackState(true));
      this._startPositionTimer();
      this._startAnalysisTimer();
    });

    audio.addEventListener("pause", () => {
      if (this._audio !== audio || this._isClosing || audio.ended) return;
      this._playPromise = null;
      this._stopAnalysisTimer();
      this._handleReplyPromise(this._backend?.applyPlaybackState(false));
    });

    audio.addEventListener("ended", () => {
      if (this._audio !== audio) return;
      this._stopPositionTimer();
      this._stopAnalysisTimer();
      this._resetLowFreq();
      this._handleReplyPromise(this._backend?.applyPlaybackFinished());
    });

    audio.addEventListener("volumechange", () => {
      if (this._audio !== audio) return;
      if (this._syncingElementVolume) return;
      this._currentVolume = audio.volume;
      this._handleReplyPromise(this._backend?.applyVolume(audio.volume));
    });

    audio.addEventListener("error", () => {
      if (this._audio !== audio) return;
      const error = this._formatMediaError(audio.error);
      this._handleReplyPromise(this._backend?.applyLoadError(error));
    });
  }

  private _play(): void {
    const audio = this._audio;
    if (!audio) return;
    if (!audio.paused && !audio.ended) return;
    if (this._playPromise) return;

    const playPromise = audio.play();
    this._playPromise = playPromise;
    void playPromise
      .catch((err) => {
        if (this._playPromise !== playPromise) return;
        this._playPromise = null;
        const message = err instanceof Error ? err.message : String(err);
        this._handleReplyPromise(this._backend?.applyPlayError(message));
      })
      .finally(() => {
        if (this._playPromise === playPromise) {
          this._playPromise = null;
        }
      });
  }

  private _seek(position: number): void {
    const audio = this._audio;
    if (!audio) return;
    const next = Number.isFinite(position) ? Math.max(0, position) : 0;
    try {
      audio.currentTime = next;
      this._lastAnalysisAt = 0;
      this._resetLowFreq();
      this._handleReplyPromise(this._backend?.applyPlayPosition(next));
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      this._handleReplyPromise(this._backend?.applyPlayError(message));
    }
  }

  private _applyOutputVolume(): void {
    if (this._audio) {
      this._setElementVolumeForCurrentGraph(this._audio);
    }
  }

  private _setElementVolumeForCurrentGraph(audio: HTMLAudioElement): void {
    const nextVolume = this._currentVolume;
    if (Math.abs(audio.volume - nextVolume) < 0.0001) return;
    this._syncingElementVolume = true;
    try {
      audio.volume = nextVolume;
    } finally {
      queueMicrotask(() => {
        this._syncingElementVolume = false;
      });
    }
  }

  private _setOutputDevice(name: string): void {
    const audio = this._audio;
    const outputName = name.trim();
    this._outputDeviceName = outputName;
    const isDefault =
      outputName === "" ||
      outputName.toLowerCase() === "default" ||
      outputName.toLowerCase() === "system";
    const sinkId = isDefault ? "" : outputName;
    const dispatchChanged = () => {
      this._dispatch({
        type: "audioOutputChanged",
        data: {
          deviceName: isDefault ? "browser default" : outputName,
          isDefault,
          channels: 2,
          sampleRate: 44100,
          sampleFormat: "browser",
        },
      });
    };
    const dispatchError = (error: string) => {
      this._dispatch({
        type: "audioOutputError",
        data: { error, recoverable: true },
      });
    };

    if (!audio) {
      if (isDefault) dispatchChanged();
      return;
    }

    const setSinkId = (
      audio as HTMLAudioElement & {
        setSinkId?: (sinkId: string) => Promise<void>;
      }
    ).setSinkId;
    if (typeof setSinkId !== "function") {
      if (isDefault) {
        dispatchChanged();
      } else {
        dispatchError("browser does not support selecting an audio output device");
      }
      return;
    }

    void setSinkId
      .call(audio, sinkId)
      .then(dispatchChanged)
      .catch((err: unknown) => {
        const message = err instanceof Error ? err.message : String(err);
        dispatchError(message);
      });
  }

  private _resetLowFreq(): void {
    this._dispatch({
      type: "lowFrequencyVolume",
      data: { volume: 0 },
    });
  }

  private _startPositionTimer(): void {
    if (this._positionTimer !== null) return;
    this._lastWorkerPositionSyncAt = 0;
    this._positionTimer = setInterval(() => {
      this._tickPosition();
    }, WASM_POSITION_DISPATCH_INTERVAL_MS);
    // Dispatch immediately so the position anchor is re-stamped to the real
    // element clock the instant playback starts. Otherwise the first
    // extrapolated reads inherit the stale load-time anchor, run ahead of real
    // playback, and snap backward when the first heartbeat lands.
    this._tickPosition();
  }

  private _tickPosition(): void {
    const audio = this._audio;
    if (!audio || audio.paused || audio.ended) return;
    const position = audio.currentTime;
    // Emit the position event locally from the real element clock. Routing each
    // heartbeat through the worker only to echo it back costs a postMessage
    // round-trip per tick and arrives too coarse (1 Hz) to keep client-side
    // extrapolation aligned with real playback.
    this._dispatch({ type: "playPosition", data: { position } });
    // Keep the worker's internal position roughly current for any later
    // syncStatus/state read, but at a low rate and without re-dispatching its
    // (older) echo on top of the fresh local value above.
    const now = this._nowMs();
    if (now - this._lastWorkerPositionSyncAt >= WASM_WORKER_POSITION_SYNC_INTERVAL_MS) {
      this._lastWorkerPositionSyncAt = now;
      const pending = this._backend?.applyPlayPosition(position);
      if (pending) void pending.catch(() => {});
    }
  }

  private _stopPositionTimer(): void {
    if (this._positionTimer !== null) {
      clearInterval(this._positionTimer);
      this._positionTimer = null;
    }
    this._lastWorkerPositionSyncAt = 0;
  }

  private _startAnalysisTimer(): void {
    if (!this._analysisEnabled) return;
    if (!this._analysisReady) return;
    if (this._analysisTimer !== null) return;
    this._lastAnalysisAt = this._nowMs();
    this._analysisTimer = setInterval(() => {
      this._tickAnalysis();
    }, WASM_ANALYSIS_INTERVAL_MS);
    this._tickAnalysis();
  }

  private _stopAnalysisTimer(): void {
    if (this._analysisTimer !== null) {
      clearInterval(this._analysisTimer);
      this._analysisTimer = null;
    }
    this._lastAnalysisAt = 0;
  }

  private _tickAnalysis(): void {
    const audio = this._audio;
    const analysisWorker = this._analysisWorker;
    if (!this._analysisEnabled) return;
    if (!audio || !analysisWorker || !this._analysisReady || audio.paused || audio.ended) return;
    if (this._analysisFrameInFlight) return;

    const now = this._nowMs();
    const deltaMs =
      this._lastAnalysisAt > 0 ? now - this._lastAnalysisAt : WASM_ANALYSIS_INTERVAL_MS;
    this._lastAnalysisAt = now;
    const generation = this._analysisGeneration;
    this._analysisFrameInFlight = true;
    void analysisWorker
      .processFrame(audio.currentTime, deltaMs)
      .then((reply) => {
        if (
          this._analysisWorker === analysisWorker &&
          this._audio === audio &&
          this._analysisGeneration === generation &&
          this._analysisEnabled &&
          this._analysisReady
        ) {
          this._handleReply(reply, false);
        }
      })
      .catch((err) => {
        if (this._analysisWorker === analysisWorker && this._analysisGeneration === generation) {
          console.warn("[audioIpc] WASM audio analysis frame failed", err);
        }
      })
      .finally(() => {
        this._analysisFrameInFlight = false;
      });
  }

  private _resetAnalysis(): number {
    this._analysisGeneration = (this._analysisGeneration + 1) >>> 0;
    this._analysisReady = false;
    this._analysisFrameInFlight = false;
    this._lastAnalysisAt = 0;
    this._stopAnalysisTimer();
    this._analysisWorker?.shutdown();
    this._analysisWorker = null;
    return this._analysisGeneration;
  }

  private _cancelAnalysis(): void {
    this._resetAnalysis();
  }

  private async _loadAnalysis(
    src: string,
    musicId: string,
    currentPlayIndex: number,
    initialPosition: number,
    generation: number,
  ): Promise<void> {
    const analysisWorker = new WasmAudioAnalysisWorkerHost();
    this._analysisWorker = analysisWorker;

    try {
      if (this._analysisFreqRange) {
        await analysisWorker.setFreqRange(
          this._analysisFreqRange.fromFreq,
          this._analysisFreqRange.toFreq,
        );
      }
      const reply = await analysisWorker.load(src, musicId, currentPlayIndex, initialPosition);
      if (this._analysisGeneration !== generation || this._analysisWorker !== analysisWorker) {
        analysisWorker.shutdown();
        return;
      }

      if (!this._analysisFftEnabled) {
        await analysisWorker.setFFT(false);
      }

      this._analysisReady = !reply.error;
      this._handleReply(reply, false);
      if (this._analysisReady && this._audio && !this._audio.paused && !this._audio.ended) {
        this._startAnalysisTimer();
      }
    } catch (err) {
      const isAbort = err instanceof Error && err.name === "AbortError";
      if (!isAbort && this._analysisGeneration === generation) {
        console.warn("[audioIpc] WASM audio analysis load failed", err);
      }
    } finally {
      if (this._analysisGeneration !== generation && this._analysisWorker === analysisWorker) {
        analysisWorker.shutdown();
        this._analysisWorker = null;
      }
    }
  }

  private _releaseAudio(): void {
    if (!this._audio) return;
    this._stopAnalysisTimer();
    this._playPromise = null;
    const audio = this._audio;
    this._isClosing = true;
    try {
      audio.pause();
      audio.removeAttribute("src");
      audio.load();
    } finally {
      this._isClosing = false;
      if (this._boundAudio === audio) this._boundAudio = null;
      this._audio = null;
    }
  }

  private _handleReplyPromise(replyPromise?: Promise<WasmReply>): void {
    if (!replyPromise) return;
    const backend = this._backend;
    void replyPromise
      .then((reply) => {
        if (backend && this._backend === backend) {
          this._handleReply(reply);
        }
      })
      .catch((err) => {
        if (backend && this._backend === backend) {
          console.warn("[audioIpc] WASM audio backend event failed", err);
        }
      });
  }

  private _handleAnalysisReplyPromise(replyPromise?: Promise<WasmReply>): void {
    if (!replyPromise) return;
    const analysisWorker = this._analysisWorker;
    void replyPromise
      .then((reply) => {
        if (analysisWorker && this._analysisWorker === analysisWorker) {
          this._handleReply(reply, false);
        }
      })
      .catch((err) => {
        if (analysisWorker && this._analysisWorker === analysisWorker) {
          console.warn("[audioIpc] WASM audio analysis control failed", err);
        }
      });
  }

  private _dispatch(event: AudioThreadEvent, seq?: number): void {
    for (const listener of this._listeners) {
      try {
        listener(event, seq);
      } catch {
        /* listener errors should not break dispatch */
      }
    }
  }

  private _newCallbackId(): string {
    if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
      return crypto.randomUUID();
    }
    return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  }

  private _formatMediaError(error: MediaError | null): string {
    if (!error) return "HTML media error";
    const names: Record<number, string> = {
      1: "MEDIA_ERR_ABORTED",
      2: "MEDIA_ERR_NETWORK",
      3: "MEDIA_ERR_DECODE",
      4: "MEDIA_ERR_SRC_NOT_SUPPORTED",
    };
    return names[error.code] ?? `MEDIA_ERR_${error.code}`;
  }

  private _nowMs(): number {
    return typeof performance !== "undefined" && typeof performance.now === "function"
      ? performance.now()
      : Date.now();
  }
}

let _tauriTransport: TauriHybridAudioIpc | null = null;
let _wasmTransport: WasmAudioBackendIpc | null = null;

export function getAudioBackendTransport(): AudioBackendTransport {
  if (isTauri()) {
    if (_tauriTransport === null) {
      _tauriTransport = new TauriHybridAudioIpc();
    }
    return _tauriTransport;
  }

  if (_wasmTransport === null) {
    _wasmTransport = new WasmAudioBackendIpc();
  }
  return _wasmTransport;
}
