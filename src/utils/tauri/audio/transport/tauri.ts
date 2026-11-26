import { Channel, invoke } from "@tauri-apps/api/core";

import { logNative } from "../../core/nativeLog";
import type {
  AudioThreadEvent,
  AudioThreadEventCallback,
  AudioThreadMessage,
  EventEnvelope,
  EventMeta,
} from "../protocol";
import { sendAudioCommand } from "../bridge";
import { PendingTicket } from "./ticket";
import type { AudioBackendTransport, CommandTicket } from "./types";

/**
 * The Tauri transport: one `Channel` carries the backend's event stream in,
 * `audio_send_msg` carries numbered commands out.
 *
 * Subscribing is a handshake. Its first message is a `hello` holding the
 * session snapshot, numbered in the stream, and everything after it is newer.
 * That gives the three rules this class enforces for every consumer:
 *
 * - **No stale overwrite.** An event numbered at or below the newest `hello`
 *   is already in it and is dropped. Without this a snapshot read beside the
 *   stream could be older than an event a subscriber had already applied.
 * - **No gap.** A page reload or `resync()` subscribes again and gets a fresh
 *   `hello`, so the events the old page missed are folded into the snapshot.
 * - **Commands are numbered above what the backend has seen.** Numbering starts
 *   after the `hello`'s `ack`, so a reloaded page can never be mistaken for one
 *   whose commands were already processed. Commands sent before the first
 *   `hello` wait for it; `CommandTicket` reads acknowledgement off later events.
 *
 * Analysis frames (`seq` 0) carry no state: they skip the ordering and are
 * coalesced to one per display frame.
 */
class TauriAudioBackendTransport implements AudioBackendTransport {
  private _listeners = new Set<AudioThreadEventCallback>();
  /** `seq` of the newest `hello`; anything at or below it is covered by it. */
  private _fence = 0;
  private _lastSeq = 0;
  /** Next command number; 0 until the first `hello` says where to start. */
  private _nextCmd = 0;
  private _queued: { msg: AudioThreadMessage; ticket: PendingTicket }[] = [];
  private _helloWaiters = new Set<() => void>();
  private _connectPromise: Promise<void> | null = null;
  private _connected = false;
  private _shutdown = false;
  private _visualFrame: number | null = null;
  private _pendingAnalysis: EventEnvelope | null = null;

  connect(): Promise<void> {
    if (this._connected) return Promise.resolve();
    this._connectPromise ??= this._subscribe().finally(() => {
      this._connectPromise = null;
    });
    return this._connectPromise;
  }

  resync(): Promise<void> {
    return this._subscribe();
  }

  subscribe(listener: AudioThreadEventCallback): () => void {
    this._listeners.add(listener);
    return () => {
      this._listeners.delete(listener);
    };
  }

  send(msg: AudioThreadMessage): CommandTicket {
    const ticket = new PendingTicket();
    if (this._shutdown) {
      ticket.dropped = true;
    } else if (this._nextCmd === 0) {
      this._queued.push({ msg, ticket });
      void this.connect().catch((err) =>
        console.error("[audioIpc] could not connect to send", msg.type, err),
      );
    } else {
      this._post(msg, ticket);
    }
    return ticket;
  }

  shutdown(): void {
    this._shutdown = true;
    if (this._visualFrame !== null) cancelAnimationFrame(this._visualFrame);
    this._visualFrame = null;
    this._pendingAnalysis = null;
    for (const { ticket } of this._queued) ticket.dropped = true;
    this._queued = [];
    this._listeners.clear();
  }

  private async _subscribe(): Promise<void> {
    let helloDelivered!: () => void;
    const hello = new Promise<void>((resolve) => (helloDelivered = resolve));
    this._helloWaiters.add(helloDelivered);
    const channel = new Channel<EventEnvelope>();
    // A replaced channel may still deliver; the fence drops what it repeats.
    channel.onmessage = (envelope) => this._receive(envelope);
    try {
      await invoke<void>("audio_subscribe_events", { channel });
    } catch (err) {
      this._helloWaiters.delete(helloDelivered);
      throw err;
    }
    await hello;
  }

  private _receive(envelope: EventEnvelope): void {
    if (this._shutdown) return;
    if (envelope.seq === 0) {
      this._pendingAnalysis = envelope;
      this._visualFrame ??= requestAnimationFrame(() => {
        this._visualFrame = null;
        const frame = this._pendingAnalysis;
        this._pendingAnalysis = null;
        if (frame) this._dispatch(frame.data, frame);
      });
      return;
    }
    if (envelope.seq <= this._fence) return;
    if (envelope.data.type === "hello") {
      this._fence = envelope.seq;
      this._lastSeq = envelope.seq;
      this._numberCommandsAbove(envelope.ack);
      this._connected = true;
      this._dispatch(envelope.data, envelope);
      const waiters = [...this._helloWaiters];
      this._helloWaiters.clear();
      for (const resolve of waiters) resolve();
      return;
    }
    if (envelope.seq !== this._lastSeq + 1) {
      // Nothing on the backend skips a number, so a gap means events were lost.
      logNative(
        "warn",
        `[audioIpc] event stream gap: expected seq ${this._lastSeq + 1}, got ${envelope.seq} (${envelope.data.type})`,
      );
    }
    this._lastSeq = envelope.seq;
    this._dispatch(envelope.data, envelope);
  }

  /** Start (or keep) numbering above `ack`, and send what waited for it. */
  private _numberCommandsAbove(ack: number): void {
    this._nextCmd = Math.max(this._nextCmd, ack + 1);
    const queued = this._queued;
    this._queued = [];
    for (const { msg, ticket } of queued) this._post(msg, ticket);
  }

  private _post(msg: AudioThreadMessage, ticket: PendingTicket): void {
    ticket.cmd = this._nextCmd++;
    sendAudioCommand({ cmd: ticket.cmd, data: msg }).catch((err) => {
      ticket.dropped = true;
      console.error("[audioIpc] audio_send_msg failed", msg.type, err);
    });
  }

  private _dispatch(event: AudioThreadEvent, meta: EventMeta): void {
    for (const listener of this._listeners) {
      try {
        listener(event, meta);
      } catch (err) {
        console.error("[audioIpc] event listener failed", event.type, err);
      }
    }
  }
}

export function createTauriAudioBackendTransport(): AudioBackendTransport {
  return new TauriAudioBackendTransport();
}
