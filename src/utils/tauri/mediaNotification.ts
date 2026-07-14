/**
 * JS/TS bridge to `tauri-plugin-media-session`.
 *
 * This module is the mobile adapter used by `useNativeMediaControls.ts`.
 * Internally it translates between our ms-based API and the plugin's
 * seconds-based API.
 *
 * Plugin identifier : "media-session"
 * Repo             : https://github.com/sak96/tauri-plugin-media-session
 *
 * All exported functions are safe to call outside Tauri (browser / desktop):
 * they check `isTauri()` first and return silently if the environment is wrong.
 */

import { invoke, addPluginListener } from "@tauri-apps/api/core";
import { isTauri } from "./windowManager";

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
 * Used to indicate buffering, playing, or paused states.
 */
export interface UpdatePlaybackStateRequest {
  /** Current playback state */
  state: "playing" | "paused" | "buffering";
  /** Current playback position in **milliseconds**. */
  position: number;
}

/**
 * Media action event delivered by `listenMediaAction`.
 * `position` is in **milliseconds** when present (seek target).
 */
export interface MediaActionPayload {
  action: "play" | "pause" | "next" | "previous" | "stop" | "seek";
  /** Seek target in **milliseconds** — only present when `action === "seek"`. */
  position?: number;
}

/**
 * Audio focus state delivered by `listenAudioFocusChange`.
 */
export type AudioFocusState = "gain" | "loss" | "loss_transient" | "loss_transient_can_duck";

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
    playbackState: req.state,
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
// Events  (native → JS)
// ═══════════════════════════════════════════════════════════════════════════════

/**
 * Subscribe to media control actions from the notification panel,
 * lock screen, or hardware media keys.
 *
 * Normalises the plugin event format (seekPosition in **seconds**) to our
 * internal format (position in **milliseconds**) so call-sites are unchanged.
 *
 * @returns An unlisten function — call it in `onUnmounted` to clean up.
 */
export async function listenMediaAction(
  handler: (payload: MediaActionPayload) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};

  try {
    // The plugin emits { action, seekPosition?: number } where seekPosition is seconds.
    const listener = await addPluginListener<{ action: string; seekPosition?: number }>(
      PLUGIN,
      "media_action",
      (event) => {
        const payload: MediaActionPayload = {
          action: event.action as MediaActionPayload["action"],
        };
        // Convert seekPosition (seconds) → position (milliseconds) for our handlers.
        if (event.action === "seek" && typeof event.seekPosition === "number") {
          payload.position = Math.round(event.seekPosition * 1_000);
        }
        handler(payload);
      },
    );
    return () => listener.unregister();
  } catch (err) {
    console.warn("[MediaSession] listenMediaAction failed:", err);
    return () => {};
  }
}

/**
 * Subscribe to audio focus changes from the Android system.
 *
 * When another app requests audio focus (e.g. phone call, voice message),
 * the system will notify us so we can pause or duck playback.
 *
 * @returns An unlisten function — call it in `onUnmounted` to clean up.
 */
export async function listenAudioFocusChange(
  handler: (state: AudioFocusState) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};

  try {
    const listener = await addPluginListener<{ state: string }>(
      PLUGIN,
      "audio_focus_change",
      (event) => {
        handler(event.state as AudioFocusState);
      },
    );
    return () => listener.unregister();
  } catch (err) {
    // Plugin may not support audio_focus_change yet — log and return no-op
    console.warn("[MediaSession] listenAudioFocusChange failed (plugin may not support it):", err);
    return () => {};
  }
}
