/**
 * What the native app keeps on disk for the user: sizes, clearing, and the audio cache budget.
 * See `src-tauri/src/storage.rs`.
 */
import { invoke } from "@tauri-apps/api/core";

export type StorageKind = "audio" | "api" | "localStream";

export interface StorageUsage {
  /** Played tracks kept for replay. */
  audio: number;
  /** Cached Netease API responses. */
  api: number;
  /** Copies of local documents that cannot be read in place (Android). */
  localStream: number;
}

export const storageUsage = (): Promise<StorageUsage> => invoke<StorageUsage>("storage_usage");

/** Resolves to the bytes freed. A file in use (the playing track) survives. */
export const storageClear = (kind: StorageKind): Promise<number> =>
  invoke<number>("storage_clear", { kind });

/** `0` turns the audio cache off. */
export const storageSetAudioBudget = (bytes: number): Promise<void> =>
  invoke("storage_set_audio_budget", { bytes });
