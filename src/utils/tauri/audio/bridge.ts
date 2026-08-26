/**
 * Tauri bridge for the native audio-backend.
 *
 * AMLL-style message/event architecture: a single `audio_send_msg` Tauri
 * command replaces all individual playback commands, and events arrive as
 * `AudioThreadEvent`s matching `amll-player-core`.
 *
 * Wire types live in `./protocol`; the timeline reconciler lives in
 * `./timeline`. This module is only the command/event surface.
 */
import { invoke, isTauri } from "../core/runtime";
import type {
  AudioThreadEvent,
  AudioThreadEventCallback,
  AudioThreadEventMessage,
  AudioThreadMessage,
  NativeSessionSnapshot,
} from "./protocol";

/** Sync query response for `audio_get_state` (no round-trip through the message loop). */
export interface AudioStateResponse {
  state: "stopped" | "playing" | "paused" | "ended";
  is_playing: boolean;
  position: number;
  duration: number;
}

// ═══════════════════════════════════════════════════════════════════
//  Message dispatch
// ═══════════════════════════════════════════════════════════════════

let _callbackCounter = 0;
function newCallbackId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  _callbackCounter = (_callbackCounter + 1) >>> 0;
  return `${Date.now()}-${_callbackCounter}`;
}

/**
 * Serializes sends so the player thread observes messages in call order.
 * Without this, two `invoke`s issued in the same tick can land out of order
 * and e.g. apply a seek against the previous track.
 */
let audioSendChain: Promise<void> = Promise.resolve();

/**
 * Send an `AudioThreadMessage` to the Rust player.
 * Returns a promise resolved when the invoke round-trip completes
 * (i.e. the message has been handed off to the player thread).
 *
 * Example:
 * ```
 * audioSendMsg({ type: "resumeAudio" });
 * audioSendMsg({ type: "seekAudio", position: 30.5 });
 * audioSendMsg({ type: "setVolume", volume: 0.8 });
 * ```
 */
export function audioSendMsg(msg: AudioThreadMessage): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  audioSendChain = audioSendChain
    .then(() => invoke<void>("audio_send_msg", { msg: { callbackId: newCallbackId(), data: msg } }))
    .then(() => undefined)
    .catch((err) => {
      console.error("[audioBridge] audio_send_msg failed", msg.type, err);
    });
  return audioSendChain;
}

/** Backward-compat alias — both forms now return Promise<void>. */
export const audioSendMsgAsync = audioSendMsg;

// ═══════════════════════════════════════════════════════════════════
//  Sync query commands
// ═══════════════════════════════════════════════════════════════════

export async function audioPreheat(): Promise<void> {
  await invoke("audio_preheat");
}

export async function audioGetState(): Promise<AudioStateResponse | null> {
  return invoke<AudioStateResponse>("audio_get_state");
}

/**
 * Authoritative read of what the backend is playing right now.
 *
 * The boot path must call this *before* resolving a URL for its persisted
 * track: the Rust process (and playback) survives a WebView reload, so the
 * rehydrated frontend state describes the past, not the present. Returns `null`
 * outside Tauri or when the command is unavailable — callers then fall back to
 * the normal startup path.
 */
export async function audioGetSession(): Promise<NativeSessionSnapshot | null> {
  if (!isTauri()) return null;
  try {
    return (await invoke<NativeSessionSnapshot>("audio_get_session")) ?? null;
  } catch (err) {
    console.warn("[audioBridge] audio_get_session failed", err);
    return null;
  }
}

// ═══════════════════════════════════════════════════════════════════
//  Session-based event polling (kept for backward compat during migration)
// ═══════════════════════════════════════════════════════════════════

export async function audioSetSession(sessionId: number): Promise<void> {
  await invoke("audio_set_session", { sessionId });
}

export async function audioPollEvents(sessionId: number): Promise<AudioThreadEvent[]> {
  return (await invoke<AudioThreadEvent[]>("audio_poll_events", { sessionId })) ?? [];
}

// ═══════════════════════════════════════════════════════════════════
//  Tauri event listener
// ═══════════════════════════════════════════════════════════════════

const EVENT_CHANNEL = "audio-player://event";

/**
 * Listen for push events from the Rust backend over the global
 * `audio-player://event` emit stream. This is the fallback path used when the
 * primary `Channel` transport (see `./transport/tauri.ts`) is unavailable; the
 * Rust forwarder emits here whenever no channel is registered.
 * Returns an unlisten function.
 *
 * The Rust side wraps each event in an `AudioThreadEventMessage<AudioThreadEvent>`
 * envelope (with `callbackId` + `data` + `seq`). Some envelopes carry only an
 * ack (data is null) — we filter those out here so consumers see only real
 * events. The envelope's `seq` is forwarded as the second handler argument
 * so consumers can dedup against duplicate deliveries from the primary
 * Channel transport during a fallback transition.
 */
export async function listenPlayerEvents(handler: AudioThreadEventCallback): Promise<() => void> {
  if (!isTauri()) return () => {};
  return window.__TAURI__!.event.listen<AudioThreadEventMessage<AudioThreadEvent>>(
    EVENT_CHANNEL,
    (e) => {
      const payload = e.payload;
      if (payload && payload.data) {
        handler(payload.data, payload.seq);
      }
    },
  );
}
