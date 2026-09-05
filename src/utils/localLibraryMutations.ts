/**
 * localLibraryMutations — the one signal every local-library write emits, and
 * every surface that shows a local track listens to.
 *
 * ## Why this exists
 *
 * Editing a local track's tags, cover or lyrics reached exactly one place: the
 * detail page that made the edit, which reloads itself. Five other surfaces kept
 * a copy and had no way to hear about it:
 *
 * - `persistData.playlists` holds a whole `SongData` **snapshot** per queued
 *   track, taken when it was queued. `refreshRows` re-reads them, but it is
 *   called once, from `App.vue` at startup.
 * - The mini player and the queue panel render straight off that snapshot.
 * - Every slave window (tray popup, taskbar lyrics, mini player, desktop lyrics)
 *   mirrors it through `broadcastPlayerState`, whose payload reads
 *   `music.getPlaySongData?.album?.picUrl`.
 * - The OS media session resolves its metadata from the **native manifest**,
 *   which the frontend publishes from that same snapshot. Re-announcing cannot
 *   fix it: `metadata_fetch::set_announcement` deliberately refuses an
 *   announcement for the already-loaded track, so a republish of the manifest is
 *   the only route. (`apply_native_manifest` calls `publish_now_playing` for
 *   exactly this case — its comment says "cover art the frontend only just had".)
 * - The list views sit in `<keep-alive :max="10">` and load `onMounted`, so
 *   navigating back re-renders the *cached* instance and issues no request.
 *
 * ## Shape
 *
 * Plain callbacks rather than reactive state, like `playlistMutations`: this is
 * an event, and a subscriber that patches on one has no use for its history.
 * `onLocalTracksChanged` returns its own unsubscribe so a view can tie it to its
 * lifetime.
 *
 * Emitted from the write wrappers in `utils/localLibrary.ts` themselves rather
 * than from each call site. Forgetting it is invisible — the detail page refreshes
 * either way, so the edit looks like it worked and only the rest of the app is
 * wrong. That is the opposite of `playlistMutations`, where the caller emits
 * because it has to wait for the account to accept the write; a local `invoke`
 * has already been persisted by the time it returns.
 */

/** Which copy of a track went stale, so a subscriber can skip work it does not need. */
export type LocalTrackChangeKind =
  /** Title, artist, album, track/disc number, year. */
  | "tags"
  /** Cover art — the picked import or its removal. */
  | "cover"
  /** An imported lyric, or its removal. */
  | "lyric"
  /**
   * A track the library did not have a moment ago — a finished download.
   *
   * Unlike the other three this is not an edit to something on screen: the row
   * count changed, so the list views have to re-page and the source's
   * `trackCount` has to be re-read. Nothing about the playing track can be
   * affected, because it cannot have been playing a file that did not exist.
   */
  | "added";

export interface LocalTracksChanged {
  /** Track locators, exactly as Rust keys them. Empty is a no-op. */
  keys: string[];
  kind: LocalTrackChangeKind;
}

type Handler = (change: LocalTracksChanged) => void;

const handlers = new Set<Handler>();

/**
 * Announce an edit Rust has already stored.
 *
 * Safe to call with an empty or partly-unknown key list; subscribers filter.
 */
export const notifyLocalTracksChanged = (change: LocalTracksChanged): void => {
  if (!change.keys.length) return;
  for (const handler of handlers) {
    try {
      handler(change);
    } catch (err) {
      // One bad subscriber must not stop the others: a half-propagated change is
      // the state this module exists to prevent.
      console.error("[localLibraryMutations] subscriber failed", err);
    }
  }
};

/**
 * Subscribe. Returns the unsubscribe — call it from `onUnmounted`, or a
 * kept-alive view keeps refetching data nobody is looking at.
 */
export const onLocalTracksChanged = (handler: Handler): (() => void) => {
  handlers.add(handler);
  return () => {
    handlers.delete(handler);
  };
};
