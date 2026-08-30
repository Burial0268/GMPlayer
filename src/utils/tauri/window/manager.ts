import { invoke, listen } from "../core/runtime";
import type { WindowConfig, WindowLabel, WindowState } from "./types";

export const windowManager = {
  /**
   * Create a window from a preset label.
   */
  async createWindow(label: WindowLabel): Promise<void> {
    await invoke("create_window", { label });
  },

  /**
   * Create a window with a fully custom configuration.
   */
  async createCustomWindow(config: WindowConfig): Promise<void> {
    await invoke("create_custom_window", { config });
  },

  /**
   * Create a window from a preset with an attached payload.
   */
  async createWindowWithPayload(label: WindowLabel, payload: unknown): Promise<void> {
    await invoke("create_window_with_payload", { label, payload });
  },

  /**
   * Show a window by label.
   */
  async showWindow(label: WindowLabel): Promise<void> {
    await invoke("show_window", { label });
  },

  /**
   * Hide a window by label.
   */
  async hideWindow(label: WindowLabel): Promise<void> {
    await invoke("hide_window", { label });
  },

  /**
   * Close a window (respects closeable-to-tray setting).
   */
  async closeWindow(label: WindowLabel): Promise<void> {
    await invoke("close_managed_window", { label });
  },

  /**
   * Toggle window visibility.
   */
  async toggleWindow(label: WindowLabel): Promise<void> {
    await invoke("toggle_window", { label });
  },

  /**
   * Focus a window.
   */
  async focusWindow(label: WindowLabel): Promise<void> {
    await invoke("focus_window", { label });
  },

  /**
   * Get the state (exists, visible) of a window.
   */
  async getWindowState(label: WindowLabel): Promise<WindowState | null> {
    return invoke<WindowState>("get_window_state", { label });
  },

  /**
   * List all open window labels.
   */
  async listWindows(): Promise<string[] | null> {
    return invoke<string[]>("list_windows");
  },

  /**
   * Open DevTools for a managed window. Dev builds only.
   */
  async openWindowDevtools(label: WindowLabel = "main"): Promise<void> {
    if (!import.meta.env.DEV) return;
    await invoke("open_window_devtools", { label });
  },

  /**
   * Store a payload for a window label.
   */
  async setPayload(label: WindowLabel, payload: unknown): Promise<void> {
    await invoke("set_window_payload", { label, payload });
  },

  /**
   * Take (consume) a stored payload.
   */
  async takePayload<T = unknown>(label: WindowLabel): Promise<T | null> {
    return invoke<T>("take_window_payload", { label });
  },

  /**
   * Peek at a stored payload without consuming it.
   */
  async peekPayload<T = unknown>(label: WindowLabel): Promise<T | null> {
    return invoke<T>("peek_window_payload", { label });
  },

  /**
   * Listen for tray play/pause events.
   */
  async onTrayPlayPause(handler: () => void): Promise<() => void> {
    return listen("tray-play-pause", handler);
  },

  /**
   * Listen for tray next track events.
   */
  async onTrayNextTrack(handler: () => void): Promise<() => void> {
    return listen("tray-next-track", handler);
  },

  /**
   * Listen for tray previous track events.
   */
  async onTrayPrevTrack(handler: () => void): Promise<() => void> {
    return listen("tray-prev-track", handler);
  },

  /**
   * Quit the application (saves window state first).
   */
  async quitApp(): Promise<void> {
    await invoke("quit_app");
  },

  /**
   * Write out everything that is otherwise only persisted on `RunEvent::Exit`
   * (window geometry, Pinia stores).
   *
   * For callers that end the process without going through the Tauri run loop —
   * in practice the OTA updater, which spawns the installer and then calls
   * `std::process::exit(0)`.
   */
  async flushPersistedState(): Promise<void> {
    await invoke("flush_persisted_state");
  },

  /**
   * Update the native window effect tint color (e.g. Acrylic on Windows).
   */
  async setWindowEffectColor(
    label: WindowLabel,
    r: number,
    g: number,
    b: number,
    a: number,
  ): Promise<void> {
    await invoke("set_window_effect_color", { label, r, g, b, a });
  },

  /** Apply the app theme to the native main-window material and reveal the window. */
  async setMainWindowEffectTheme(dark: boolean): Promise<void> {
    await invoke("set_main_window_effect_theme", { dark });
  },

  /**
   * Set whether a window ignores cursor events (click-through).
   */
  async setIgnoreCursorEvents(label: WindowLabel, ignore: boolean): Promise<void> {
    await invoke("set_ignore_cursor_events", { label, ignore });
  },

  /**
   * Resize a window to a logical size.
   */
  async resizeWindow(label: WindowLabel, width: number, height: number): Promise<void> {
    await invoke("resize_window", { label, width, height });
  },

  /**
   * Set window position to specific physical coordinates.
   */
  async setWindowPosition(label: WindowLabel, x: number, y: number): Promise<void> {
    await invoke("set_window_position", { label, x, y });
  },

  /**
   * Open the Windows taskbar lyric window.
   */
  async openTaskbarLyrics(): Promise<void> {
    await invoke("plugin:taskbar-lyric|open_taskbar_lyric");
  },

  /**
   * Close the Windows taskbar lyric window.
   */
  async closeTaskbarLyrics(): Promise<void> {
    await invoke("plugin:taskbar-lyric|close_taskbar_lyric");
  },

  /**
   * Open devtools for the Windows taskbar lyric window. Dev builds only.
   */
  async openTaskbarLyricsDevtools(): Promise<void> {
    if (!import.meta.env.DEV) return;
    await invoke("plugin:taskbar-lyric|open_taskbar_lyric_devtools");
  },

  /**
   * Listen for main window close-requested events.
   */
  async onMainCloseRequested(handler: () => void): Promise<() => void> {
    return listen("main-close-requested", handler);
  },

  /**
   * Listen for main window visibility changes (show/hide from Rust).
   */
  async onMainWindowVisibility(handler: (visible: boolean) => void): Promise<() => void> {
    return listen("main-window-visibility", handler);
  },

  /**
   * Get the current screen cursor position (physical pixels).
   */
  async getCursorPosition(): Promise<[number, number] | null> {
    return invoke<[number, number]>("get_cursor_position");
  },

  /**
   * Get a window's outer bounds (physical pixels): [x, y, width, height].
   */
  async getWindowBounds(label: WindowLabel): Promise<[number, number, number, number] | null> {
    return invoke<[number, number, number, number]>("get_window_bounds", { label });
  },

  /**
   * Update the tray icon tooltip (e.g., "Song Name - Artist").
   */
  async setTrayTooltip(text: string): Promise<void> {
    await invoke("set_tray_tooltip", { text });
  },
};

if (import.meta.env.DEV && typeof window !== "undefined") {
  window.open_taskbar_lyric_devtools = () => windowManager.openTaskbarLyricsDevtools();
  window.open_window_devtools = (label = "main") => windowManager.openWindowDevtools(label);
}
