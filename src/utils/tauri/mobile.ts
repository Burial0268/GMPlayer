import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./windowManager";

export function isMobileDevice(): boolean {
  if (typeof window === "undefined" || !window.navigator) return false;
  const platform = window.navigator.platform ?? "";
  const maxTouchPoints = window.navigator.maxTouchPoints ?? 0;
  const isIpadDesktopMode = /Mac/i.test(platform) && maxTouchPoints > 1;

  return (
    /Android|webOS|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i.test(
      window.navigator.userAgent,
    ) || isIpadDesktopMode
  );
}

export async function isMobile(): Promise<boolean> {
  // Always return true if it's a mobile device (browser or native)
  if (isMobileDevice()) return true;

  if (!isTauri()) return false;
  try {
    const isDesktop = await invoke<boolean>("detect_desktop");
    return !isDesktop;
  } catch (error) {
    console.error("Failed to detect desktop status:", error);
    return false; // Default to not mobile on error
  }
}
