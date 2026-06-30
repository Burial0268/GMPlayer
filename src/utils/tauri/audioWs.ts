/**
 * Split local-loopback WebSocket transport for the Rust audio backend.
 *
 * Events and controls intentionally use different sockets:
 * - event socket: Rust → frontend, high-rate FFT/status/position events;
 * - control socket: frontend → Rust, play/pause/seek/volume commands and
 *   Rust → frontend priority status/control events.
 *
 * This keeps playback controls off the event stream. Event traffic may be
 * coalesced or dropped under pressure; controls must stay hot and ordered by
 * the latest user intent.
 */
import type { AudioThreadEvent, AudioThreadEventMessage, AudioThreadMessage } from "./audioBridge";

type EventListener = (evt: AudioThreadEvent, seq?: number) => void;
type AudioWsUrls = { events: string; control: string };
type SeekAudioMessage = Extract<AudioThreadMessage, { type: "seekAudio" }>;

const CTRL_RESUME = 1;
const CTRL_PAUSE = 2;
const CTRL_TOGGLE = 3;
const CTRL_SEEK = 4;
const CTRL_SET_VOLUME = 5;
const CTRL_SET_VOLUME_RELATIVE = 6;

export class AudioWsClient {
  private _eventWs: WebSocket | null = null;
  private _controlWs: WebSocket | null = null;
  private _urls: AudioWsUrls | null = null;
  private _listeners: Set<EventListener> = new Set();

  private _eventConnectPromise: Promise<void> | null = null;
  private _controlConnectPromise: Promise<void> | null = null;
  private _eventReconnectAttempt = 0;
  private _controlReconnectAttempt = 0;
  private _eventReconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private _controlReconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private _shuttingDown = false;

  private _pendingOrderedCommands: AudioThreadMessage[] = [];
  private _pendingPlaybackState: boolean | null = null;
  private _pendingSeekCommand: SeekAudioMessage | null = null;
  private _pendingVolume: number | null = null;
  private _flushingPending = false;

  /** True when the control socket is open and ready to send. */
  isConnected(): boolean {
    return this.controlsConnected();
  }

  eventsConnected(): boolean {
    return this._eventWs !== null && this._eventWs.readyState === WebSocket.OPEN;
  }

  controlsConnected(): boolean {
    return this._controlWs !== null && this._controlWs.readyState === WebSocket.OPEN;
  }

  /** Resolve once both event and control sockets are OPEN. */
  async connect(): Promise<void> {
    await Promise.all([this.connectEvents(), this.connectControls()]);
  }

  connectEvents(): Promise<void> {
    if (this.eventsConnected()) return Promise.resolve();
    if (this._eventConnectPromise) return this._eventConnectPromise;
    this._eventConnectPromise = this._doConnectEvents().finally(() => {
      this._eventConnectPromise = null;
    });
    return this._eventConnectPromise;
  }

  connectControls(): Promise<void> {
    if (this.controlsConnected()) return Promise.resolve();
    if (this._controlConnectPromise) return this._controlConnectPromise;
    this._controlConnectPromise = this._doConnectControls().finally(() => {
      this._controlConnectPromise = null;
    });
    return this._controlConnectPromise;
  }

  private async _doConnectEvents(): Promise<void> {
    if (this._shuttingDown) throw new Error("AudioWsClient is shutting down");
    await this._ensureUrls();

    return new Promise((resolve, reject) => {
      let ws: WebSocket;
      try {
        ws = new WebSocket(this._urls!.events);
      } catch (e) {
        reject(e instanceof Error ? e : new Error(String(e)));
        return;
      }

      const onOpen = (): void => {
        ws.removeEventListener("open", onOpen);
        ws.removeEventListener("error", onError);
        this._eventWs = ws;
        this._eventReconnectAttempt = 0;
        ws.addEventListener("message", (e) => this._onMessage(e));
        ws.addEventListener("close", () => this._onEventClose(ws));
        ws.addEventListener("error", () => {
          /* close handles reconnect */
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
        reject(new Error("Event WebSocket failed to open"));
      };
      ws.addEventListener("open", onOpen);
      ws.addEventListener("error", onError);
    });
  }

  private async _doConnectControls(): Promise<void> {
    if (this._shuttingDown) throw new Error("AudioWsClient is shutting down");
    await this._ensureUrls();

    return new Promise((resolve, reject) => {
      let ws: WebSocket;
      try {
        ws = new WebSocket(this._urls!.control);
      } catch (e) {
        reject(e instanceof Error ? e : new Error(String(e)));
        return;
      }

      const onOpen = (): void => {
        ws.removeEventListener("open", onOpen);
        ws.removeEventListener("error", onError);
        this._controlWs = ws;
        this._controlReconnectAttempt = 0;
        ws.addEventListener("message", (e) => this._onMessage(e));
        ws.addEventListener("close", () => this._onControlClose(ws));
        ws.addEventListener("error", () => {
          /* close handles reconnect */
        });
        this._flushPendingControls();
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
        reject(new Error("Control WebSocket failed to open"));
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

  /** Send immediately over the control socket. Throws if it is not open. */
  send(msg: AudioThreadMessage): void {
    if (!this.controlsConnected()) {
      throw new Error("Control WebSocket not connected");
    }
    this._sendNow(msg);
  }

  /**
   * Send immediately when possible; otherwise keep only the latest realtime
   * intent and flush it as soon as the control socket reconnects.
   */
  sendOrQueue(msg: AudioThreadMessage): boolean {
    if (this.controlsConnected()) {
      try {
        this._sendNow(msg);
        return true;
      } catch {
        this._queueControl(msg);
        this._closeControlSocket();
      }
    } else {
      this._queueControl(msg);
    }

    void this.connectControls()
      .then(() => this._flushPendingControls())
      .catch((e) => {
        console.error("[audioWs] control reconnect failed", e);
      });
    return false;
  }

  /** Close both sockets and stop reconnecting. */
  shutdown(): void {
    this._shuttingDown = true;
    if (this._eventReconnectTimer !== null) {
      clearTimeout(this._eventReconnectTimer);
      this._eventReconnectTimer = null;
    }
    if (this._controlReconnectTimer !== null) {
      clearTimeout(this._controlReconnectTimer);
      this._controlReconnectTimer = null;
    }
    this._closeEventSocket();
    this._closeControlSocket();
  }

  private _sendNow(msg: AudioThreadMessage): void {
    const hotFrame = this._encodeHotControl(msg);
    if (hotFrame !== null) {
      this._controlWs!.send(hotFrame);
      return;
    }

    const envelope: AudioThreadEventMessage<AudioThreadMessage> = {
      callbackId: "",
      data: msg,
    };
    this._controlWs!.send(JSON.stringify(envelope));
  }

  private _encodeHotControl(msg: AudioThreadMessage): ArrayBuffer | null {
    switch (msg.type) {
      case "resumeAudio":
        return this._opcodeFrame(CTRL_RESUME);
      case "pauseAudio":
        return this._opcodeFrame(CTRL_PAUSE);
      case "resumeOrPauseAudio":
        return this._opcodeFrame(CTRL_TOGGLE);
      case "seekAudio":
        if (msg.requestId !== undefined || msg.expectedMusicId !== undefined) return null;
        return this._f64Frame(CTRL_SEEK, msg.position);
      case "setVolume":
        return this._f64Frame(CTRL_SET_VOLUME, msg.volume);
      case "setVolumeRelative":
        return this._f64Frame(CTRL_SET_VOLUME_RELATIVE, msg.volume);
      default:
        return null;
    }
  }

  private _opcodeFrame(opcode: number): ArrayBuffer {
    const buffer = new ArrayBuffer(1);
    new DataView(buffer).setUint8(0, opcode);
    return buffer;
  }

  private _f64Frame(opcode: number, value: number): ArrayBuffer {
    const buffer = new ArrayBuffer(9);
    const view = new DataView(buffer);
    view.setUint8(0, opcode);
    view.setFloat64(1, value, true);
    return buffer;
  }

  private _queueControl(msg: AudioThreadMessage): void {
    switch (msg.type) {
      case "resumeAudio":
        this._pendingPlaybackState = true;
        break;
      case "pauseAudio":
        this._pendingPlaybackState = false;
        break;
      case "seekAudio":
        this._pendingSeekCommand = msg;
        break;
      case "setVolume":
        this._pendingVolume = msg.volume;
        break;
      default:
        this._pendingOrderedCommands.push(msg);
        break;
    }
  }

  private _flushPendingControls(): void {
    if (this._flushingPending || !this.controlsConnected()) return;
    const messages = this._drainPendingControls();
    if (messages.length === 0) return;

    this._flushingPending = true;
    try {
      for (let i = 0; i < messages.length; i++) {
        try {
          this._sendNow(messages[i]);
        } catch (e) {
          for (let j = i; j < messages.length; j++) {
            this._queueControl(messages[j]);
          }
          this._closeControlSocket();
          void this.connectControls().catch((err) => {
            console.error("[audioWs] control reconnect failed during flush", err, e);
          });
          break;
        }
      }
    } finally {
      this._flushingPending = false;
    }
  }

  private _drainPendingControls(): AudioThreadMessage[] {
    const messages = this._pendingOrderedCommands.splice(0);
    const playbackState = this._pendingPlaybackState;
    const seekCommand = this._pendingSeekCommand;
    const volume = this._pendingVolume;

    this._pendingPlaybackState = null;
    this._pendingSeekCommand = null;
    this._pendingVolume = null;

    if (playbackState === false) {
      messages.push({ type: "pauseAudio" });
    }
    if (seekCommand !== null) {
      messages.push(seekCommand);
    }
    if (playbackState === true) {
      messages.push({ type: "resumeAudio" });
    }
    if (volume !== null) {
      messages.push({ type: "setVolume", volume });
    }

    return messages;
  }

  private _onMessage(e: MessageEvent): void {
    if (typeof e.data !== "string") return;

    let envelope: AudioThreadEventMessage<AudioThreadEvent>;
    try {
      envelope = JSON.parse(e.data);
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

  private _onEventClose(ws: WebSocket): void {
    if (this._eventWs !== ws) return;
    this._eventWs = null;
    if (this._shuttingDown) return;
    const delay = Math.min(5000, 250 * 2 ** this._eventReconnectAttempt);
    this._eventReconnectAttempt++;
    this._eventReconnectTimer = setTimeout(() => {
      this._eventReconnectTimer = null;
      this.connectEvents().catch(() => {
        /* next close/error schedules another attempt */
      });
    }, delay);
  }

  private _onControlClose(ws: WebSocket): void {
    if (this._controlWs !== ws) return;
    this._controlWs = null;
    if (this._shuttingDown) return;
    const delay = Math.min(1000, 50 * 2 ** this._controlReconnectAttempt);
    this._controlReconnectAttempt++;
    this._controlReconnectTimer = setTimeout(() => {
      this._controlReconnectTimer = null;
      this.connectControls()
        .then(() => this._flushPendingControls())
        .catch(() => {
          /* next close/error schedules another attempt */
        });
    }, delay);
  }

  private _closeEventSocket(): void {
    if (!this._eventWs) return;
    const ws = this._eventWs;
    this._eventWs = null;
    try {
      ws.close();
    } catch {
      /* ignore */
    }
  }

  private _closeControlSocket(): void {
    if (!this._controlWs) return;
    const ws = this._controlWs;
    this._controlWs = null;
    try {
      ws.close();
    } catch {
      /* ignore */
    }
  }

  private async _ensureUrls(): Promise<void> {
    if (this._urls) return;
    this._urls = await this._fetchWsUrls();
  }

  private async _fetchWsUrls(): Promise<AudioWsUrls> {
    if (!("__TAURI__" in window) || !window.__TAURI__) {
      throw new Error("Tauri runtime not available");
    }

    try {
      const urls = await window.__TAURI__.core.invoke<{
        events?: string;
        control?: string;
        eventUrl?: string;
        controlUrl?: string;
      } | null>("audio_get_ws_urls");
      const events = urls?.events ?? urls?.eventUrl;
      const control = urls?.control ?? urls?.controlUrl;
      if (events && control) {
        return { events, control };
      }
    } catch (e) {
      console.warn("[audioWs] audio_get_ws_urls invoke failed", e);
    }

    const url = await window.__TAURI__.core.invoke<string | null>("audio_get_ws_url");
    if (!url) {
      throw new Error("Audio backend did not expose WebSocket URLs");
    }
    return { events: url, control: url };
  }
}

let _singleton: AudioWsClient | null = null;

export function getAudioWs(): AudioWsClient {
  if (_singleton === null) {
    _singleton = new AudioWsClient();
  }
  return _singleton;
}
