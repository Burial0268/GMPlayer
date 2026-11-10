/**
 * Write a line to the app log (`GMPlayer.log`) from the WebView.
 *
 * `console.*` never reaches that file — the log plugin captures Rust only — and
 * most playback diagnostics here were also behind `import.meta.env.DEV`, so a
 * release build on a device left nothing to read after a bug. This goes through
 * the same `plugin:log|log` command `@tauri-apps/plugin-log` wraps, via the
 * injected global, so the web bundle stays free of the Tauri API.
 *
 * Use it for events that are rare or per-track (a manifest publish, an adoption,
 * a disagreement), never per frame. Outside Tauri it is a no-op.
 */
import { invoke, isTauri } from "./runtime";

// `log::Level` as the plugin numbers it.
const LEVELS = { info: 3, warn: 4 } as const;

export const logNative = (level: keyof typeof LEVELS, message: string): void => {
  if (!isTauri()) return;
  invoke("plugin:log|log", { level: LEVELS[level], message, location: "webview" }).catch(() => {
    /* a log line must never be able to fail the caller */
  });
};
