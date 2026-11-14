import type { SoundEventCallback, SoundEventMap, SoundEventType } from "./types";

interface Listener {
  callback: SoundEventCallback;
  once: boolean;
}

/** Event plumbing shared by every `ISound` implementation. */
export abstract class SoundEvents {
  private _listeners = new Map<SoundEventType, Listener[]>();

  on<E extends SoundEventType>(event: E, callback: SoundEventCallback<E>): this {
    return this._listen(event, callback, false);
  }

  once<E extends SoundEventType>(event: E, callback: SoundEventCallback<E>): this {
    return this._listen(event, callback, true);
  }

  /** Without a callback, drops every listener for the event. */
  off<E extends SoundEventType>(event: E, callback?: SoundEventCallback<E>): this {
    if (!callback) {
      this._listeners.delete(event);
      return this;
    }
    const kept = this._listeners.get(event)?.filter((l) => l.callback !== callback);
    if (kept) this._listeners.set(event, kept);
    return this;
  }

  protected _emit<E extends SoundEventType>(event: E, ...args: SoundEventMap[E]): void {
    const listeners = this._listeners.get(event);
    if (!listeners) return;
    // Once-listeners leave before any callback runs, so one that registers
    // itself again from inside its own call is kept for the next emit.
    this._listeners.set(
      event,
      listeners.filter((l) => !l.once),
    );
    // Persistent listeners first, then once-listeners: the order callers rely on.
    for (const once of [false, true]) {
      for (const listener of listeners) {
        if (listener.once !== once) continue;
        try {
          (listener.callback as (...a: unknown[]) => void)(...args);
        } catch (error) {
          console.error(`[sound] "${event}" listener threw:`, error);
        }
      }
    }
  }

  protected _clearListeners(): void {
    this._listeners.clear();
  }

  private _listen(event: SoundEventType, callback: SoundEventCallback, once: boolean): this {
    const listeners = this._listeners.get(event);
    if (listeners) listeners.push({ callback, once });
    else this._listeners.set(event, [{ callback, once }]);
    return this;
  }
}
