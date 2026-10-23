/**
 * Screen orientation control for Tauri mobile (Android).
 *
 * Android delegates to the Kotlin `OrientationPlugin`; other mobile targets
 * fall through to a no-op Rust command. Note that on Android the command is
 * intentionally handled *only* in Kotlin — a Rust handler of the same name
 * would shadow it and silently do nothing.
 *
 * Rotation policy: the activity ships as `nosensor`, so the system never
 * rotates us on its own. Orientation changes only happen where we explicitly
 * ask for one, and `restoreDefaultOrientation()` puts the lock back.
 *
 * All functions are safe to call outside Tauri or on desktop:
 * they check the current target first and return silently.
 */

import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "../core/runtime";

/**
 * "default"   = 设备自然方向，传感器关闭（应用常态）
 * "portrait"  = 锁定竖屏
 * "landscape" = 锁定横屏，允许左右两个横向翻转
 * "auto"      = 交还控制权，并尊重系统的「自动旋转」开关
 */
export type ScreenOrientation = "default" | "portrait" | "landscape" | "auto";

// 平台在进程生命周期内不会变，探测一次即可；每次换向都往 IPC 上打一发
// detect_desktop 纯属浪费，而且会让换向多等一个来回。
let mobileProbe: Promise<boolean> | null = null;

function isTauriMobile(): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(false);

  mobileProbe ??= invoke<boolean>("detect_desktop")
    .then((isDesktop) => !isDesktop)
    .catch(() => false);

  return mobileProbe;
}

/**
 * Lock the screen to a specific orientation.
 */
export async function setScreenOrientation(orientation: ScreenOrientation): Promise<void> {
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
 * Convenience: lock to portrait.
 */
export function lockPortrait(): Promise<void> {
  return setScreenOrientation("portrait");
}

/**
 * Restore the app default: device-natural orientation with the sensor off.
 *
 * Prefer this over {@link lockPortrait} when leaving a screen that forced an
 * orientation — it keeps AndroidTV (naturally landscape) upright.
 */
export function restoreDefaultOrientation(): Promise<void> {
  return setScreenOrientation("default");
}

/**
 * Convenience: allow auto-rotation (follow the user's auto-rotate setting).
 */
export function unlockOrientation(): Promise<void> {
  return setScreenOrientation("auto");
}
