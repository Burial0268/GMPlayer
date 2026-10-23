/**
 * NativeListenTogetherSync — hand the listen-together keepalive to the Rust
 * backend when there is one.
 *
 * The 30 s heartbeat is what keeps the room's server-side `CONNECTED` state
 * alive. Running it as a `setInterval` in the store means it dies with the
 * WebView, and on Android the WebView is destroyed while playback continues —
 * so the user silently drops out of the room mid-song.
 *
 * Only the keepalive moves. The status poll that observes *remote* commands
 * stays in JS: acting on one changes what plays, and two writers racing over
 * that is worse than a few seconds of missed observation.
 */

import { isTauri } from "@/utils/tauri/core/runtime";
import { NativeRustSound } from "@/utils/tauri/audio/nativeRustSound";

/**
 * Arm the backend keepalive for `roomId`, or disarm with `null`.
 *
 * Returns `true` when the backend took ownership. `false` means the caller must
 * keep running its own timer — either we are on web, or no native controller
 * exists yet (playback has not started, so there is nothing to report anyway).
 */
export const setNativeListenTogetherRoom = (roomId: string | null): boolean => {
  if (!isTauri()) return false;
  const sound = window.$player;
  if (!(sound instanceof NativeRustSound) || sound.isDestroyed()) return false;
  return sound.setListenTogetherRoom(roomId);
};
