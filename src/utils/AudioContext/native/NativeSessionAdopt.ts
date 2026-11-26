/**
 * NativeSessionAdopt — reconcile the frontend with a backend that is already
 * playing, before the frontend commits to loading anything.
 *
 * Why this exists
 * ---------------
 * On Android the WebView is destroyed and the page reloaded (Activity teardown
 * under memory pressure) while the Rust process — and playback — keeps running.
 * The reloaded frontend rehydrates Pinia from persisted storage, which
 * describes the track that was playing when the app was *backgrounded*, not
 * what is playing now. Without this step the boot path would:
 *
 *   1. resolve a fresh CDN URL for the stale persisted track,
 *   2. fail the existing `allowInitialBackendAttach` check — that compares
 *      `local:<url>` ids, and the URL is re-resolved with a new token every
 *      session, so it can never match,
 *   3. `SetPlaylist` + play the stale track, cutting off live audio, and
 *   4. publish a manifest whose `cursorIdentity` is the stale track, dragging
 *      the planner cursor backwards too (revisions are monotonic across
 *      reloads, so the backend accepts it).
 *
 * i.e. playback visibly rewinds to wherever it was when the app went to the
 * background. Asking the backend first — and matching on `TrackIdentity`, the
 * only key that survives URL re-resolution — turns that into an adoption.
 *
 * Tauri-only. Everywhere else the JS runtime and the audio host share a
 * lifetime, so there is nothing to reconcile.
 */

import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "@/utils/tauri/core/runtime";
import { getAudioBackendTransport } from "@/utils/tauri/audio/transport";
import type { NativeSessionSnapshot, TrackIdentity } from "@/utils/tauri/audio/protocol";
import { trackIdentityKey } from "@/utils/tauri/audio/identity";
// Import the store directly to avoid a cycle through the barrel export.
import useMusicDataStore from "@/store/musicData";
import { reconcileNativeManifestRevision } from "./NativeManifestPublisher";
import { setOrderBase } from "./NativeManifestRevision";
import { adoptNativePersonalFm } from "./NativePersonalFm";
import { logNative } from "@/utils/tauri/core/nativeLog";
import {
  findQueueIndex,
  pickResume,
  planQueueAdoption,
  type LastBackendResume,
} from "./LastBackendSession";
import {
  installPlaybackPhaseSubscriber,
  installQueueOrderSubscriber,
} from "./NativeQueueOrderSync";

const MUSIC_ID_PREFIX = "local:";

export interface AdoptedBackendSession {
  songId: number;
  playlistIndex: number;
  identity: TrackIdentity;
  /**
   * Source the backend is decoding, recovered from its `local:<url>` music id.
   * Reusing it means the adopting controller's expected id matches exactly,
   * on top of the identity check.
   */
  sourceUrl: string;
  position: number;
  duration: number;
  isPlaying: boolean;
}

/**
 * Ask the backend what it is playing and, when that track is in the restored
 * playlist, move the store onto it.
 *
 * Returns the adoption descriptor when the caller should attach to live
 * playback instead of starting a fresh load, or `null` to proceed normally
 * (nothing playing, unknown track, or not running under Tauri).
 *
 * Only touches store fields that describe *which* track and *where* — never
 * the playlist itself, which the frontend still owns.
 */
export const adoptNativeBackendSession = async (): Promise<AdoptedBackendSession | null> => {
  if (!isTauri()) return null;
  // Before the snapshot: an order or phase event arriving after it must be heard.
  // This is the one boot-time install point (a sound, and so a manifest, cannot
  // exist before this runs).
  installQueueOrderSubscriber();
  installPlaybackPhaseSubscriber();

  // The snapshot is adopted inside the `hello` handler, in stream order: every
  // event after it is newer and reaches the subscribers above after it, so
  // nothing adopted here can overwrite a later change. Read beside the stream,
  // as it used to be, it could. A fresh `hello` is asked for because something
  // else may already have connected and consumed the first one.
  const transport = getAudioBackendTransport();
  let adopted: AdoptedBackendSession | null = null;
  let idle = false;
  let freshProcess = false;
  let received = false;
  const unsubscribe = transport.subscribe((event) => {
    if (received || event.type !== "hello") return;
    received = true;
    const { snapshot } = event.data;
    adopted = adoptSnapshot(snapshot);
    idle = !snapshot.hasTrack && !snapshot.personalFm;
    // A process that has seen no manifest: the queue half of the snapshot is a
    // default, and the previous process's record is the backend's answer.
    freshProcess = snapshot.manifestRevision === 0;
  });
  try {
    await transport.resync?.();
  } catch (err) {
    logNative("warn", `[NativeSessionAdopt] could not read the backend session: ${String(err)}`);
  } finally {
    unsubscribe();
  }
  if (idle) await resumeLastBackendSession(freshProcess);
  return adopted;
};

/**
 * Nothing is loaded: resume where the backend's on-disk record says it was,
 * since this page's queue and cursor are stale whenever the backend advanced,
 * or a notification press changed the mode, while the page was frozen. The
 * normal startup path then loads the track.
 */
const resumeLastBackendSession = async (freshProcess: boolean): Promise<void> => {
  const music = useMusicDataStore();
  if (music.persistData.personalFmMode) return;
  let resume: LastBackendResume;
  try {
    resume = await invoke<LastBackendResume>("last_session_get");
  } catch (err) {
    logNative(
      "warn",
      `[NativeSessionAdopt] could not read the last backend session: ${String(err)}`,
    );
    return;
  }

  // Queue first: mirroring an order moves the row the track is found at.
  if (freshProcess) {
    const { mode, backendOrdered, order } = planQueueAdoption(resume?.queue);
    if (mode) music.adoptPlaySongMode(mode, { backendOrdered });
    if (order) {
      music.adoptBackendQueueOrder(order.order, order.manifestRevision, {
        force: true,
        shuffleId: order.shuffleId,
      });
    }
    if (mode) {
      logNative(
        "info",
        `[NativeSessionAdopt] last backend queue: mode=${mode} order=${order?.order.length ?? 0}`,
      );
    }
  }

  const last = resume?.track ?? null;
  const picked = pickResume(
    last,
    music.persistData.playlists ?? [],
    music.persistData.playSongIndex,
  );
  if (!last || !picked) return;
  const { index, position, duration } = picked;
  if (!music.commitPlaySongIndex(index, { currentTime: position, duration })) return;
  // The song watcher, fired by an index move, may reset the clock; re-apply.
  music.setPlaySongTime({ currentTime: position, duration });
  logNative(
    "info",
    `[NativeSessionAdopt] resuming backend's last track ${trackIdentityKey(last.identity)} ` +
      `@ index ${index}, ${position.toFixed(1)}s (saved ${new Date(last.savedAt).toISOString()})`,
  );
};

/** Synchronous by design — see the caller. */
const adoptSnapshot = (snapshot: NativeSessionSnapshot): AdoptedBackendSession | null => {
  // A page recreated mid-track starts from the phase it missed the edge for.
  if (snapshot.hasTrack && snapshot.phase) {
    useMusicDataStore().adoptBackendPlaybackPhase(snapshot.phase);
    useMusicDataStore().bufferedTime = snapshot.bufferedPosition ?? 0;
  }

  // Keep our manifest counter ahead of the backend's even when we do not adopt:
  // a reload resets the in-memory counter, and publishing a revision the
  // backend already holds would be rejected for the rest of the session.
  reconcileNativeManifestRevision(snapshot.manifestRevision);
  if (snapshot.personalFm) adoptNativePersonalFm(snapshot.personalFm);
  logNative(
    "info",
    `[NativeSessionAdopt] snapshot rev=${snapshot.manifestRevision} hasTrack=${snapshot.hasTrack} ` +
      `identity=${trackIdentityKey(snapshot.identity)} mode=${snapshot.playMode} ` +
      `order=${snapshot.order?.length ?? 0} store(mode=${useMusicDataStore().persistData.playSongMode}, ` +
      `index=${useMusicDataStore().persistData.playSongIndex})`,
  );

  // Catch the queue up with the backend BEFORE locating the playing track. The
  // backend owns the random permutation, and a notification press while the page
  // was away may have changed the mode or the order under the persisted queue.
  // `manifestRevision === 0` is a process that has seen no frontend: its mode is
  // a default, not a choice, and adopting it would overwrite the user's.
  if (!snapshot.personalFm && snapshot.manifestRevision > 0) {
    const music = useMusicDataStore();
    if (snapshot.playMode) {
      music.adoptPlaySongMode(snapshot.playMode, { backendOrdered: true });
    }
    if (snapshot.order?.length) {
      music.adoptBackendQueueOrder(snapshot.order, snapshot.manifestRevision, {
        force: true,
        shuffleId: snapshot.shuffleId,
        // Our queue is this order now, so our first publish needs no stale-base answer.
        orderGeneration: snapshot.orderGeneration,
      });
    }
  } else if (!snapshot.personalFm && snapshot.orderGeneration !== undefined) {
    // A process that has seen no manifest takes the first one as it is: whatever queue we publish
    // (this one, or the last session's adopted below) is already its order.
    setOrderBase(snapshot.orderGeneration);
  }

  if (!snapshot.hasTrack || !snapshot.identity) return null;
  if (!snapshot.musicId.startsWith(MUSIC_ID_PREFIX)) return null;
  const sourceUrl = snapshot.musicId.slice(MUSIC_ID_PREFIX.length);
  if (!sourceUrl) return null;

  const music = useMusicDataStore();
  const playlists = music.persistData.playlists;
  if (!playlists?.length) return null;

  // Both providers map onto a store row. Refusing `local` here would send an
  // imported track down the "resolve a fresh URL and restart" path on every
  // reload — which for a local file means restarting from the persisted position
  // while the backend is still playing it.
  const identity = snapshot.identity;
  const index = findQueueIndex(playlists, identity);
  if (index < 0) {
    // The backend is on a track this frontend no longer lists (playlist edited
    // in another window, storage rolled back). Let the normal startup path
    // replace it rather than adopting something the UI cannot describe.
    logNative(
      "warn",
      `[NativeSessionAdopt] backend track ${trackIdentityKey(snapshot.identity)} not in the ` +
        `${playlists.length}-track queue; falling back to a normal start`,
    );
    return null;
  }

  const songId = Number(playlists[index]?.id);
  // `!== 0`: a local id is negative. `0` is what a missing id coerces to.
  if (!Number.isFinite(songId) || songId === 0) return null;

  const duration =
    Number.isFinite(snapshot.duration) && snapshot.duration > 0 ? snapshot.duration : 0;
  const position =
    Number.isFinite(snapshot.position) && snapshot.position > 0 ? snapshot.position : 0;

  // Move the cursor and the clock together. `commitPlaySongIndex` also writes
  // `persistData.playSongIndex` and the resume snapshot, so a *second* reload
  // starts from the adopted track rather than the original stale one.
  if (!music.commitPlaySongIndex(index, { currentTime: position, duration })) return null;
  music.playingSongId = songId;
  // Moving the index fires the Player's `flush: "sync"` song watcher, which
  // resets the clock for what it believes is a track change. Re-apply after it
  // so the UI shows the live position instead of 0 until the first sync event.
  music.setPlaySongTime({ currentTime: position, duration });
  music.setPlayState(snapshot.isPlaying);

  logNative(
    "info",
    `[NativeSessionAdopt] adopting ${trackIdentityKey(identity)} @ index ${index}, ` +
      `${position.toFixed(1)}s / ${duration.toFixed(1)}s, playing=${snapshot.isPlaying}`,
  );

  return {
    songId,
    playlistIndex: index,
    identity: snapshot.identity,
    sourceUrl,
    position,
    duration,
    isPlaying: snapshot.isPlaying,
  };
};
