import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./windowManager";

export { windowManager, isTauri, isWindowsTauri } from "./windowManager";
export type { WindowConfig, WindowLabel, WindowState } from "./types";
export { usePlayerBridge } from "./playerBridge";
export type {
  PlayerStatePayload,
  PlayerTimePayload,
  PlayerLyricPayload,
  PlayerSettingsPayload,
} from "./playerBridge";

// Android media notification plugin bridge
export {
  initializeMediaNotification,
  updateMediaNotification,
  updateMediaProgress,
  hideMediaNotification,
  listenMediaAction,
  type MediaNotificationRequest,
  type UpdateProgressRequest,
  type MediaActionPayload,
} from "./mediaNotification";

// Desktop now playing controls bridge
export {
  initializeNowPlayingControls,
  updateNowPlayingState,
  updateNowPlayingTimeline,
  updateNowPlayingPlayMode,
  setNowPlayingEnabled,
  clearNowPlayingControls,
  listenNowPlayingAction,
  type NowPlayingStateRequest,
  type NowPlayingTimelineRequest,
  type NowPlayingPlayModeRequest,
  type NowPlayingActionPayload,
} from "./nowPlayingControls";

// Screen orientation control (Android)
export {
  setScreenOrientation,
  lockLandscape,
  lockPortrait,
  unlockOrientation,
} from "./screenOrientation";
export function isMobileDevice(): boolean {
  if (typeof window === "undefined" || !window.navigator) return false;
  return /Android|webOS|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i.test(
    window.navigator.userAgent,
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
