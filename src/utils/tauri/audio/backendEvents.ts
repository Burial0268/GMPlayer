/**
 * One subscription to the audio backend's event stream, fanned out by event type.
 *
 * Several modules react to backend events without owning a playback controller
 * (session controls, the random order, ...). Each used to subscribe to the
 * transport on its own and filter by `event.type` in its own callback, so the
 * order they ran in was an accident of who subscribed first, an error in one was
 * swallowed silently by the transport, and nothing said which modules listened to
 * what. This is the single place that does.
 *
 * What it guarantees:
 * - **Order.** The backend emits events in a meaningful order (the random order
 *   follows the controls that announce the mode, because a page whose mode is not
 *   yet random drops an order it did not expect), and the stream is delivered
 *   one event at a time in that order. Handlers see events in it, and handlers
 *   of the same type run in registration order.
 * - **Isolation.** A handler that throws is logged and the rest still run.
 * - **One transport listener**, attached with the first handler and detached
 *   with the last.
 *
 * What it deliberately does not do: dedupe by sequence number or gate by
 * revision. Those rules differ per consumer (a revision means something only to
 * whoever published one), so they stay with the handler. Controllers that own a
 * lifecycle — `NativeRustSound`, the personal-FM subscriber, AutoMix — keep their
 * own subscriptions for the same reason.
 *
 * Tauri-only. Outside it `onBackendEvent` is a no-op.
 */

import { isTauri } from "../core/runtime";
import { logNative } from "../core/nativeLog";
import { getAudioBackendTransport } from "./transport";
import type { AudioThreadEvent, AudioThreadEventCallback } from "./protocol";

type EventType = AudioThreadEvent["type"];
type EventOf<T extends EventType> = Extract<AudioThreadEvent, { type: T }>;
type AnyHandler = (event: AudioThreadEvent, seq?: number) => void;

const handlers = new Map<EventType, Set<AnyHandler>>();
let detach: (() => void) | null = null;

const dispatch: AudioThreadEventCallback = (event, seq) => {
  const registered = handlers.get(event.type);
  if (!registered) return;
  // Safe against a handler unregistering during the pass: a `Set` skips what was
  // deleted before it is reached.
  for (const handler of registered) {
    try {
      handler(event, seq);
    } catch (err) {
      logNative("warn", `[backendEvents] ${event.type} handler threw: ${String(err)}`);
    }
  }
};

/** Run `handler` for every backend event of `type`. Returns the unregister function. */
export const onBackendEvent = <T extends EventType>(
  type: T,
  handler: (event: EventOf<T>, seq?: number) => void,
): (() => void) => {
  if (!isTauri()) return () => {};

  let registered = handlers.get(type);
  if (!registered) {
    registered = new Set();
    handlers.set(type, registered);
  }
  const stored = handler as AnyHandler;
  registered.add(stored);
  detach ??= getAudioBackendTransport().subscribe(dispatch);

  return () => {
    if (!registered.delete(stored)) return;
    // Only drop the entry if it is still ours: after this set emptied and was
    // removed, the type may have been registered afresh under a new set.
    if (registered.size === 0 && handlers.get(type) === registered) handlers.delete(type);
    if (handlers.size === 0 && detach) {
      detach();
      detach = null;
    }
  };
};
