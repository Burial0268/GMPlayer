/**
 * Screen orientation control for Tauri mobile (Android).
 *
 * Uses the inline "orientation" Tauri plugin on mobile. Android delegates
 * to the Kotlin OrientationPlugin; other mobile targets use a no-op Rust
 * command.
 *
 * All functions are safe to call outside Tauri or on desktop:
 * they check the current target first and return silently.
 */

import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./windowManager";

async function isTauriMobile(): Promise<boolean> {
  if (!isTauri()) return false;

  try {
    const isDesktop = await invoke<boolean>("detect_desktop");
    return !isDesktop;
  } catch {
    return false;
  }
}

/**
 * Lock the screen to a specific orientation.
 *
 * @param orientation - "landscape" | "portrait" | "auto"
 *   landscape = force landscape (for video fullscreen)
 *   portrait  = force portrait (normal app usage)
 *   auto      = follow device sensor (allow rotation)
 */
export async function setScreenOrientation(
  orientation: "landscape" | "portrait" | "auto",
): Promise<void> {
  if (!(await isTauriMobile())) return;

  try {
    await invoke("plugin:orientation|setOrientation", { orientation });
    console.log(`[ScreenOrientation] Set to: ${orientation}`);
  } catch (err) {
    console.warn("[ScreenOrientation] Failed to set orientation:", err);
  }
}

/**
 * Convenience: lock to landscape (call when entering video fullscreen).
 */
export function lockLandscape(): Promise<void> {
  return setScreenOrientation("landscape");
}

/**
 * Convenience: lock to portrait (call when exiting video fullscreen).
 */
export function lockPortrait(): Promise<void> {
  return setScreenOrientation("portrait");
}

/**
 * Convenience: allow auto-rotation (follow device sensor).
 */
export function unlockOrientation(): Promise<void> {
  return setScreenOrientation("auto");
}
