/**
 * NativeQueueOrderSync — the frontend's half of the random-order contract.
 *
 * In random mode the Rust backend owns the permutation (see
 * `player/manifest.rs`, `set_owning_order`). It reports a difference from the
 * order we published as `nativeQueueOrderChanged`, and this module mirrors that
 * into the queue. Mirroring rather than shuffling a copy of our own is the point:
 * two writers is how the screen came to name a different song from the one the
 * planner was playing after the WebView had been away.
 *
 * Tauri-only. The web build has no backend to ask and shuffles in JS.
 */

import { isTauri } from "@/utils/tauri/core/runtime";
import { getAudioBackendTransport } from "@/utils/tauri/audio/transport";
import type { AudioThreadEvent } from "@/utils/tauri/audio/protocol";
// Import the store directly to avoid a cycle through the barrel export.
import useMusicDataStore from "@/store/musicData";

let unsubscribe: (() => void) | null = null;

/** Idempotent. Safe to call from every place that talks to the backend. */
export const installQueueOrderSubscriber = (): void => {
  if (!isTauri() || unsubscribe) return;
  unsubscribe = getAudioBackendTransport().subscribe((event: AudioThreadEvent) => {
    if (event.type !== "nativeQueueOrderChanged") return;
    useMusicDataStore().adoptBackendQueueOrder(event.data.order);
  });
};
