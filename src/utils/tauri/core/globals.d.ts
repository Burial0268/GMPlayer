import type { WindowLabel } from "../window/types";

/**
 * The single source of truth for the `window.__TAURI__` shape.
 *
 * This describes the runtime global that Tauri injects via
 * `withGlobalTauri`, which is deliberately *not* the same thing as the
 * `@tauri-apps/api` module imports. Only the surface we actually call is
 * modelled here — widen it here rather than re-declaring the global in a
 * feature module, otherwise the declarations silently have to be kept in
 * lockstep and drift the moment one of them gains a member.
 */
interface TauriGlobal {
  core: {
    invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
  };
  event: {
    listen: <T>(event: string, handler: (event: { payload: T }) => void) => Promise<() => void>;
    emit: (event: string, payload?: unknown) => Promise<void>;
    emitTo: (target: string, event: string, payload?: unknown) => Promise<void>;
  };
}

declare global {
  interface Window {
    __TAURI__?: TauriGlobal;

    /** Dev-only devtools helpers, mounted by `window/manager.ts`. */
    open_taskbar_lyric_devtools?: () => Promise<void>;
    open_window_devtools?: (label?: WindowLabel) => Promise<void>;
  }
}

export type { TauriGlobal };
