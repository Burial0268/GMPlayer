import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  AudioThreadEvent,
  AudioThreadEventCallback,
  AudioThreadEventMessage,
  AudioThreadMessage,
} from "./audioBridge";
import { audioSendMsg, listenPlayerEvents } from "./audioBridge";
import type { AudioBackendTransport } from "./audioIpcTypes";

class TauriInvokeAudioIpc implements AudioBackendTransport {
  private _listeners: Set<AudioThreadEventCallback> = new Set();
  private _unlisten: (() => void) | null = null;
  private _connectPromise: Promise<void> | null = null;
  private _visualFrame: number | null = null;
  private _pendingFft: { event: AudioThreadEvent; seq?: number } | null = null;
  private _pendingLowFreq: { event: AudioThreadEvent; seq?: number } | null = null;

  connect(): Promise<void> {
    if (this._unlisten) return Promise.resolve();
    if (this._connectPromise) return this._connectPromise;

    this._connectPromise = listenPlayerEvents((event, seq) => {
      this._dispatchOrCoalesce(event, seq);
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
    if (this._visualFrame !== null) {
      cancelAnimationFrame(this._visualFrame);
      this._visualFrame = null;
    }
    this._pendingFft = null;
    this._pendingLowFreq = null;
    if (this._unlisten) {
      this._unlisten();
      this._unlisten = null;
    }
    this._listeners.clear();
  }

  private _dispatchOrCoalesce(event: AudioThreadEvent, seq?: number): void {
    if (event.type === "fftData") {
      this._pendingFft = { event, seq };
      this._scheduleVisualDispatch();
      return;
    }
    if (event.type === "lowFrequencyVolume") {
      this._pendingLowFreq = { event, seq };
      this._scheduleVisualDispatch();
      return;
    }
    this._dispatch(event, seq);
  }

  private _scheduleVisualDispatch(): void {
    if (this._visualFrame !== null) return;
    this._visualFrame = requestAnimationFrame(() => {
      this._visualFrame = null;
      const fft = this._pendingFft;
      const lowFreq = this._pendingLowFreq;
      this._pendingFft = null;
      this._pendingLowFreq = null;
      if (fft) this._dispatch(fft.event, fft.seq);
      if (lowFreq) this._dispatch(lowFreq.event, lowFreq.seq);
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
}

/**
 * Primary Tauri transport: a `Channel` carries the Rust -> frontend event
 * stream (FFT / status / position); control commands go out over the existing
 * `audio_send_msg` invoke. The Channel rides the webview's native IPC, so it
 * behaves identically on WebView2 / WebKitGTK / WKWebView / Android WebView.
 *
 * `connect()` rejects if `audio_subscribe_events` fails so the hybrid can fall
 * back to `TauriInvokeAudioIpc` (global `audio-player://event` emit), which the
 * Rust forwarder targets whenever no channel is registered.
 */
class TauriChannelAudioIpc implements AudioBackendTransport {
  private _listeners: Set<AudioThreadEventCallback> = new Set();
  private _channel: Channel<AudioThreadEventMessage<AudioThreadEvent>> | null = null;
  private _connectPromise: Promise<void> | null = null;
  private _visualFrame: number | null = null;
  private _pendingFft: { event: AudioThreadEvent; seq?: number } | null = null;
  private _pendingLowFreq: { event: AudioThreadEvent; seq?: number } | null = null;

  connect(): Promise<void> {
    if (this._channel) return Promise.resolve();
    if (this._connectPromise) return this._connectPromise;

    const channel = new Channel<AudioThreadEventMessage<AudioThreadEvent>>();
    channel.onmessage = (envelope) => {
      if (envelope && envelope.data) {
        this._dispatchOrCoalesce(envelope.data, envelope.seq);
      }
    };

    this._connectPromise = invoke<void>("audio_subscribe_events", { channel })
      .then(() => {
        this._channel = channel;
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
    if (this._visualFrame !== null) {
      cancelAnimationFrame(this._visualFrame);
      this._visualFrame = null;
    }
    this._pendingFft = null;
    this._pendingLowFreq = null;
    this._channel = null;
    this._listeners.clear();
  }

  private _dispatchOrCoalesce(event: AudioThreadEvent, seq?: number): void {
    if (event.type === "fftData") {
      this._pendingFft = { event, seq };
      this._scheduleVisualDispatch();
      return;
    }
    if (event.type === "lowFrequencyVolume") {
      this._pendingLowFreq = { event, seq };
      this._scheduleVisualDispatch();
      return;
    }
    this._dispatch(event, seq);
  }

  private _scheduleVisualDispatch(): void {
    if (this._visualFrame !== null) return;
    this._visualFrame = requestAnimationFrame(() => {
      this._visualFrame = null;
      const fft = this._pendingFft;
      const lowFreq = this._pendingLowFreq;
      this._pendingFft = null;
      this._pendingLowFreq = null;
      if (fft) this._dispatch(fft.event, fft.seq);
      if (lowFreq) this._dispatch(lowFreq.event, lowFreq.seq);
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
}

class TauriHybridAudioIpc implements AudioBackendTransport {
  private _active: AudioBackendTransport | null = null;
  private _fallback: TauriInvokeAudioIpc | null = null;

  async connect(): Promise<void> {
    if (this._active) {
      await this._active.connect();
      return;
    }

    const channel = new TauriChannelAudioIpc();
    try {
      await channel.connect();
      this._active = channel;
      return;
    } catch (err) {
      console.warn(
        "[audioIpc] Tauri Channel transport unavailable, falling back to global events",
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

export function createTauriAudioBackendTransport(): AudioBackendTransport {
  return new TauriHybridAudioIpc();
}
