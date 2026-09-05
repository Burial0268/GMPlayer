/**
 * NativeSessionControlsSync — the frontend's half of the session-controls
 * subscription.
 *
 * Play mode and "favourite" are shown on three surfaces (the app, the OS
 * notification, the lock screen) and settable from all of them. Three writers
 * for one value is how they drift, so there is exactly one copy and it lives in
 * the backend — see `player/session_controls.rs`. This module is the frontend's
 * end of that contract, and it does two things and nothing else:
 *
 * - **publish** what the frontend knows. Play mode is a persisted setting the UI
 *   owns; the like state comes from the account's likelist, which only the
 *   frontend fetches. Both are pushed as a patch, so neither claims the other.
 * - **adopt** what the backend reports. A `sessionControlsChanged` is the
 *   authority — including for a change this frontend asked for, which is what
 *   makes the OS button and the in-app button behave identically.
 *
 * Adoption deliberately does *not* re-publish: the backend only emits on a real
 * change, so adopting is a fixed point. Writing the store here and letting its
 * own watchers publish would be an infinite round trip.
 *
 * Tauri-only. On the web the page is the only surface, so it owns both values
 * directly and there is nothing to reconcile.
 */

import { isTauri } from "@/utils/tauri/core/runtime";
import { getAudioBackendTransport } from "@/utils/tauri/audio/transport";
import type { AudioThreadEvent, SessionControls } from "@/utils/tauri/audio/protocol";
import { musicStore, userStore, useLocalLibraryStore } from "@/store";
import { publishNativeManifest } from "./NativeManifestPublisher";

const IS_DEV = import.meta.env?.DEV ?? false;

type PlayMode = SessionControls["playMode"];

let unsubscribe: (() => void) | null = null;
let lastPublished = "";
/**
 * Set while a backend value is being written into the store.
 *
 * The store's own watchers publish on change, and a value that arrived *from*
 * the backend must not be sent straight back — harmless but pointless traffic on
 * every mode change, and it makes the logs unreadable.
 */
let adopting = false;

/** Whether the store is currently applying a backend-supplied value. */
export const isAdoptingSessionControls = (): boolean => adopting;

/**
 * Push the frontend-known controls to the backend. Idempotent: skips the IPC
 * when nothing changed, so it is safe to call from a watcher.
 */
export const publishSessionControls = (options: { force?: boolean } = {}): void => {
  if (!isTauri() || adopting) return;

  // Self-install. The subscriber used to be wired only from
  // `useNativeMediaControls.onMounted`, *after* two awaits — and an async
  // `onMounted` that rejects (the notification-permission round trip can) is
  // swallowed by Vue, leaving a frontend that publishes but never listens. That
  // is invisible in the direction we test most and total in the other: the
  // notification followed the app, the app never followed the notification.
  // Anchoring it here means anything that talks to the backend also hears it.
  installSessionControlsSubscriber();

  const music = musicStore();
  const user = userStore();
  const songId = Number(music.playingSongId ?? music.getPlaySongData?.id);
  // A local file has a *negative* id, so `songId > 0` is not the test for
  // "there is a track" — it is the test for "there is a Netease track".
  const localKey = Number.isFinite(songId) ? music.localSongRef(songId) : null;
  const isLocal = Boolean(localKey);
  const hasTrack = Number.isFinite(songId) && (isLocal || songId > 0);
  // The backend derives the like state from a `/likelist` it fetches itself, so
  // it is never blind. Ours is the fast path — but only while we actually have a
  // list: on a cold start `likeList` is empty until `setLikeList` lands, and
  // publishing `favourite: false` from an empty list would overwrite a correct
  // `true` with a wrong one. Patch semantics let us simply not claim it.
  //
  // That protection is about the *Netease* list and must not extend to a local
  // track: its set is local and authoritative, so withholding the value there
  // would mean a local favourite never reaches the notification at all.
  const likelistLoaded = isLocal || music.persistData.likeList.length > 0;

  const controls: {
    playMode: PlayMode;
    favourite?: boolean;
    canFavourite: boolean;
  } = {
    playMode: (music.persistData.playSongMode || "normal") as PlayMode,
    // Only meaningful for a Netease track we have a likelist for. A logged-out
    // user gets `canFavourite: false` rather than a heart that fails on tap.
    // A local file needs no account at all, so it is always favouritable.
    canFavourite: hasTrack && (isLocal || Boolean(user.userLogin)),
  };
  if (hasTrack && likelistLoaded) {
    controls.favourite = music.getSongIsLike(songId);
  }

  const serialized = JSON.stringify(controls);
  if (!options.force && serialized === lastPublished) return;
  lastPublished = serialized;

  try {
    getAudioBackendTransport().sendOrQueue({ type: "setSessionControls", controls });
  } catch (err) {
    if (IS_DEV) console.warn("[SessionControls] publish failed", err);
  }
};

/** Force a re-push on the next call (e.g. after the backend was replaced). */
export const invalidateSessionControls = (): void => {
  lastPublished = "";
};

/**
 * Ask the backend to advance the play mode.
 *
 * The in-app control routes through here rather than writing the store, so the
 * app and the notification take the same path and cannot disagree about what
 * "next mode" means.
 */
export const requestNextPlayMode = (): boolean => {
  if (!isTauri()) return false;
  try {
    getAudioBackendTransport().sendOrQueue({ type: "setNextPlayMode" });
    return true;
  } catch {
    return false;
  }
};

/**
 * Ask the backend to toggle the current track's like state.
 *
 * Returns `false` when there is no backend to ask, so the caller can fall back
 * to its own API call — which is what the web build always does.
 */
export const requestToggleFavourite = (): boolean => {
  if (!isTauri()) return false;
  try {
    getAudioBackendTransport().sendOrQueue({ type: "toggleFavourite" });
    return true;
  } catch {
    return false;
  }
};

/**
 * Write a backend-reported value into the store.
 *
 * Only the fields the backend is authoritative about are touched, and the
 * likelist is updated in place rather than refetched: the backend has already
 * performed the account call, so a refetch would be a second round trip to learn
 * something we were just told.
 */
const adoptSessionControls = (controls: SessionControls): void => {
  const music = musicStore();
  let modeChanged = false;
  adopting = true;
  try {
    if (
      controls.playMode &&
      controls.playMode !== music.persistData.playSongMode &&
      // `setPlaySongMode` is the interactive path: it toasts, reseeds the random
      // traversal and republishes the manifest. None of that is right for a
      // value the backend has *already* applied to its own planner.
      ["normal", "random", "single"].includes(controls.playMode)
    ) {
      music.persistData.playSongMode = controls.playMode;
      modeChanged = true;
    }

    const songId = Number(music.playingSongId ?? music.getPlaySongData?.id);
    if (Number.isFinite(songId) && controls.canFavourite) {
      // A local track's like state belongs to the local set, not to `likeList`.
      // Routing it through `applyLikeState` would push a negative id into the
      // array login replaces wholesale — the value would be silently dropped at
      // the next sign-in, and would meanwhile make 我喜欢的音乐 report a phantom
      // extra track.
      const localKey = music.localSongRef(songId);
      if (localKey) {
        // Mirror the backend's value locally; it already performed the write.
        void useLocalLibraryStore().setFavourite(localKey, Boolean(controls.favourite));
      } else if (songId > 0) {
        music.applyLikeState(songId, controls.favourite);
      }
    }
  } finally {
    adopting = false;
  }
  // Keep the dedup key in step with what the backend now holds, so the next
  // publish compares against reality rather than our last guess.
  lastPublished = "";

  if (modeChanged) {
    // The backend rebuilt its own traversal when it applied the mode, and the
    // frontend now has a different one — its own list order. Republishing is
    // the convergence point: it hands the backend the order the UI is actually
    // showing, so "next up" and what plays next are the same thing. Safe from a
    // loop because a manifest publish never publishes controls.
    publishNativeManifest({ force: true });
  }
};

/**
 * Subscribe to the backend's session controls. Idempotent; returns the
 * unsubscribe so a caller that owns the lifetime can drop it.
 */
export const installSessionControlsSubscriber = (): (() => void) => {
  if (!isTauri()) return () => {};
  if (unsubscribe) return unsubscribe;

  const transport = getAudioBackendTransport();
  const drop = transport.subscribe((event: AudioThreadEvent) => {
    if (event.type === "sessionControlsChanged") {
      adoptSessionControls(event.data.controls);
      return;
    }
    // The projection carries them too, and it is what a freshly attached
    // controller sees first — a reload adopts the live values from here rather
    // than waiting for the next change.
    if (event.type === "nowPlayingChanged" && event.data.info?.controls) {
      adoptSessionControls(event.data.info.controls);
    }
  });

  unsubscribe = () => {
    drop();
    unsubscribe = null;
  };
  return unsubscribe;
};
