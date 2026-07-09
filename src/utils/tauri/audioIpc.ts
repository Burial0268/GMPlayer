import type { AudioThreadEventCallback, AudioThreadMessage } from "./audioBridge";
import { isTauri } from "./audioBridge";
import { createTauriAudioBackendTransport } from "./audioIpcTauri";
import type { AudioBackendTransport } from "./audioIpcTypes";

export type { AudioBackendTransport } from "./audioIpcTypes";

declare const __GMPLAYER_TAURI_BUILD__: boolean;

export function isWasmAudioBackendAvailable(): boolean {
  if (__GMPLAYER_TAURI_BUILD__ || isTauri()) return false;
  return (
    typeof window !== "undefined" && typeof Audio !== "undefined" && typeof Worker !== "undefined"
  );
}

class LazyWasmAudioBackendTransport implements AudioBackendTransport {
  private _impl: AudioBackendTransport | null = null;
  private _loadPromise: Promise<AudioBackendTransport> | null = null;
  private _listeners: Set<AudioThreadEventCallback> = new Set();
  private _listenerUnsubscribers = new Map<AudioThreadEventCallback, () => void>();
  private _shutdown = false;

  connect(): Promise<void> {
    return this._load().then((impl) => impl.connect());
  }

  subscribe(listener: AudioThreadEventCallback): () => void {
    if (this._shutdown) return () => {};

    this._listeners.add(listener);
    this._bindListener(listener);

    return () => {
      this._listeners.delete(listener);
      this._listenerUnsubscribers.get(listener)?.();
      this._listenerUnsubscribers.delete(listener);
    };
  }

  sendOrQueue(msg: AudioThreadMessage): boolean {
    if (this._impl) return this._impl.sendOrQueue(msg);
    void this._load().then((impl) => impl.sendOrQueue(msg));
    return false;
  }

  getGainNode(): GainNode | null {
    return this._impl?.getGainNode?.() ?? null;
  }

  ensureAudioGraph(): Promise<boolean> {
    return this._load().then((impl) => impl.ensureAudioGraph?.() ?? false);
  }

  shutdown(): void {
    this._shutdown = true;
    for (const unsubscribe of this._listenerUnsubscribers.values()) {
      unsubscribe();
    }
    this._listenerUnsubscribers.clear();
    this._listeners.clear();
    this._impl?.shutdown?.();
    this._impl = null;
    this._loadPromise = null;
  }

  private _load(): Promise<AudioBackendTransport> {
    if (this._shutdown) {
      return Promise.reject(new Error("WASM audio backend transport is shut down"));
    }
    if (this._impl) return Promise.resolve(this._impl);
    if (!this._loadPromise) {
      this._loadPromise = import("./audioIpcWasm").then((mod) => {
        const impl = mod.createWasmAudioBackendTransport();
        if (this._shutdown) {
          impl.shutdown?.();
          throw new Error("WASM audio backend transport is shut down");
        }
        this._impl = impl;
        for (const listener of this._listeners) {
          this._bindListener(listener);
        }
        return impl;
      });
    }
    return this._loadPromise;
  }

  private _bindListener(listener: AudioThreadEventCallback): void {
    if (!this._impl || this._listenerUnsubscribers.has(listener)) return;
    this._listenerUnsubscribers.set(listener, this._impl.subscribe(listener));
  }
}

let _tauriTransport: AudioBackendTransport | null = null;
let _wasmTransport: LazyWasmAudioBackendTransport | null = null;

function getTauriTransport(): AudioBackendTransport {
  if (_tauriTransport === null) {
    _tauriTransport = createTauriAudioBackendTransport();
  }
  return _tauriTransport;
}

function getLazyWasmTransport(): AudioBackendTransport {
  if (_wasmTransport === null) {
    _wasmTransport = new LazyWasmAudioBackendTransport();
  }
  return _wasmTransport;
}

export function getAudioBackendTransport(): AudioBackendTransport {
  if (__GMPLAYER_TAURI_BUILD__ || isTauri()) return getTauriTransport();
  return getLazyWasmTransport();
}
