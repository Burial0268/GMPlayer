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

import { isTauri } from "@/utils/tauri/core/runtime";
import { audioGetSession } from "@/utils/tauri/audio/bridge";
import type { TrackIdentity } from "@/utils/tauri/audio/protocol";
// Import the store directly to avoid a cycle through the barrel export.
import useMusicDataStore from "@/store/musicData";
import { reconcileNativeManifestRevision } from "./NativeManifestPublisher";

const IS_DEV = import.meta.env?.DEV ?? false;

const MUSIC_ID_PREFIX = "local:";

/**
 * Hard cap on the session query. `audio_get_session` lazily constructs the Rust
 * player, which on a cold Android start waits for the NDK context — so a first
 * launch can legitimately take seconds. Boot playback must not queue behind
 * that: if the backend cannot answer quickly it is almost certainly not already
 * playing, and the normal startup path is the right fallback.
 */
const SESSION_QUERY_TIMEOUT_MS = 3000;

const withTimeout = <T>(promise: Promise<T>, ms: number): Promise<T | null> =>
  new Promise((resolve) => {
    const timer = setTimeout(() => resolve(null), ms);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      () => {
        clearTimeout(timer);
        resolve(null);
      },
    );
  });

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

  const snapshot = await withTimeout(audioGetSession(), SESSION_QUERY_TIMEOUT_MS);
  if (!snapshot) return null;

  // Keep our manifest counter ahead of the backend's even when we do not adopt:
  // a reload resets the in-memory counter, and publishing a revision the
  // backend already holds would be rejected for the rest of the session.
  reconcileNativeManifestRevision(snapshot.manifestRevision);

  if (!snapshot.hasTrack || !snapshot.identity) return null;
  if (!snapshot.musicId.startsWith(MUSIC_ID_PREFIX)) return null;
  const sourceUrl = snapshot.musicId.slice(MUSIC_ID_PREFIX.length);
  if (!sourceUrl) return null;

  // Only Netease identities map onto a store song; a `local` one would need a
  // path lookup the store cannot do today.
  if (snapshot.identity.provider !== "netease") return null;
  const identityId = snapshot.identity.id;

  const music = useMusicDataStore();
  const playlists = music.persistData.playlists;
  if (!playlists?.length) return null;

  const index = playlists.findIndex((song) => String(song?.id) === identityId);
  if (index < 0) {
    // The backend is on a track this frontend no longer lists (playlist edited
    // in another window, storage rolled back). Let the normal startup path
    // replace it rather than adopting something the UI cannot describe.
    if (IS_DEV) {
      console.warn(`[NativeSessionAdopt] backend track netease:${identityId} not in playlist`);
    }
    return null;
  }

  const songId = Number(playlists[index]?.id);
  if (!Number.isFinite(songId) || songId <= 0) return null;

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

  if (IS_DEV) {
    console.log(
      `[NativeSessionAdopt] adopting netease:${identityId} @ index ${index}, ` +
        `${position.toFixed(1)}s / ${duration.toFixed(1)}s, playing=${snapshot.isPlaying}`,
    );
  }

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
