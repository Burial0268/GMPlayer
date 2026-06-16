import type {
  AudioThreadEvent,
  AudioThreadEventCallback,
  AudioThreadEventMessage,
  AudioThreadMessage,
} from "./audioBridge";
import { audioSendMsg, isTauri, listenPlayerEvents } from "./audioBridge";
import { getAudioWs } from "./audioWs";
import { registerPCMCaptureWorklet } from "../AudioContext/pcm-capture-worklet";

const WASM_ANALYSIS_INTERVAL_MS = 33;
const LOW_FREQ_MIN_HZ = 70;
const LOW_FREQ_MAX_HZ = 2000;
const LOW_FREQ_GAIN = 3.2;
const LOW_FREQ_ATTACK_MS = 35;
const LOW_FREQ_RELEASE_MS = 160;
const LOW_FREQ_EMIT_INTERVAL_MS = 33;

export interface AudioBackendTransport {
  connect(): Promise<void>;
  subscribe(listener: AudioThreadEventCallback): () => void;
  sendOrQueue(msg: AudioThreadMessage): boolean;
  getGainNode?(): GainNode | null;
  ensureAudioGraph?(): Promise<boolean>;
  shutdown?(): void;
}

export function isWasmAudioBackendAvailable(): boolean {
  return typeof window !== "undefined" && typeof Audio !== "undefined" && typeof Worker !== "undefined";
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
      console.warn("[audioIpc] WebSocket audio transport unavailable, falling back to Tauri IPC", err);
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
  | { type: "close" };

interface WasmReply {
  events?: AudioThreadEventMessage<AudioThreadEvent>[];
  effects?: WasmEffect[];
  error?: string;
}

class WebLowFrequencyAnalyzer {
  private _sampleRate = 44100;
  private _channelIndex = 0;
  private _frameSum = 0;
  private _prevInput = 0;
  private _highpass = 0;
  private _band = 0;
  private _envelope = 0;
  private _highpassAlpha = 0;
  private _lowpassAlpha = 0;
  private _attackAlpha = 0;
  private _releaseAlpha = 0;

  constructor(sampleRate: number) {
    this.setSampleRate(sampleRate);
  }

  get envelope(): number {
    return this._envelope;
  }

  setSampleRate(sampleRate: number): void {
    const rate = Number.isFinite(sampleRate) ? Math.max(1, sampleRate) : 44100;
    if (rate === this._sampleRate && this._highpassAlpha > 0) return;

    this._sampleRate = rate;
    this._highpassAlpha = Math.exp((-2 * Math.PI * LOW_FREQ_MIN_HZ) / rate);
    this._lowpassAlpha = 1 - Math.exp((-2 * Math.PI * LOW_FREQ_MAX_HZ) / rate);
    this._attackAlpha = 1 - Math.exp(-1000 / (LOW_FREQ_ATTACK_MS * rate));
    this._releaseAlpha = 1 - Math.exp(-1000 / (LOW_FREQ_RELEASE_MS * rate));
  }

  pushMono(samples: Float32Array): number {
    for (let i = 0; i < samples.length; i++) {
      this._pushSample(samples[i] || 0, 1);
    }
    return this._envelope;
  }

  pushSilence(frameCount: number): number {
    const count = Math.max(0, Math.floor(frameCount));
    for (let i = 0; i < count; i++) {
      this._pushSample(0, 1);
    }
    return this._envelope;
  }

  reset(): void {
    this._channelIndex = 0;
    this._frameSum = 0;
    this._prevInput = 0;
    this._highpass = 0;
    this._band = 0;
    this._envelope = 0;
  }

  private _pushSample(sample: number, channels: number): void {
    this._frameSum += Number.isFinite(sample) ? sample : 0;
    this._channelIndex += 1;
    if (this._channelIndex < channels) return;

    const mono = this._frameSum / channels;
    this._frameSum = 0;
    this._channelIndex = 0;

    this._highpass = this._highpassAlpha * (this._highpass + mono - this._prevInput);
    this._prevInput = mono;
    this._band += (this._highpass - this._band) * this._lowpassAlpha;

    let target = Math.min(1, Math.max(0, Math.abs(this._band) * LOW_FREQ_GAIN));
    if (target < 0.01) target = 0;
    const alpha = target > this._envelope ? this._attackAlpha : this._releaseAlpha;
    this._envelope += (target - this._envelope) * alpha;
    this._envelope = Math.min(1, Math.max(0, this._envelope));
  }
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

  loadAnalysisBytes(
    bytes: Uint8Array,
    extension: string,
    musicId: string,
  ): Promise<WasmReply> {
    return this._callReply(
      { type: "loadAnalysisBytes", bytes, extension, musicId },
      [bytes.buffer],
    );
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

  private async _callReply(
    request: WasmAnalysisWorkerRequestPayload,
  ): Promise<WasmReply> {
    const value = await this._call(request);
    return value && typeof value === "object" ? value : {};
  }

  private _call(
    request: WasmAnalysisWorkerRequestPayload,
  ): Promise<WasmReply | undefined> {
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
  private _audioContext: AudioContext | null = null;
  private _mediaSource: MediaElementAudioSourceNode | null = null;
  private _gainNode: GainNode | null = null;
  private _lowFreqWorkletNode: AudioWorkletNode | null = null;
  private _lowFreqWorkletSink: GainNode | null = null;
  private _lowFreqAnalyzer: WebLowFrequencyAnalyzer | null = null;
  private _playbackGraphPromise: Promise<boolean> | null = null;
  private _lowFreqSilenceTimer: ReturnType<typeof setInterval> | null = null;
  private _lastLowFreqEmitAt = 0;
  private _playbackGraphReady = false;
  private _lowFreqGraphReady = false;
  private _lowFreqWorkletUnavailable = false;
  private _syncingElementVolume = false;
  private _positionTimer: ReturnType<typeof setInterval> | null = null;
  private _analysisTimer: ReturnType<typeof setInterval> | null = null;
  private _analysisFrameInFlight = false;
  private _analysisFftEnabled = true;
  private _analysisFreqRange: { fromFreq: number; toFreq: number } | null = null;
  private _analysisGeneration = 0;
  private _analysisReady = false;
  private _lastAnalysisAt = 0;
  private _currentVolume = 1;
  private _isClosing = false;

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
    return this._gainNode;
  }

  async ensureAudioGraph(): Promise<boolean> {
    const audio = this._audio;
    if (!audio) return false;
    return this._ensurePlaybackGraph(audio);
  }

  shutdown(): void {
    this._cancelAnalysis();
    this._stopPositionTimer();
    this._releaseAudio();
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
        if (event.data.type === "lowFrequencyVolume" && this._lowFreqGraphReady) {
          continue;
        }
        this._dispatch(event.data, preserveSeq ? event.seq : undefined);
      }
    }
    for (const effect of reply.effects ?? []) {
      this._applyEffect(effect);
    }
  }

  private _mirrorAnalysisControl(msg: AudioThreadMessage): void {
    switch (msg.type) {
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
        this._audio?.pause();
        this._stopAnalysisTimer();
        this._startLowFreqSilenceDecay();
        break;
      case "seek":
        this._seek(effect.position);
        break;
      case "setVolume":
        this._currentVolume = Math.max(0, Math.min(1, effect.volume));
        this._applyOutputVolume();
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

    const audio = new Audio();
    audio.crossOrigin = "anonymous";
    audio.preload = "auto";
    audio.src = effect.src;
    this._setElementVolumeForCurrentGraph(audio);
    this._audio = audio;
    this._bindAudio(audio, effect.initialPosition);
    audio.load();
    void this._ensurePlaybackGraph(audio);

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
      this._stopLowFreqSilenceDecay();
      void this._ensurePlaybackGraph(audio);
      this._handleReplyPromise(this._backend?.applyPlaybackState(true));
      this._startPositionTimer();
      this._startAnalysisTimer();
    });

    audio.addEventListener("pause", () => {
      if (this._audio !== audio || this._isClosing || audio.ended) return;
      this._stopAnalysisTimer();
      this._startLowFreqSilenceDecay();
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
      if (this._playbackGraphReady) {
        this._setElementVolumeForCurrentGraph(audio);
      } else {
        this._currentVolume = audio.volume;
        this._handleReplyPromise(this._backend?.applyVolume(audio.volume));
      }
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
    void this._ensurePlaybackGraph(audio)
      .then(() => audio.play())
      .catch((err) => {
        const message = err instanceof Error ? err.message : String(err);
        this._handleReplyPromise(this._backend?.applyPlayError(message));
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

  private async _ensurePlaybackGraph(audio: HTMLAudioElement): Promise<boolean> {
    if (this._audio !== audio) return false;
    if (this._playbackGraphReady) {
      await this._resumeAudioContext();
      if (!this._lowFreqGraphReady) {
        await this._ensureLowFreqWorklet(audio);
      }
      return true;
    }
    if (typeof AudioContext === "undefined") return false;
    if (this._playbackGraphPromise) return this._playbackGraphPromise;

    this._playbackGraphPromise = this._createPlaybackGraph(audio).finally(() => {
      this._playbackGraphPromise = null;
    });
    return this._playbackGraphPromise;
  }

  private async _createPlaybackGraph(audio: HTMLAudioElement): Promise<boolean> {
    try {
      const ctx = this._audioContext ?? new AudioContext();
      this._audioContext = ctx;
      await this._resumeAudioContext();
      if (this._audio !== audio) return false;

      const source = this._mediaSource ?? ctx.createMediaElementSource(audio);
      this._mediaSource = source;

      const gain = this._gainNode ?? ctx.createGain();
      this._gainNode = gain;
      gain.gain.value = this._currentVolume;

      try {
        source.disconnect();
      } catch {
        /* node may not be connected yet */
      }
      source.connect(gain);
      gain.connect(ctx.destination);

      this._playbackGraphReady = true;
      this._setElementVolumeForCurrentGraph(audio);
      this._lowFreqAnalyzer = new WebLowFrequencyAnalyzer(ctx.sampleRate);
      await this._ensureLowFreqWorklet(audio);
      return true;
    } catch (err) {
      console.warn("[audioIpc] Web audio playback graph unavailable", err);
      this._teardownPlaybackGraph();
      this._setElementVolumeForCurrentGraph(audio);
      return false;
    }
  }

  private async _ensureLowFreqWorklet(audio: HTMLAudioElement): Promise<void> {
    const ctx = this._audioContext;
    const source = this._mediaSource;
    if (
      !ctx ||
      !source ||
      this._lowFreqWorkletNode ||
      this._lowFreqGraphReady ||
      this._lowFreqWorkletUnavailable
    ) {
      return;
    }

    try {
      await registerPCMCaptureWorklet(ctx);
      const worklet = new AudioWorkletNode(ctx, "pcm-capture-processor");
      if (this._audio !== audio || this._audioContext !== ctx || this._mediaSource !== source) {
        try {
          worklet.disconnect();
        } catch {
          /* already disconnected */
        }
        return;
      }
      worklet.port.onmessage = (event: MessageEvent<Float32Array>) => {
        if (this._audio !== audio || audio.paused || audio.ended) return;
        this._stopLowFreqSilenceDecay();
        const samples = event.data;
        if (!(samples instanceof Float32Array)) return;
        this._lowFreqAnalyzer?.pushMono(samples);
        this._emitLowFreq(false);
      };
      source.connect(worklet);
      const sink = ctx.createGain();
      sink.gain.value = 0;
      worklet.connect(sink);
      sink.connect(ctx.destination);
      this._lowFreqWorkletNode = worklet;
      this._lowFreqWorkletSink = sink;

      this._lowFreqGraphReady = true;
    } catch (err) {
      console.warn("[audioIpc] Web low-frequency audio graph unavailable", err);
      this._lowFreqGraphReady = false;
      this._lowFreqWorkletUnavailable = true;
    }
  }

  private async _resumeAudioContext(): Promise<void> {
    const ctx = this._audioContext;
    if (!ctx || ctx.state !== "suspended") return;
    try {
      await ctx.resume();
    } catch {
      /* resume can be rejected outside a user gesture */
    }
  }

  private _applyOutputVolume(): void {
    if (this._gainNode) {
      this._gainNode.gain.value = this._currentVolume;
    }
    if (this._audio) {
      this._setElementVolumeForCurrentGraph(this._audio);
    }
  }

  private _setElementVolumeForCurrentGraph(audio: HTMLAudioElement): void {
    const nextVolume = this._playbackGraphReady ? 1 : this._currentVolume;
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

  private _emitLowFreq(force: boolean): void {
    const analyzer = this._lowFreqAnalyzer;
    if (!analyzer) return;

    const now = this._nowMs();
    if (!force && now - this._lastLowFreqEmitAt < LOW_FREQ_EMIT_INTERVAL_MS) return;
    this._lastLowFreqEmitAt = now;
    this._dispatch({
      type: "lowFrequencyVolume",
      data: { volume: analyzer.envelope },
    });
  }

  private _resetLowFreq(): void {
    this._stopLowFreqSilenceDecay();
    this._lowFreqAnalyzer?.reset();
    this._lastLowFreqEmitAt = 0;
    this._dispatch({
      type: "lowFrequencyVolume",
      data: { volume: 0 },
    });
  }

  private _startLowFreqSilenceDecay(): void {
    const analyzer = this._lowFreqAnalyzer;
    const ctx = this._audioContext;
    if (!analyzer || !ctx || this._lowFreqSilenceTimer !== null) return;

    this._lowFreqSilenceTimer = setInterval(() => {
      const frames = Math.ceil((ctx.sampleRate * LOW_FREQ_EMIT_INTERVAL_MS) / 1000);
      analyzer.pushSilence(frames);
      this._emitLowFreq(true);
      if (analyzer.envelope <= 0.0005) {
        this._stopLowFreqSilenceDecay();
      }
    }, LOW_FREQ_EMIT_INTERVAL_MS);
  }

  private _stopLowFreqSilenceDecay(): void {
    if (this._lowFreqSilenceTimer !== null) {
      clearInterval(this._lowFreqSilenceTimer);
      this._lowFreqSilenceTimer = null;
    }
  }

  private _teardownPlaybackGraph(): void {
    this._playbackGraphPromise = null;
    this._stopLowFreqSilenceDecay();
    if (this._lowFreqWorkletNode) {
      try {
        this._lowFreqWorkletNode.port.onmessage = null;
        this._lowFreqWorkletNode.disconnect();
      } catch {
        /* already disconnected */
      }
      this._lowFreqWorkletNode = null;
    }
    if (this._lowFreqWorkletSink) {
      try {
        this._lowFreqWorkletSink.disconnect();
      } catch {
        /* already disconnected */
      }
      this._lowFreqWorkletSink = null;
    }
    if (this._mediaSource) {
      try {
        this._mediaSource.disconnect();
      } catch {
        /* already disconnected */
      }
      this._mediaSource = null;
    }
    if (this._gainNode) {
      try {
        this._gainNode.disconnect();
      } catch {
        /* already disconnected */
      }
      this._gainNode = null;
    }
    this._lowFreqAnalyzer = null;
    this._playbackGraphReady = false;
    this._lowFreqGraphReady = false;
    this._lowFreqWorkletUnavailable = false;
    this._lastLowFreqEmitAt = 0;
  }

  private _startPositionTimer(): void {
    if (this._positionTimer !== null) return;
    this._positionTimer = setInterval(() => {
      if (!this._audio || this._audio.paused || this._audio.ended) return;
      this._handleReplyPromise(this._backend?.applyPlayPosition(this._audio.currentTime));
    }, 1000);
  }

  private _stopPositionTimer(): void {
    if (this._positionTimer !== null) {
      clearInterval(this._positionTimer);
      this._positionTimer = null;
    }
  }

  private _startAnalysisTimer(): void {
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
    this._teardownPlaybackGraph();
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
