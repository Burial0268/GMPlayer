/**
 * JS/TS bridge to `tauri-plugin-media-session`.
 *
 * Push direction only. Metadata and playback state are normally written by
 * Rust (`src-tauri/src/media`), and the *control* direction — buttons, media
 * keys, audio focus — never comes back through here at all; see the note at
 * the bottom of this file. What is left is a thin, correct wrapper for the
 * plugin's commands, translating our ms-based API to its seconds-based one.
 *
 * Plugin identifier : "media-session"
 * Vendored at      : `src-tauri/crates/tauri-plugin-media-session`
 *
 * All exported functions are safe to call outside Tauri (browser / desktop):
 * they check `isTauri()` first and return silently if the environment is wrong.
 */

import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "../core/runtime";

// ═══════════════════════════════════════════════════════════════════════════════
// Types (public interface — kept identical so call-sites need no changes)
// ═══════════════════════════════════════════════════════════════════════════════

/**
 * Full metadata + state payload for `updateMediaNotification`.
 * Times are in **milliseconds** (we divide by 1000 before calling the plugin).
 */
export interface MediaNotificationRequest {
  title: string;
  artist: string;
  album: string;
  isPlaying: boolean;
  /** Current playback position in **milliseconds**. */
  position: number;
  /** Total track duration in **milliseconds**. */
  duration: number;
  /** HTTP/HTTPS URL for album cover art.  Empty string = use app-icon fallback. */
  artworkUrl: string;
}

/**
 * Lightweight position + playing-state payload for `updateMediaProgress`.
 * Times are in **milliseconds**.
 */
export interface UpdateProgressRequest {
  isPlaying: boolean;
  /** Current playback position in **milliseconds**. */
  position: number;
  /** Total track duration in **milliseconds** — keeps the notification
   * seekbar range consistent when metadata pushes were deduped away. */
  duration?: number;
}

/**
 * Playback state payload for `updateMediaPlaybackState`.
 *
 * `buffering` is a distinct state, not "paused with a spinner": the native
 * session renders it as `STATE_BUFFERING`, which is what tells the system
 * controls to show a spinner instead of a play button that looks inert.
 */
export interface UpdatePlaybackStateRequest {
  /** Current playback state */
  state: "playing" | "paused" | "buffering";
  /** Current playback position in **milliseconds**. */
  position: number;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Internal
// ═══════════════════════════════════════════════════════════════════════════════

const PLUGIN = "media-session";

async function call<T = void>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T | undefined> {
  if (!isTauri()) return undefined;
  try {
    return await invoke<T>(`plugin:${PLUGIN}|${command}`, args);
  } catch (err) {
    console.warn(`[MediaSession] "${command}" failed:`, err);
    return undefined;
  }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Commands  (JS → Android / iOS)
// ═══════════════════════════════════════════════════════════════════════════════

/**
 * Initialize the native MediaSession bridge when the plugin exposes an explicit
 * lifecycle command. Older plugin builds may no-op here through `call`.
 */
export function initializeMediaNotification(): Promise<void | undefined> {
  return call("initialize");
}

/**
 * Push a full metadata + playback-state update to the native MediaSession.
 * Internally calls `update_state` on the plugin with times converted to seconds.
 */
export function updateMediaNotification(req: MediaNotificationRequest): Promise<void | undefined> {
  return call("update_state", {
    title: req.title || undefined,
    artist: req.artist || undefined,
    album: req.album || undefined,
    artworkUrl: req.artworkUrl || undefined,
    isPlaying: req.isPlaying,
    position: req.position / 1_000, // ms → s
    duration: req.duration / 1_000, // ms → s
    // Always advertise all controls so prev / next / seek buttons are shown.
    canPrev: true,
    canNext: true,
    canSeek: true,
  });
}

/**
 * Lightweight playing-state + position update.
 * Internally calls `update_state` (not `update_timeline`) so `isPlaying` is
 * also updated atomically with the position.
 */
export function updateMediaProgress(req: UpdateProgressRequest): Promise<void | undefined> {
  return call("update_state", {
    isPlaying: req.isPlaying,
    position: req.position / 1_000, // ms → s
    duration:
      typeof req.duration === "number" && req.duration > 0
        ? req.duration / 1_000 // ms → s
        : undefined,
  });
}

/**
 * Update only the playback state without changing metadata.
 * Used to indicate buffering state during loading.
 */
export function updateMediaPlaybackState(
  req: UpdatePlaybackStateRequest,
): Promise<void | undefined> {
  return call("update_state", {
    isLoading: req.state === "buffering",
    isPlaying: req.state === "playing",
    position: req.position / 1_000, // ms → s
  });
}

/**
 * Dismiss the native media notification and release session resources.
 */
export function hideMediaNotification(): Promise<void | undefined> {
  return call("clear");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Events  (native → Rust)
// ═══════════════════════════════════════════════════════════════════════════════
//
// There is deliberately no listener here.
//
// Media buttons, the lock screen and audio focus are delivered by the Android
// plugin over its Rust `Channel` and handled in `src-tauri/src/media` — a JNI
// hop with no WebView in it. That is not a preference: the WebView is destroyed
// while playback continues, and a notification whose buttons only reach JS
// stops working exactly when the notification is the only UI left. Routing them
// back through here would also make two writers for one transport.
