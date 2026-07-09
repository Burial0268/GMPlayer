import { invoke } from "@tauri-apps/api/core";
import { isMobileDevice } from "./mobile";
import { isTauri } from "./windowManager";

export { windowManager, isTauri, isWindowsTauri } from "./windowManager";
export { isMobile, isMobileDevice } from "./mobile";
export type { WindowConfig, WindowLabel, WindowState } from "./types";
export { usePlayerBridge } from "./playerBridge";
export {
  PLAYER_COMMUNICATION_EVENTS,
  PLAYER_CONTENT_WINDOW_LABELS,
  PLAYER_STATE_WINDOW_LABELS,
} from "./playerCommunicationTypes";
export type {
  PlayerFullStatePayload,
  PlayerStatePayload,
  PlayerTimePayload,
  PlayerLyricPayload,
  PlayerSettingsPayload,
} from "./playerCommunicationTypes";

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

export interface DesktopEnvironment {
  os: string;
  family: string;
  desktop: string | null;
  sessionType: string | null;
  isMobile: boolean;
  isMacos: boolean;
  isLinux: boolean;
  isHyprland: boolean;
  usesNativeTrafficLights: boolean;
}

let desktopEnvironmentPromise: Promise<DesktopEnvironment> | null = null;

function browserDesktopEnvironment(): DesktopEnvironment {
  if (typeof window === "undefined" || !window.navigator) {
    return {
      os: "unknown",
      family: "unknown",
      desktop: null,
      sessionType: null,
      isMobile: false,
      isMacos: false,
      isLinux: false,
      isHyprland: false,
      usesNativeTrafficLights: false,
    };
  }

  const platform = window.navigator.platform ?? "";
  const userAgent = window.navigator.userAgent ?? "";
  const isMobile = isMobileDevice();
  const isMacos = /Mac/i.test(platform) && !isMobile;
  const isLinux = /Linux|X11/i.test(platform) || /Linux/i.test(userAgent);
  const isWindows = /Win/i.test(platform) || /Windows/i.test(userAgent);

  return {
    os: isMacos ? "macos" : isWindows ? "windows" : isLinux ? "linux" : "unknown",
    family: isWindows ? "windows" : isMacos || isLinux ? "unix" : "unknown",
    desktop: null,
    sessionType: null,
    isMobile,
    isMacos,
    isLinux,
    isHyprland: false,
    usesNativeTrafficLights: isMacos && !isMobile,
  };
}

export async function getDesktopEnvironment(): Promise<DesktopEnvironment> {
  if (!isTauri()) return browserDesktopEnvironment();

  desktopEnvironmentPromise ??= invoke<DesktopEnvironment>("desktop_environment").catch((error) => {
    console.error("Failed to detect desktop environment:", error);
    return browserDesktopEnvironment();
  });

  return desktopEnvironmentPromise;
}
