/**
 * Public surface of the Tauri integration layer.
 *
 * Import from here for anything general-purpose. Deep imports into the
 * subfolders are fine (and expected) for the heavier, more specialized
 * modules — `audio/nativeRustSound`, `audio/transport`, `player/communication`
 * — which only a few call sites need and which shouldn't be pulled into every
 * bundle that wants `isTauri()`.
 *
 * Layout:
 *   core/      runtime detection, invoke/listen, the `window.__TAURI__` global
 *   window/    window manager + window config types
 *   audio/     native audio backend: protocol, bridge, transports, timeline
 *   player/    cross-window player state sync
 *   media/     OS media controls (Android notification, desktop now-playing)
 *   platform/  device/platform capability helpers
 *   store/     Tauri-backed Pinia persistence (layered on top of localStorage)
 */

// ── core ────────────────────────────────────────────────────────────
export { isTauri, isWindowsTauri, getTauri, invoke, listen, emit, emitTo } from "./core/runtime";
export { getDesktopEnvironment, type DesktopEnvironment } from "./core/env";

// ── window ──────────────────────────────────────────────────────────
export { windowManager } from "./window/manager";
export type { WindowConfig, WindowLabel, WindowState } from "./window/types";

// ── platform ────────────────────────────────────────────────────────
export { isMobile, isMobileDevice } from "./platform/mobile";

// ── store ───────────────────────────────────────────────────────────
export {
  installTauriPinia,
  isTauriPiniaInstalled,
  startTauriPiniaStores,
  destroyTauriPiniaStores,
  type TauriPersistedStore,
} from "./store/piniaPersistence";

// Screen orientation control (Android)
export {
  setScreenOrientation,
  lockLandscape,
  lockPortrait,
  restoreDefaultOrientation,
  unlockOrientation,
  type ScreenOrientation,
} from "./platform/screenOrientation";

// ── player ──────────────────────────────────────────────────────────
export { usePlayerBridge } from "./player/bridge";
export {
  PLAYER_COMMUNICATION_EVENTS,
  PLAYER_CONTENT_WINDOW_LABELS,
  PLAYER_STATE_WINDOW_LABELS,
} from "./player/types";
export type {
  PlayerFullStatePayload,
  PlayerStatePayload,
  PlayerTimePayload,
  PlayerLyricPayload,
  PlayerSettingsPayload,
} from "./player/types";

// ── media ───────────────────────────────────────────────────────────
// Android media notification plugin bridge (push only — the control
// direction is owned by Rust, see `media/notification.ts`)
export {
  initializeMediaNotification,
  updateMediaNotification,
  updateMediaProgress,
  updateMediaPlaybackState,
  hideMediaNotification,
  type MediaNotificationRequest,
  type UpdateProgressRequest,
  type UpdatePlaybackStateRequest,
} from "./media/notification";

// Desktop now playing controls bridge
export {
  initializeNowPlayingControls,
  updateNowPlayingState,
  updateNowPlayingTimeline,
  updateNowPlayingPlayMode,
  setNowPlayingEnabled,
  clearNowPlayingControls,
  type NowPlayingStateRequest,
  type NowPlayingTimelineRequest,
  type NowPlayingPlayModeRequest,
} from "./media/nowPlaying";
