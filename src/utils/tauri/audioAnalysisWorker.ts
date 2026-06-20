import type {
  AudioThreadEvent,
  AudioThreadEventMessage,
  AudioThreadMessage,
  SongData,
} from "./audioBridge";

interface WasmAudioBackendBinding {
  sendMessageJson(envelopeJson: string): string;
  loadAnalysisBytes(bytes: Uint8Array, extension: string, musicId: string): string;
  processAnalysisFrame(position: number, deltaMs: number): string;
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

interface AnalysisFetchResult {
  bytes: Uint8Array;
  responseUrl: string;
}

type AnalysisWorkerRequest =
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

type AnalysisWorkerResponse =
  | { id: number; ok: true; reply?: WasmReply }
  | { id: number; ok: false; error: string };

let backend: WasmAudioBackendBinding | null = null;
let initPromise: Promise<WasmAudioBackendBinding> | null = null;
let abortController: AbortController | null = null;

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

function sendMessage(wasmBackend: WasmAudioBackendBinding, data: AudioThreadMessage): WasmReply {
  return parseReply(
    wasmBackend.sendMessageJson(
      JSON.stringify({
        callbackId: "",
        data,
      }),
    ),
  );
}

async function loadAnalysis(request: Extract<AnalysisWorkerRequest, { type: "load" }>) {
  const wasmBackend = await getBackend();

  const song: SongData = {
    type: "local",
    filePath: request.src,
    origOrder: request.currentPlayIndex,
  };

  sendMessage(wasmBackend, { type: "setPlaylist", songs: [song] });
  sendMessage(
    wasmBackend,
    request.initialPosition > 0
      ? { type: "jumpToSongAt", songIndex: 0, position: request.initialPosition }
      : { type: "jumpToSong", songIndex: 0 },
  );

  abortController?.abort();
  abortController = typeof AbortController !== "undefined" ? new AbortController() : null;

  const result = await fetchAnalysisBytes(request.src, abortController?.signal);
  const extension = extensionFromSrc(result.responseUrl) || extensionFromSrc(request.src);
  return parseReply(wasmBackend.loadAnalysisBytes(result.bytes, extension, request.musicId));
}

async function handleRequest(request: AnalysisWorkerRequest): Promise<AnalysisWorkerResponse> {
  if (request.type === "shutdown") {
    abortController?.abort();
    abortController = null;
    backend = null;
    initPromise = null;
    return { id: request.id, ok: true };
  }

  const wasmBackend = await getBackend();

  switch (request.type) {
    case "load":
      return { id: request.id, ok: true, reply: await loadAnalysis(request) };
    case "process":
      return {
        id: request.id,
        ok: true,
        reply: parseReply(wasmBackend.processAnalysisFrame(request.position, request.deltaMs)),
      };
    case "setFreqRange":
      return {
        id: request.id,
        ok: true,
        reply: sendMessage(wasmBackend, {
          type: "setFFTRange",
          fromFreq: request.fromFreq,
          toFreq: request.toFreq,
        }),
      };
    case "setFFT":
      return {
        id: request.id,
        ok: true,
        reply: sendMessage(wasmBackend, { type: "setFFT", enabled: request.enabled }),
      };
  }
}

async function fetchAnalysisBytes(src: string, signal?: AbortSignal): Promise<AnalysisFetchResult> {
  let lastError: unknown = null;
  for (const url of [src, analysisProxyUrl(src)]) {
    if (!url) continue;
    const attempts = buildAnalysisFetchAttempts(url);
    for (const attempt of attempts) {
      try {
        const response = await fetch(url, { ...attempt.init, signal });
        if (!response.ok) {
          lastError = new Error(
            `HTTP ${response.status} ${response.statusText || ""}`.trim() +
              ` (${attempt.label}, ${url === src ? "direct" : "proxy"})`,
          );
          continue;
        }

        const buffer = await response.arrayBuffer();
        if (buffer.byteLength === 0) {
          lastError = new Error(
            `empty response (${attempt.label}, ${url === src ? "direct" : "proxy"})`,
          );
          continue;
        }

        return {
          bytes: new Uint8Array(buffer),
          responseUrl: response.headers.get("x-audio-source-url") || response.url || src,
        };
      } catch (err) {
        if (err instanceof Error && err.name === "AbortError") throw err;
        lastError = err;
      }
    }
  }

  const detail = lastError instanceof Error ? lastError.message : String(lastError);
  throw new Error(`unable to fetch analysis audio bytes: ${detail}; url=${src}`);
}

function buildAnalysisFetchAttempts(url: string): Array<{ label: string; init: RequestInit }> {
  if (isSameOriginUrl(url)) {
    return [
      {
        label: "range+credentials",
        init: { credentials: "include", headers: { Range: "bytes=0-" } },
      },
      {
        label: "range",
        init: { credentials: "same-origin", headers: { Range: "bytes=0-" } },
      },
      { label: "credentials", init: { credentials: "include" } },
      { label: "default", init: { credentials: "same-origin" } },
    ];
  }

  // Netease media URLs commonly return `Access-Control-Allow-Origin: *`.
  // Cross-origin credentialed fetches are rejected for wildcard ACAO, and the
  // playback element does not need cookies, so never include credentials here.
  return [
    {
      label: "range",
      init: { credentials: "omit", headers: { Range: "bytes=0-" } },
    },
    { label: "default", init: { credentials: "omit" } },
  ];
}

function isSameOriginUrl(src: string): boolean {
  try {
    return new URL(src, self.location.href).origin === self.location.origin;
  } catch {
    return false;
  }
}

function analysisProxyUrl(src: string): string | null {
  try {
    const url = new URL(src, self.location.href);
    if (url.protocol !== "http:" && url.protocol !== "https:") return null;
    if (url.origin === self.location.origin) return null;
    return `/api/audio-proxy?url=${encodeURIComponent(url.href)}`;
  } catch {
    return null;
  }
}

function extensionFromSrc(src: string): string {
  const withoutHash = src.split("#", 1)[0] ?? src;
  const withoutQuery = withoutHash.split("?", 1)[0] ?? withoutHash;
  const name = withoutQuery.split(/[\\/]/).pop() ?? "";
  const dot = name.lastIndexOf(".");
  return dot >= 0 ? name.slice(dot + 1).toLowerCase() : "";
}

self.onmessage = (event: MessageEvent<AnalysisWorkerRequest>) => {
  const request = event.data;
  handleRequest(request)
    .then((response) => {
      (self as unknown as Worker).postMessage(response);
    })
    .catch((err) => {
      const response: AnalysisWorkerResponse = {
        id: request.id,
        ok: false,
        error: err instanceof Error ? err.message : String(err),
      };
      (self as unknown as Worker).postMessage(response);
    });
};

export {};
