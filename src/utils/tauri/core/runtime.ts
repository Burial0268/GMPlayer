/**
 * Tauri runtime detection and the low-level `invoke`/`listen` wrappers that
 * every bridge module in this folder builds on.
 *
 * These wrappers go through the injected `window.__TAURI__` global rather than
 * the `@tauri-apps/api` module so that a web build can import any bridge
 * module without pulling the Tauri API into the bundle — calls degrade to a
 * null/no-op in
 stead of throwing.
 */
import type { TauriGlobal } from "./globals";

/**
 * Whether the app is running inside a Tauri webview.
 *
 * The null/typeof checks matter: some webview bootstraps briefly expose the
 * key with a non-object value, so a bare `"__TAURI__" in window` can report
 * true while `.core` is still undefined.
 */
export function isTauri(): boolean {
  return (
    typeof window !== "undefined" &&
    "__TAURI__" in window &&
    window.__TAURI__ !== null &&
    typeof window.__TAURI__ === "object"
  );
}

/** The injected Tauri global, or `null` outside Tauri. */
export function getTauri(): TauriGlobal | null {
  return isTauri() ? window.__TAURI__! : null;
}

/** Whether the app is running in Tauri on Windows. */
export function isWindowsTauri(): boolean {
  if (!isTauri()) return false;
  const platform = window.navigator?.platform ?? "";
  const userAgent = window.navigator?.userAgent ?? "";
  return /Win/i.test(platform) || /Windows/i.test(userAgent);
}

/** Invoke a Tauri command. Resolves to `null` outside Tauri. */
export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  const tauri = getTauri();
  if (!tauri) return null;
  return tauri.core.invoke<T>(cmd, args);
}

/**
 * Listen to a Tauri event, receiving the unwrapped payload.
 * Returns a no-op unlisten outside Tauri.
 */
export async function listen<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
  const tauri = getTauri();
  if (!tauri) return () => {};
  return tauri.event.listen<T>(event, (e) => handler(e.payload));
}

/** Emit a Tauri event globally. No-op outside Tauri. */
export async function emit(event: string, payload?: unknown): Promise<void> {
  await getTauri()?.event.emit(event, payload);
}

/** Emit a Tauri event to a specific window label. No-op outside Tauri. */
export async function emitTo(target: string, event: string, payload?: unknown): Promise<void> {
  await getTauri()?.event.emitTo(target, event, payload);
}
