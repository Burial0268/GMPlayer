import { acceptHMRUpdate, defineStore } from "pinia";
import { asRawEntries } from "@/utils/rawEntry";
import {
  type LocalGroup,
  type LocalPlaylist,
  type LocalScanSummary,
  type LocalSource,
  localFavouriteList,
  localFavouriteSet,
  localGroups,
  localLibraryList,
  localPlaylistList,
  localSourceAddDirectory,
  localSourceAddFiles,
  localSourceList,
  localSourceRemove,
  localSourceRescan,
  localScanCancel,
  localTracksBySongIds,
  localTrackToSongData,
} from "@/utils/localLibrary";
import { onLocalTracksChanged } from "@/utils/localLibraryMutations";
import type { SongData } from "@/store/musicTypes";

interface ScanState {
  active: boolean;
  sourceId: string;
  phase: string;
  done: number;
  total: number;
  current: string;
  error: string;
}

interface LocalLibraryState {
  sources: LocalSource[];
  playlists: LocalPlaylist[];
  /**
   * Local favourites, keyed by **locator**.
   *
   * A plain array rather than a `Set` because pinia state has to survive
   * `$patch` and devtools serialization; membership goes through
   * [`favouriteSet`](#favouriteSet), which is the reactive index over it.
   */
  favourites: string[];
  albums: LocalGroup[];
  artists: LocalGroup[];
  folders: LocalGroup[];
  scan: ScanState;
  /** Whether the first hydrate has completed, so views can skip a re-fetch. */
  hydrated: boolean;
  /**
   * Bumped whenever a track's indexed row changes under us.
   *
   * The list views hold their own row arrays and sit in `<keep-alive>`, so they
   * load `onMounted` and never again — navigating back re-renders the *cached*
   * instance. Watching this is how they hear that a tag correction, a cover or a
   * lyric import happened on a detail page. `trackCount` cannot serve: an edit
   * does not change how many tracks there are.
   */
  revision: number;
}

const emptyScan = (): ScanState => ({
  active: false,
  sourceId: "",
  phase: "",
  done: 0,
  total: 0,
  current: "",
  error: "",
});

/**
 * The local music library, as the UI sees it.
 *
 * **Deliberately not persisted.** Every field here is a cache of what Rust
 * already holds in `$APPDATA/local-library.bin`; `persistData` is localStorage
 * plus a throttled whole-object `JSON.stringify`, so a few thousand track rows
 * in it would turn every volume drag into a multi-megabyte serialization (the
 * reason `musicPersistedData` avoids `pinia-plugin-persistedstate` in the first
 * place).
 *
 * It is also **not** registered with `destroyTauriPiniaStores`, and must not be:
 * that list is the account-state teardown, and local data is the one thing that
 * has to survive login, logout and an account switch. The queue itself still
 * persists — a local row inside `persistData.playlists` is a whole `SongData`
 * including `local.uri`, which is exactly what lets the backend session be
 * adopted again after a restart.
 */
const useLocalLibraryStore = defineStore("localLibrary", {
  state: (): LocalLibraryState => ({
    sources: [],
    playlists: [],
    favourites: [],
    albums: [],
    artists: [],
    folders: [],
    scan: emptyScan(),
    hydrated: false,
    revision: 0,
  }),
  getters: {
    /** Membership index. Rebuilt when `favourites` changes, not per lookup. */
    favouriteSet(state): Set<string> {
      return new Set(state.favourites);
    },
    availableSources(state): LocalSource[] {
      return state.sources.filter((source) => source.available);
    },
    trackCount(state): number {
      return state.sources.reduce((total, source) => total + source.trackCount, 0);
    },
    hasLibrary(state): boolean {
      return state.sources.length > 0;
    },
  },
  actions: {
    /** Load sources, playlists and favourites. Cheap; safe to call repeatedly. */
    async hydrate(force = false) {
      if (this.hydrated && !force) return;
      const [sources, playlists, favourites] = await Promise.all([
        localSourceList(),
        localPlaylistList(),
        localFavouriteList(),
      ]);
      this.sources = sources;
      this.playlists = playlists;
      this.favourites = favourites;
      this.hydrated = true;
    },

    /** Refresh the automatic collections. Separate because it walks the index. */
    async loadGroups() {
      const [albums, artists, folders] = await Promise.all([
        localGroups("album"),
        localGroups("artist"),
        localGroups("folder"),
      ]);
      // `asRawEntries` for the same reason song rows use it: a library with a few
      // thousand albums would otherwise have pinia mint a reactive Dep per field
      // per card, and nothing here is ever mutated in place — a re-scan replaces
      // the whole array. See `utils/rawEntry` and the song path in `page()`.
      this.albums = asRawEntries(albums);
      this.artists = asRawEntries(artists);
      this.folders = asRawEntries(folders);
    },

    /** One page of tracks, already mapped into `SongData`. */
    async page(query: Parameters<typeof localLibraryList>[0] = {}): Promise<{
      songs: SongData[];
      total: number;
    }> {
      const page = await localLibraryList(query);
      return {
        songs: asRawEntries(page.tracks.map(localTrackToSongData)),
        total: page.total,
      };
    },

    async importDirectory(): Promise<LocalScanSummary | null> {
      return this.runScan(() => localSourceAddDirectory(this.onScanProgress));
    },

    /**
     * Pick individual files. With `playlistId` they also land in that playlist,
     * which is what makes "add a song to this playlist" a single action.
     */
    async importFiles(playlistId?: string): Promise<LocalScanSummary | null> {
      const summary = await this.runScan(() =>
        localSourceAddFiles(this.onScanProgress, playlistId),
      );
      if (summary && playlistId) await this.refreshPlaylists();
      return summary;
    },

    async rescan(sourceId: string): Promise<LocalScanSummary | null> {
      return this.runScan(() => localSourceRescan(sourceId, this.onScanProgress));
    },

    /**
     * Shared wrapper so a cancelled picker, a failure and a completed scan all
     * leave `scan.active` false. A scan that never clears the flag makes every
     * later import button inert, which reads as the feature being broken.
     */
    async runScan(run: () => Promise<LocalScanSummary | null>): Promise<LocalScanSummary | null> {
      this.scan = { ...emptyScan(), active: true };
      try {
        const summary = await run();
        if (summary) {
          await this.hydrate(true);
          await this.loadGroups();
        }
        return summary;
      } catch (error) {
        this.scan.error = String(error);
        throw error;
      } finally {
        this.scan.active = false;
      }
    },

    onScanProgress(progress: {
      sourceId: string;
      phase: string;
      done: number;
      total: number;
      current: string;
      error?: string;
    }) {
      this.scan = {
        active: progress.phase !== "done" && progress.phase !== "failed",
        sourceId: progress.sourceId,
        phase: progress.phase,
        done: progress.done,
        total: progress.total,
        current: progress.current,
        error: progress.error ?? "",
      };
    },

    async cancelScan() {
      await localScanCancel();
    },

    async removeSource(sourceId: string) {
      await localSourceRemove(sourceId);
      await this.hydrate(true);
      await this.loadGroups();
    },

    isFavourite(key: string | null | undefined): boolean {
      if (!key) return false;
      return this.favouriteSet.has(key);
    },

    /**
     * Flip a local favourite.
     *
     * Writes through Rust, which owns the set, and mirrors the result locally.
     * Never touches `persistData.likeList`: login replaces that array wholesale
     * from `/likelist`, so a local entry in it would be erased at the next
     * sign-in — and the symptom would point at the login code.
     */
    async setFavourite(key: string, favourite: boolean): Promise<boolean> {
      const changed = await localFavouriteSet(key, favourite);
      if (!changed) return false;
      if (favourite) {
        if (!this.favourites.includes(key)) this.favourites.push(key);
      } else {
        const index = this.favourites.indexOf(key);
        if (index !== -1) this.favourites.splice(index, 1);
      }
      return true;
    },

    async refreshPlaylists() {
      this.playlists = await localPlaylistList();
    },

    /**
     * Re-read the index rows for local tracks already sitting in a list.
     *
     * Needed because the queue *is* persisted while the library is not: a row in
     * `persistData.playlists` is a whole `SongData` snapshot taken when it was
     * queued. After a re-scan that snapshot can be wrong in a way that shows —
     * a re-tagged file keeps its old title, and worse its `coverPath` points at
     * a cover file the scan pruned (covers are content-addressed, so re-tagging
     * produces a new one and drops the old), leaving a broken image.
     *
     * Returns a new array; rows are **replaced** rather than mutated, because
     * every list entry is `markRaw`'d and an in-place field write would not be
     * seen (see `utils/rawEntry`). A track the index no longer knows is left
     * exactly as it was — dropping it would silently shorten the user's queue.
     */
    async refreshRows(songs: SongData[]): Promise<SongData[]> {
      const localIds = songs
        .filter((song) => song?.local?.uri)
        .map((song) => Number(song.id))
        .filter((id) => Number.isFinite(id));
      if (!localIds.length) return songs;

      const rows = await localTracksBySongIds(localIds);
      if (!rows.length) return songs;
      const byId = new Map(rows.map((row) => [row.songId, row]));
      return songs.map((song) => {
        const row = song?.local?.uri ? byId.get(Number(song.id)) : undefined;
        return row ? localTrackToSongData(row) : song;
      });
    },
  },
});

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useLocalLibraryStore, import.meta.hot));
}

/**
 * Keep the library views in step with edits made on a detail page.
 *
 * Registered at module scope rather than from a component, so it exists for as
 * long as anything can emit. `useLocalLibraryStore()` is called lazily inside the
 * handler because at module-evaluation time Pinia may not be installed yet.
 *
 * Deliberately does **not** touch the playback queue: `store/musicData.ts`
 * already imports this module, so reaching back into it would close an import
 * cycle. `components/Player/index.vue` owns that half — it is the playback master
 * and is mounted only in the main window, which keeps one writer on the queue.
 */
onLocalTracksChanged((change) => {
  const store = useLocalLibraryStore();
  store.revision += 1;
  // A download added rows rather than editing one, so the counts are stale too —
  // and `Local/songs.vue` watches `trackCount` alongside `revision`, which only
  // `hydrate` refreshes. Forced, because `hydrated` is already true by then.
  if (change.kind === "added") void store.hydrate(true);
  // Only when they have been loaded at least once. The grids read `albums` /
  // `artists` / `folders` reactively, so refreshing here is what updates them —
  // their own `onMounted` guard is `if (!local.albums.length)`, which never
  // re-fetches a stale non-empty array.
  if (store.albums.length || store.artists.length || store.folders.length) {
    void store.loadGroups();
  }
});

export default useLocalLibraryStore;
