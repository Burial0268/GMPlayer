import type { WindowConfig, WindowLabel, WindowState } from "./types";

declare global {
  interface Window {
    __TAURI__?: {
      core: {
        invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
      };
      event: {
        listen: <T>(event: string, handler: (event: { payload: T }) => void) => Promise<() => void>;
        emit: (event: string, payload?: unknown) => Promise<void>;
      };
    };
  }
}

/**
 * Check if the app is running inside Tauri.
 */
export function isTauri(): boolean {
  return "__TAURI__" in window;
}

/**
 * Invoke a Tauri command. Returns null if not in Tauri.
 */
async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!isTauri()) return null;
  return window.__TAURI__!.core.invoke<T>(cmd, args);
}

/**
 * Listen to a Tauri event. Returns a no-op unlisten if not in Tauri.
 */
async function listen<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  return window.__TAURI__!.event.listen<T>(event, (e) => handler(e.payload));
}

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
