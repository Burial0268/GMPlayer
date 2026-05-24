/**
 * Local-loopback WebSocket transport for the Rust audio backend.
 *
 * Why: Tauri IPC on Windows has unbounded queueing that adds 10–40 ms per
 * round-trip and back-pressures when the webview isn't focused (causing
 * the user-visible stutter / spectrum dropouts). A 127.0.0.1 WebSocket is
 * sub-millisecond and decoupled from the webview's event loop.
 *
 * Lifecycle: `getAudioWs()` returns a lazy singleton. The first call
 * invokes `audio_get_ws_url` to discover the port (chosen by the OS),
 * opens the socket, and starts reading. If the connection drops, an
 * exponential-backoff reconnect kicks in. Pending outbound commands are
 * NOT queued — callers should fall back to `audioSendMsg` (Tauri invoke)
 * when `isConnected()` returns false.
 */
import type { AudioThreadEvent, AudioThreadEventMessage, AudioThreadMessage } from "./audioBridge";

type EventListener = (evt: AudioThreadEvent, seq?: number) => void;

export class AudioWsClient {
  private _ws: WebSocket | null = null;
  private _wsUrl: string | null = null;
  private _listeners: Set<EventListener> = new Set();
  private _connectPromise: Promise<void> | null = null;
  private _reconnectAttempt = 0;
  private _reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private _shuttingDown = false;

  /** True when the socket is open and ready to send. */
  isConnected(): boolean {
    return this._ws !== null && this._ws.readyState === WebSocket.OPEN;
  }

  /**
   * Resolve once the socket is OPEN. Returns the same in-flight promise
   * if called concurrently. Caller MUST handle rejection by falling back
   * to the Tauri invoke path.
   */
  connect(): Promise<void> {
    if (this.isConnected()) return Promise.resolve();
    if (this._connectPromise) return this._connectPromise;
    this._connectPromise = this._doConnect().finally(() => {
      this._connectPromise = null;
    });
    return this._connectPromise;
  }

  private async _doConnect(): Promise<void> {
    if (this._shuttingDown) throw new Error("AudioWsClient is shutting down");

    if (!this._wsUrl) {
      // Discover the URL exactly once. The Rust side picks a free port at
      // startup; calling this multiple times returns the same URL.
      const url = await this._fetchWsUrl();
      if (!url) {
        throw new Error("Audio backend did not expose a WebSocket URL");
      }
      this._wsUrl = url;
    }

    return new Promise((resolve, reject) => {
      let ws: WebSocket;
      try {
        ws = new WebSocket(this._wsUrl!);
      } catch (e) {
        reject(e instanceof Error ? e : new Error(String(e)));
        return;
      }
      const onOpen = (): void => {
        ws.removeEventListener("open", onOpen);
        ws.removeEventListener("error", onError);
        this._ws = ws;
        this._reconnectAttempt = 0;
        ws.addEventListener("message", (e) => this._onMessage(e));
        ws.addEventListener("close", () => this._onClose());
        ws.addEventListener("error", () => {
          // After OPEN, errors are surfaced via close; nothing to do here.
        });
        resolve();
      };
      const onError = (): void => {
        ws.removeEventListener("open", onOpen);
        ws.removeEventListener("error", onError);
        try {
          ws.close();
        } catch {
          /* ignore */
        }
        reject(new Error("WebSocket failed to open"));
      };
      ws.addEventListener("open", onOpen);
      ws.addEventListener("error", onError);
    });
  }

  /** Subscribe to audio events. Returns an unsubscribe function. */
  subscribe(listener: EventListener): () => void {
    this._listeners.add(listener);
    return () => {
      this._listeners.delete(listener);
    };
  }

  /**
   * Send a command. Throws if the socket isn't open — callers MUST catch
   * and fall back to the Tauri invoke path. We deliberately don't queue:
   * a queue would silently delay commands during connection blips, and
   * the invoke fallback already guarantees no message is lost.
   */
  send(msg: AudioThreadMessage): void {
    if (!this.isConnected()) {
      throw new Error("WebSocket not connected");
    }
    const envelope: AudioThreadEventMessage<AudioThreadMessage> = {
      callbackId: this._newCallbackId(),
      data: msg,
    };
    this._ws!.send(JSON.stringify(envelope));
  }

  /** Close the socket and stop reconnecting. */
  shutdown(): void {
    this._shuttingDown = true;
    if (this._reconnectTimer !== null) {
      clearTimeout(this._reconnectTimer);
      this._reconnectTimer = null;
    }
    if (this._ws) {
      try {
        this._ws.close();
      } catch {
        /* ignore */
      }
      this._ws = null;
    }
  }

  private _onMessage(e: MessageEvent): void {
    let envelope: AudioThreadEventMessage<AudioThreadEvent>;
    try {
      envelope = JSON.parse(typeof e.data === "string" ? e.data : "");
    } catch {
      return;
    }
    if (!envelope || !envelope.data) return;
    const seq = envelope.seq;
    for (const fn of this._listeners) {
      try {
        fn(envelope.data, seq);
      } catch {
        /* listener errors don't abort the dispatch loop */
      }
    }
  }

  private _onClose(): void {
    this._ws = null;
    if (this._shuttingDown) return;
    // Exponential backoff capped at 5 s — the Rust server only stops on
    // process exit, so reconnects should succeed quickly unless the
    // backend itself crashed.
    const delay = Math.min(5000, 250 * 2 ** this._reconnectAttempt);
    this._reconnectAttempt++;
    this._reconnectTimer = setTimeout(() => {
      this._reconnectTimer = null;
      this.connect().catch(() => {
        // _onClose will fire again, scheduling the next retry.
      });
    }, delay);
  }

  private async _fetchWsUrl(): Promise<string | null> {
    if (!("__TAURI__" in window) || !window.__TAURI__) return null;
    try {
      const url = await window.__TAURI__.core.invoke<string | null>("audio_get_ws_url");
      return url ?? null;
    } catch (e) {
      console.warn("[audioWs] audio_get_ws_url invoke failed", e);
      return null;
    }
  }

  private _callbackCounter = 0;
  private _newCallbackId(): string {
    if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
      return crypto.randomUUID();
    }
    this._callbackCounter = (this._callbackCounter + 1) >>> 0;
    return `${Date.now()}-${this._callbackCounter}`;
  }
}

let _singleton: AudioWsClient | null = null;

export function getAudioWs(): AudioWsClient {
  if (_singleton === null) {
    _singleton = new AudioWsClient();
  }
  return _singleton;
}
