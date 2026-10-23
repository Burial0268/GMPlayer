/**
 * playlistMutations — the one signal every playlist write emits and every
 * playlist view listens to.
 *
 * A playlist write reaches the user through three layers that do not know about
 * each other: the row list on an open playlist page, the track count on the
 * sidebar and the cards, and `ncm-core`'s response cache. Before this module
 * only the last one was handled — `invalidated_by` drops the stale entries, so a
 * *re-navigation* refetched correctly — which left the case that actually shows:
 * while the page is open, un-liking a song left its row sitting there and the
 * count unchanged. Nothing was wrong in the account; the UI simply had no way to
 * hear about it.
 *
 * The shape is patch-then-reconcile, and both halves are load-bearing:
 *
 * - **Patch** immediately, from what the mutating code already knows. Costs no
 *   request, so it cannot fail or lag, and it is what makes the change feel like
 *   it happened rather than like it was fetched.
 * - **Reconcile** afterwards, quietly, for the things a patch cannot know:
 *   the position Netease assigns a new track, what that pushes off the end of a
 *   page, the real total. Debounced, so a burst of taps costs one request.
 *
 * Patching *first* is not just for latency. Netease is read-after-write
 * inconsistent on `/playlist/track/all`: a refetch issued immediately after
 * `/like` can hand back the pre-write list. A reconcile-only design would
 * therefore re-show the row it had just removed, which is worse than not
 * updating at all — so consumers keep a pending delta and re-apply it over
 * whatever the server says until the server agrees. See
 * `PlayListView.reconcileQuietly`.
 *
 * Subscribers are plain callbacks rather than reactive state because there is
 * nothing to *render* here — this is an event, and a component that patches on
 * one has no use for its history. `onPlaylistChanged` returns its own
 * unsubscribe so a view can tie it to its lifetime.
 *
 * There is deliberately no "a playlist was created / deleted / subscribed"
 * variant. That changes the *set* of the user's playlists, which only the
 * account can answer, and every one of those call sites already refreshes
 * through `setUserPlayLists`. A signal nobody could act on differently would be
 * one more thing to keep in step for no gain.
 */

import type { SongData } from "@/store/musicTypes";
import { userStore } from "@/store";

/** Netease's own id for 我喜欢的音乐 is the user's first *created* playlist. */
export const likedPlaylistId = (): number | null => {
  const own = userStore().getUserPlayLists?.own;
  const id = Number(own?.[0]?.id);
  return Number.isFinite(id) && id > 0 ? id : null;
};

export type PlaylistChange =
  /**
   * Tracks were added to or removed from one playlist.
   *
   * `added` carries whole songs where the caller had them, so a row can appear
   * without waiting for a request; `addedIds` covers the callers that only know
   * an id (the notification's heart knows a `TrackIdentity`, not a row). Either
   * way `reconcile` fills in what the patch could not.
   */
  | {
      kind: "tracks";
      playlistId: number;
      added?: SongData[];
      addedIds?: number[];
      removedIds?: number[];
    }
  /** Name, description, tags or cover changed — the header, not the rows. */
  | { kind: "meta"; playlistId: number };

type Handler = (change: PlaylistChange) => void;

const handlers = new Set<Handler>();

/**
 * Announce a playlist write that has already been accepted by the account.
 *
 * Call it *after* the API call succeeds, never optimistically before: a
 * subscriber patches on this, and a patch for a write Netease refused would have
 * to be un-patched, which is precisely the flicker this module exists to avoid.
 * The in-app heart is the exception that proves the rule — `changeLikeList`
 * awaits `/like` and only then reports.
 */
export const notifyPlaylistChanged = (change: PlaylistChange): void => {
  applyToUserPlaylists(change);
  for (const handler of handlers) {
    try {
      handler(change);
    } catch (err) {
      // One bad subscriber must not stop the others from updating — a
      // half-propagated change is the state this module exists to prevent.
      console.error("[playlistMutations] subscriber failed", err);
    }
  }
};

/** Convenience for the commonest case: one track in or out of one playlist. */
export const notifyTrackInPlaylist = (
  playlistId: number | null | undefined,
  song: SongData | number | null | undefined,
  present: boolean,
): void => {
  const pid = Number(playlistId);
  if (!Number.isFinite(pid) || pid <= 0) return;
  const id = Number(typeof song === "object" && song !== null ? song.id : song);
  if (!Number.isFinite(id) || id <= 0) return;

  if (!present) {
    notifyPlaylistChanged({ kind: "tracks", playlistId: pid, removedIds: [id] });
    return;
  }
  notifyPlaylistChanged({
    kind: "tracks",
    playlistId: pid,
    ...(typeof song === "object" && song !== null ? { added: [song] } : { addedIds: [id] }),
  });
};

/**
 * Subscribe. Returns the unsubscribe — call it from `onUnmounted`, or a
 * kept-alive view will keep patching data nobody is looking at.
 */
export const onPlaylistChanged = (handler: Handler): (() => void) => {
  handlers.add(handler);
  return () => {
    handlers.delete(handler);
  };
};

/**
 * "Something in this playlist changed, but I cannot say what."
 *
 * Reconcile-only: no row patch and no count patch, just the quiet refetch. For
 * writes whose effect on a specific list we do not model — `fm_trash` is the
 * case that matters, since it lands the track in the FM bin and, if it was
 * liked, takes it out of 我喜欢的音乐 too. Guessing a delta there would risk a
 * count that is wrong until the next navigation, which is worse than paying one
 * request to be right.
 */
export const notifyPlaylistNeedsReconcile = (playlistId: number | null | undefined): void => {
  const pid = Number(playlistId);
  if (!Number.isFinite(pid) || pid <= 0) return;
  notifyPlaylistChanged({ kind: "tracks", playlistId: pid });
};

/**
 * Keep the user's own playlist list in step, so the sidebar and the cards move
 * with the page.
 *
 * Done here rather than in each caller because it is the same edit every time
 * and forgetting it is invisible — the count is a number nobody looks at until
 * it disagrees. `meta` is deliberately not handled: what a playlist is called
 * can only come from the account, and that call site already refreshes through
 * `setUserPlayLists`.
 *
 * The entry is **replaced**, not mutated. Playlist entries are `markRaw`'d (see
 * `utils/rawEntry`) so that a deep persist watcher does not mint a Dep per
 * field, which also means writing `entry.trackCount` reaches no reactivity at
 * all; assigning the array slot is what re-renders.
 */
const applyToUserPlaylists = (change: PlaylistChange): void => {
  if (change.kind !== "tracks") return;
  const delta =
    (change.added?.length ?? 0) + (change.addedIds?.length ?? 0) - (change.removedIds?.length ?? 0);
  if (delta === 0) return;
  userStore().applyPlaylistTrackDelta(change.playlistId, delta);
};
