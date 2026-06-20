import type { AudioThreadEvent, AudioThreadEventMessage, AudioThreadMessage } from "./audioBridge";

interface WasmAudioBackendBinding {
  sendMessageJson(envelopeJson: string): string;
  loadAnalysisBytes(bytes: Uint8Array, extension: string, musicId: string): string;
  processAnalysisFrame(position: number, deltaMs: number): string;
  applyLoadedTrack(duration: number, sampleRate: number, channels: number, bitrate: number): string;
  applyPlaybackState(isPlaying: boolean): string;
  applyPlayPosition(position: number): string;
  applyPlaybackFinished(): string;
  applyVolume(volume: number): string;
  applyLoadError(error: string): string;
  applyPlayError(error: string): string;
  syncStatusJson(): string;
  stateJson(): string;
}

type WasmAudioBackendCtor = new () => WasmAudioBackendBinding;

interface WasmAudioBackendModule {
  WasmAudioBackend: WasmAudioBackendCtor;
}

interface WasmReply {
  events?: AudioThreadEventMessage<AudioThreadEvent>[];
  effects?: unknown[];
  error?: string;
}

type WorkerRequest =
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

type WorkerResponse =
  | { id: number; ok: true; reply?: WasmReply; stateJson?: string }
  | { id: number; ok: false; error: string };

let backend: WasmAudioBackendBinding | null = null;
let initPromise: Promise<WasmAudioBackendBinding> | null = null;

async function getBackend(): Promise<WasmAudioBackendBinding> {
  if (backend) return backend;
  if (!initPromise) {
    initPromise = import("@player-helper/gmplayer-audio-backend").then((mod) => {
      const wasmMod = mod as unknown as WasmAudioBackendModule;
      backend = new wasmMod.WasmAudioBackend();
      return backend;
    });
  }
  return initPromise;
}

function parseReply(replyJson: string): WasmReply {
  try {
    const reply = JSON.parse(replyJson) as WasmReply;
    return reply && typeof reply === "object" ? reply : {};
  } catch {
    return {};
  }
}

async function handleRequest(request: WorkerRequest): Promise<WorkerResponse> {
  if (request.type === "shutdown") {
    backend = null;
    initPromise = null;
    return { id: request.id, ok: true };
  }

  const wasmBackend = await getBackend();

  switch (request.type) {
    case "init":
      return { id: request.id, ok: true };
    case "sendMessage":
      return {
        id: request.id,
        ok: true,
        reply: parseReply(wasmBackend.sendMessageJson(JSON.stringify(request.envelope))),
      };
    case "loadAnalysisBytes":
      return {
        id: request.id,
        ok: true,
        reply: parseReply(
          wasmBackend.loadAnalysisBytes(request.bytes, request.extension, request.musicId),
        ),
      };
    case "processAnalysisFrame":
      return {
        id: request.id,
        ok: true,
        reply: parseReply(wasmBackend.processAnalysisFrame(request.position, request.deltaMs)),
      };
    case "applyLoadedTrack":
      return {
        id: request.id,
        ok: true,
        reply: parseReply(
          wasmBackend.applyLoadedTrack(
            request.duration,
            request.sampleRate,
            request.channels,
            request.bitrate,
          ),
        ),
      };
    case "applyPlaybackState":
      return {
        id: request.id,
        ok: true,
        reply: parseReply(wasmBackend.applyPlaybackState(request.isPlaying)),
      };
    case "applyPlayPosition":
      return {
        id: request.id,
        ok: true,
        reply: parseReply(wasmBackend.applyPlayPosition(request.position)),
      };
    case "applyPlaybackFinished":
      return {
        id: request.id,
        ok: true,
        reply: parseReply(wasmBackend.applyPlaybackFinished()),
      };
    case "applyVolume":
      return {
        id: request.id,
        ok: true,
        reply: parseReply(wasmBackend.applyVolume(request.volume)),
      };
    case "applyLoadError":
      return {
        id: request.id,
        ok: true,
        reply: parseReply(wasmBackend.applyLoadError(request.error)),
      };
    case "applyPlayError":
      return {
        id: request.id,
        ok: true,
        reply: parseReply(wasmBackend.applyPlayError(request.error)),
      };
    case "syncStatus":
      return {
        id: request.id,
        ok: true,
        reply: parseReply(wasmBackend.syncStatusJson()),
      };
    case "state":
      return { id: request.id, ok: true, stateJson: wasmBackend.stateJson() };
  }
}

self.onmessage = (event: MessageEvent<WorkerRequest>) => {
  const request = event.data;
  handleRequest(request)
    .then((response) => {
      (self as unknown as Worker).postMessage(response);
    })
    .catch((err) => {
      const response: WorkerResponse = {
        id: request.id,
        ok: false,
        error: err instanceof Error ? err.message : String(err),
      };
      (self as unknown as Worker).postMessage(response);
    });
};

export {};
