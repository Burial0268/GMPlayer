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
import { onBackendEvent } from "@/utils/tauri/audio/backendEvents";
// Import the store directly to avoid a cycle through the barrel export.
import useMusicDataStore from "@/store/musicData";
import { logNative } from "@/utils/tauri/core/nativeLog";
import { getManifestRevision } from "./NativeManifestRevision";

let unsubscribe: (() => void) | null = null;
let unsubscribePhase: (() => void) | null = null;

/**
 * The page's loading display follows the backend's phase, not the lifecycle of
 * whichever sound object happened to be bound: a backend-driven advance reuses
 * the sound, so per-sound `load`/`waiting` handlers never saw the next track.
 */
export const installPlaybackPhaseSubscriber = (): void => {
  if (!isTauri() || unsubscribePhase) return;
  unsubscribePhase = onBackendEvent("playbackPhaseChanged", (event) => {
    useMusicDataStore().adoptBackendPlaybackPhase(event.data.phase);
  });
};

/** Idempotent. Safe to call from every place that talks to the backend. */
export const installQueueOrderSubscriber = (): void => {
  if (!isTauri() || unsubscribe) return;
  unsubscribe = onBackendEvent("nativeQueueOrderChanged", (event) => {
    // An order describes the list of the revision it was computed for. If we have
    // published a newer one since, this event was overtaken in transit: applying
    // it would lay an old permutation over the edited list, and the backend sends
    // no correction when its rebased order for the newer revision already equals
    // what we published. Whatever the newer revision needs, it reports itself.
    const published = getManifestRevision();
    if (event.data.manifestRevision < published) {
      logNative(
        "info",
        `[queueOrder] dropped order for superseded revision ${event.data.manifestRevision} (published ${published})`,
      );
      return;
    }
    useMusicDataStore().adoptBackendQueueOrder(event.data.order, event.data.manifestRevision, {
      shuffleId: event.data.shuffleId,
    });
  });
};
