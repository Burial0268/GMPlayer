/**
 * Playlist sources: one way to ask "give me the songs on page N of this list".
 *
 * Deliberately thin. It covers *fetching a page* and *what may be done to the
 * list*, and nothing else — no header, no cover, no description rendering. The
 * moment it tried to unify detail pages it would have to absorb Netease's
 * `creator` / `tags` / `subscribed` and its read-after-write reconciliation
 * (`utils/playlistMutations.ts`), which is why `PlayListView.vue` stays as it is
 * and the local list gets its own view. What the two genuinely share is
 * `DataLists.vue`, and that is the expensive part.
 */
import type { SongData } from "@/store/musicTypes";
import type { LocalLibraryQuery } from "@/utils/localLibrary";

export type PlaylistRef =
  | { kind: "netease"; id: number }
  | { kind: "local-all" }
  | { kind: "local-favourites" }
  /**
   * A folder inside an imported source.
   *
   * Both parts are required and `folder` may legitimately be `""` — that is the
   * source's own root, not "no filter". Collapsing the two (`folder || undefined`)
   * makes the root folder select the entire library.
   */
  | { kind: "local-folder"; sourceId: string; folder: string }
  | { kind: "local-album"; album: string }
  | { kind: "local-artist"; artist: string }
  | { kind: "local-playlist"; id: string };

export interface PlaylistCapabilities {
  /** Whether rows can be dragged into a new order. */
  reorder: boolean;
  /** Whether a row can be taken out of this list. */
  remove: boolean;
  /** Whether "add to a Netease playlist" applies. */
  addToNetease: boolean;
  /** Whether the list has comments. */
  comment: boolean;
  /** Whether tracks here can be downloaded from Netease. */
  download: boolean;
  /** Whether "show in folder" applies. */
  revealInFolder: boolean;
  /** Whether the "collect" (subscribe) action applies. */
  subscribe: boolean;
}

export interface PlaylistMeta {
  name: string;
  description: string;
  coverUrl: string;
  trackCount: number;
  totalDurationMs: number;
  /** Whether the list's membership can be edited at all. */
  writable: boolean;
}

export interface PlaylistPage {
  songs: SongData[];
  total: number;
}

export interface PlaylistSource {
  ref: PlaylistRef;
  capabilities: PlaylistCapabilities;
  meta(): Promise<PlaylistMeta>;
  page(offset: number, limit: number): Promise<PlaylistPage>;
}

/**
 * What a Netease playlist can do. The baseline every existing menu was written
 * against, spelled out so the local variants are diffs against something real
 * rather than against an implicit default.
 */
export const NETEASE_CAPABILITIES: PlaylistCapabilities = {
  reorder: false,
  remove: true,
  addToNetease: true,
  comment: true,
  download: true,
  revealInFolder: false,
  subscribe: true,
};

/**
 * A local *automatic* collection — all tracks, an album, an artist, a folder.
 *
 * Nothing about membership is editable: it is derived from the files on disk, so
 * "remove from this list" would have to mean "delete the file", which is not
 * something a list row should be able to do.
 */
export const LOCAL_AUTO_CAPABILITIES: PlaylistCapabilities = {
  reorder: false,
  remove: false,
  addToNetease: false,
  comment: false,
  download: false,
  revealInFolder: true,
  subscribe: false,
};

/** A local playlist the user built. Ordered and editable. */
export const LOCAL_PLAYLIST_CAPABILITIES: PlaylistCapabilities = {
  reorder: true,
  remove: true,
  addToNetease: false,
  comment: false,
  download: false,
  revealInFolder: true,
  subscribe: false,
};

export const isLocalRef = (ref: PlaylistRef): boolean => ref.kind !== "netease";

/** Capabilities for a ref, without having to build the whole source. */
export const capabilitiesFor = (ref: PlaylistRef): PlaylistCapabilities => {
  switch (ref.kind) {
    case "netease":
      return NETEASE_CAPABILITIES;
    case "local-playlist":
      return LOCAL_PLAYLIST_CAPABILITIES;
    default:
      return LOCAL_AUTO_CAPABILITIES;
  }
};

/** The `local_library_list` query a ref maps onto. `null` for Netease. */
export const queryForRef = (ref: PlaylistRef): LocalLibraryQuery | null => {
  switch (ref.kind) {
    case "local-all":
      return {};
    case "local-favourites":
      return { favouritesOnly: true };
    case "local-album":
      return { album: ref.album };
    case "local-artist":
      return { artist: ref.artist };
    case "local-folder":
      return { sourceId: ref.sourceId, folder: ref.folder };
    case "local-playlist":
      return { playlistId: ref.id };
    default:
      return null;
  }
};

/** Serialize a ref into route query params, and back. */
export const refToQuery = (ref: PlaylistRef): Record<string, string> => {
  switch (ref.kind) {
    case "netease":
      return { kind: "netease", id: String(ref.id) };
    case "local-album":
      return { kind: "local-album", album: ref.album };
    case "local-artist":
      return { kind: "local-artist", artist: ref.artist };
    case "local-folder":
      return { kind: "local-folder", sourceId: ref.sourceId, folder: ref.folder };
    case "local-playlist":
      return { kind: "local-playlist", id: ref.id };
    default:
      return { kind: ref.kind };
  }
};

export const refFromQuery = (query: Record<string, unknown>): PlaylistRef => {
  const kind = String(query.kind ?? "local-all");
  const str = (key: string): string => (query[key] === undefined ? "" : String(query[key]));
  switch (kind) {
    case "netease":
      return { kind: "netease", id: Number(str("id")) };
    case "local-album":
      return { kind: "local-album", album: str("album") };
    case "local-artist":
      return { kind: "local-artist", artist: str("artist") };
    case "local-folder":
      // `str` already turns a missing key into `""`, which is exactly the value
      // the root folder needs — do not coerce it to `undefined`.
      return { kind: "local-folder", sourceId: str("sourceId"), folder: str("folder") };
    case "local-playlist":
      return { kind: "local-playlist", id: str("id") };
    case "local-favourites":
      return { kind: "local-favourites" };
    default:
      return { kind: "local-all" };
  }
};
