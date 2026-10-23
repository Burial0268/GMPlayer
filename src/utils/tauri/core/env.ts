/**
 * Desktop environment detection.
 *
 * In Tauri the answer comes from the Rust `desktop_environment` command and is
 * memoized for the process lifetime (it cannot change while running). On the
 * web, and whenever that command fails, we fall back to a best-effort guess
 * derived from the user agent so callers never have to special-case the
 * platform.
 */
import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./runtime";
import { isMobileDevice } from "../platform/mobile";

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
