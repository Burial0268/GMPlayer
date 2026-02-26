/**
 * Known window preset labels.
 * Use string literals for presets, or any string for custom windows.
 */
export type WindowLabel =
  | "main"
  | "mini-player"
  | "desktop-lyrics"
  | "settings"
  | "about"
  | "tray-popup"
  | (string & {});

/**
 * Window configuration — mirrors the Rust WindowConfig struct.
 */
export interface WindowConfig {
  label: string;
  title: string;
  url: string;
  width: number;
  height: number;
  minWidth?: number;
  minHeight?: number;
  maxWidth?: number;
  maxHeight?: number;
  resizable?: boolean;
  decorations?: boolean;
  transparent?: boolean;
  alwaysOnTop?: boolean;
  skipTaskbar?: boolean;
  center?: boolean;
  visible?: boolean;
  singleInstance?: boolean;
  closeableToTray?: boolean;
  useOverlayTitlebar?: boolean;
  trafficLightsInset?: [number, number];
}

/**
 * Window state returned from get_window_state.
 */
export interface WindowState {
  exists: boolean;
  visible: boolean;
}
