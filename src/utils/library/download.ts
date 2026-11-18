/**
 * The download queue's command layer.
 *
 * Tauri-only, like `localLibrary.ts`, and degrades to "unavailable" on the web
 * build rather than throwing — the web has no place to put a file that the app
 * could then index, so there is nothing to fall back *to*.
 *
 * ## Why so little happens here
 *
 * The destination, the filename, the transfer and the library registration all
 * live in Rust (`src-tauri/src/download/`). That is not tidiness: a batch has to
 * survive the page that started it. On Android the WebView is killed routinely
 * while playback and downloads continue, and the old implementation — `fetch` into
 * a blob, then a synthetic `<a download>` click — did not work on Android *at all*
 * and put the file somewhere the app could not name on desktop.
 *
 * So this file is types plus `invoke`, and the queue's truth is always what Rust
 * last pushed down the event channel.
 */
import { Channel, invoke } from "@tauri-apps/api/core";
import { isTauri } from "@/utils/tauri/core/runtime";

// ── Wire types (mirror `src-tauri/src/download/`) ─────────────────

/**
 * String-valued on purpose. `tsconfig` has `strict: false`, under which a boolean
 * discriminant does not narrow — see the note in `ts-strict-false` territory
 * elsewhere in this codebase — so every discriminated union here keys on a string.
 */
export type DownloadStatus =
  | "queued"
  | "resolving"
  | "downloading"
  | "paused"
  | "done"
  | "failed"
  | "skipped";

export interface DownloadTask {
  id: string;
  songId: number;
  title: string;
  artist: string;
  album: string;
  br: number;
  status: DownloadStatus;
  received: number;
  /** `null` when the server declared no size; render indeterminate. */
  total: number | null;
  error: string | null;
  /** The local-library key, once the file has been published. */
  locator: string | null;
  fileName: string | null;
  createdAt: number;
  index: number;
}

export interface DownloadConfig {
  /** An absolute path on desktop, a `content://` tree URI on Android. */
  dir: string | null;
  dirLabel: string | null;
  concurrency: number;
  /** How many `Range` streams one song is split across. 1 turns splitting off. */
  segments: number;
  filenameTemplate: string;
  defaultBr: number;
  writeLyric: boolean;
  writeCover: boolean;
  /**
   * Write title/artist/album, the lyric and the cover *into* the audio file.
   *
   * On by default: an NCM streaming source carries none of them, so without this a
   * downloaded track describes itself only by its filename.
   */
  embedTags: boolean;
}

export interface DownloadSettings extends DownloadConfig {
  /** Where downloads would go if nothing has been chosen. Not created. */
  defaultDir: string;
  /** True on Android: the destination is a SAF grant the user must pick. */
  requiresPick: boolean;
}

export type DownloadEvent =
  | { type: "snapshot"; tasks: DownloadTask[] }
  | { type: "updated"; task: DownloadTask }
  | { type: "configChanged"; config: DownloadConfig };

export interface DownloadRequest {
  songId: number;
  title: string;
  artist?: string;
  album?: string;
  /** Overrides the configured default quality for this item. */
  br?: number;
  coverUrl?: string;
}

/**
 * What `download_enqueue` rejects with when Android has no directory yet.
 *
 * A sentinel rather than a message so the caller can open the picker instead of
 * showing an error — "you have not chosen a folder" is not a failure.
 */
export const NEEDS_DIRECTORY = "needs-directory";

export const isNeedsDirectory = (error: unknown): boolean =>
  typeof error === "string" ? error.includes(NEEDS_DIRECTORY) : false;

// ── Commands ─────────────────────────────────────────────────────

/** The web build has nowhere to put a file the library could then index. */
export const downloadAvailable = (): boolean => isTauri();

const unavailableSettings = (): DownloadSettings => ({
  dir: null,
  dirLabel: null,
  concurrency: 3,
  segments: 4,
  filenameTemplate: "{artist} - {title}",
  defaultBr: 320000,
  writeLyric: true,
  writeCover: false,
  embedTags: true,
  defaultDir: "",
  requiresPick: false,
});

export const downloadSettingsGet = async (): Promise<DownloadSettings> => {
  if (!isTauri()) return unavailableSettings();
  return invoke<DownloadSettings>("download_settings_get");
};

export const downloadConfigSet = async (
  config: Partial<DownloadConfig> & Pick<DownloadConfig, "concurrency">,
): Promise<DownloadSettings> => {
  if (!isTauri()) return unavailableSettings();
  return invoke<DownloadSettings>("download_config_set", { config });
};

/**
 * Open the platform picker for a download directory.
 *
 * Resolves to `null` when the user cancelled. The desktop/Android split is made
 * in Rust so there is one flow: a native folder dialog, or
 * `ACTION_OPEN_DOCUMENT_TREE` with a persistable read+write grant.
 */
export const downloadDirPick = async (): Promise<DownloadSettings | null> => {
  if (!isTauri()) return null;
  return invoke<DownloadSettings | null>("download_dir_pick");
};

export const downloadList = async (): Promise<DownloadTask[]> => {
  if (!isTauri()) return [];
  return invoke<DownloadTask[]>("download_list");
};

/**
 * Subscribe to queue events.
 *
 * One channel at a time on the Rust side, replaced on re-subscribe — so calling
 * this again after an HMR reload is correct rather than a leak.
 */
export const downloadSubscribe = async (onEvent: (event: DownloadEvent) => void): Promise<void> => {
  if (!isTauri()) return;
  const channel = new Channel<DownloadEvent>();
  channel.onmessage = onEvent;
  await invoke("download_subscribe", { events: channel });
};

/**
 * Hand the queue the account cookie it resolves download URLs with.
 *
 * Held in memory only on the Rust side. Pushed on login state changes because a
 * queue that outlives the page cannot ask the page for a credential later.
 */
export const downloadSetCredentials = async (cookie: string | null): Promise<void> => {
  if (!isTauri()) return;
  await invoke("download_set_credentials", { cookie });
};

export const downloadEnqueue = async (items: DownloadRequest[]): Promise<DownloadTask[]> => {
  if (!isTauri()) return [];
  return invoke<DownloadTask[]>("download_enqueue", { items });
};

export const downloadPause = async (id: string): Promise<void> => {
  if (!isTauri()) return;
  await invoke("download_pause", { id });
};

export const downloadResume = async (id: string): Promise<void> => {
  if (!isTauri()) return;
  await invoke("download_resume", { id });
};

export const downloadRetry = async (id: string): Promise<void> => {
  if (!isTauri()) return;
  await invoke("download_retry", { id });
};

export const downloadCancel = async (id: string): Promise<void> => {
  if (!isTauri()) return;
  await invoke("download_cancel", { id });
};

export const downloadClearFinished = async (): Promise<void> => {
  if (!isTauri()) return;
  await invoke("download_clear_finished");
};

// ── Presentation helpers ─────────────────────────────────────────

/** Human label for the destination: the folder name, never a raw `content://`. */
export const dirLabelOf = (settings: DownloadSettings | null): string => {
  if (!settings) return "";
  if (settings.dirLabel) return settings.dirLabel;
  if (!settings.dir) {
    // `defaultDir` is a *desktop* promise — that is where the first download will
    // actually go. On Android the destination is a grant the user has to pick, and
    // the platform default is the app-private folder we deliberately do not use,
    // so showing it would name somewhere nothing will ever be written.
    return settings.requiresPick ? "" : settings.defaultDir;
  }
  if (!settings.dir.startsWith("content://")) return settings.dir;
  // Only reachable for a hand-edited config: the label is written when the picker
  // returns. Decode *before* splitting — a SAF document id escapes its own
  // separators (`primary%3AMusic%2FGMPlayer`), so splitting first leaves
  // `2FGMPlayer`, which is the folder name with two stray characters welded on.
  try {
    const decoded = decodeURIComponent(settings.dir);
    const tail = decoded.split(/[/:]/).pop();
    return tail || settings.dir;
  } catch {
    return settings.dir;
  }
};

/** Whether the destination can be revealed in a file manager. */
export const dirIsRevealable = (settings: DownloadSettings | null): boolean =>
  !!settings?.dir && !settings.dir.startsWith("content://");
