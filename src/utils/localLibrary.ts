/**
 * The local music library's command layer.
 *
 * Every function here is Tauri-only and returns an empty/false answer on the
 * web build rather than throwing, so callers can be written once and gated at
 * the UI level. Nothing in this file knows about Vue.
 *
 * ## Why ids are not computed here
 *
 * A local track's `SongData.id` is a negative hash of its locator, assigned in
 * Rust (`local::index::local_song_id`) and shipped with every row. It is
 * deliberately **not** recomputed in TypeScript. Two implementations of one hash
 * is the exact trap `identity.ts` ↔ `TrackIdentity::key()` already carries a
 * warning about, and there is no need for a second one: the id always travels
 * with the row, and `persistData.playlists` stores whole rows.
 */
import { Channel, convertFileSrc, invoke } from "@tauri-apps/api/core";
import { isTauri } from "@/utils/tauri/core/runtime";
import { asRawEntry } from "@/utils/rawEntry";
import { getSongTime } from "@/utils/timeTools";
import { notifyLocalTracksChanged } from "@/utils/localLibraryMutations";
import type { SongData } from "@/store/musicTypes";

// ── Wire types (mirror `src-tauri/src/local/`) ───────────────────

export type LocalSourceKind = "directory" | "files";

export interface LocalSource {
  id: string;
  kind: LocalSourceKind;
  locator: string;
  displayName: string;
  available: boolean;
  lastScannedAt: number;
  trackCount: number;
  members: string[];
}

export interface LocalTrack {
  key: string;
  songId: number;
  sourceId: string;
  title: string;
  artist: string;
  album: string;
  albumArtist: string;
  durationMs: number;
  sampleRate: number;
  channels: number;
  bitrateBps: number | null;
  codec: string;
  trackNo: number | null;
  discNo: number | null;
  year: number | null;
  size: number | null;
  modifiedAt: number | null;
  coverKey: string | null;
  needsCache: boolean;
  displayName: string;
  relativeDir: string;
  lyricKey: string | null;
  /** Absolute path to the extracted cover. */
  coverPath: string | null;
  favourite: boolean;
  /** Whether any field above came from a user correction rather than the file. */
  overridden: boolean;
}

export interface LocalLibraryPage {
  tracks: LocalTrack[];
  total: number;
}

export interface LocalGroup {
  id: string;
  name: string;
  subtitle: string;
  trackCount: number;
  totalDurationMs: number;
  coverPath: string | null;
  /**
   * The query fields that select this group, straight from Rust.
   *
   * Only present for folders, where the display label is *not* the identity: two
   * sources can each hold a `Disc 1`, so the pair `(sourceId, folder)` is what
   * distinguishes them. `folder: ""` means the source's own root.
   */
  sourceId?: string;
  folder?: string;
}

export interface LocalPlaylist {
  id: string;
  name: string;
  description: string;
  createdAt: number;
  updatedAt: number;
  tracks: string[];
  importedFrom: string | null;
}

export interface LocalScanSummary {
  source: LocalSource;
  added: number;
  updated: number;
  removed: number;
  skipped: number;
  failed: number;
  truncated: boolean;
}

export interface ScanProgress {
  sourceId: string;
  phase: "listing" | "tagging" | "done" | "failed";
  done: number;
  total: number;
  current: string;
  error?: string;
}

export interface LocalLibraryQuery {
  sourceId?: string;
  album?: string;
  artist?: string;
  folder?: string;
  playlistId?: string;
  favouritesOnly?: boolean;
  keyword?: string;
  sort?: "title" | "artist" | "album" | "added" | "duration";
  descending?: boolean;
  offset?: number;
  limit?: number;
}

export interface M3uImportResult {
  playlist: LocalPlaylist;
  matched: number;
  missing: number;
}

/** Which parser family a lyric belongs to. Mirrors Rust's `LyricKind`. */
export type LyricKind = "lrc" | "word" | "ttml";

/** Where the lyric currently shown came from. Mirrors Rust's `LocalLyricSource`. */
export type LocalLyricSource = "imported" | "embedded" | "sidecar";

export interface LocalLyric {
  text: string;
  kind: LyricKind;
  source: LocalLyricSource;
  /**
   * Translation and romanisation for whichever `source` won.
   *
   * Companions rather than alternatives: the parser aligns them onto the main
   * lyric's lines, and they are spelled the way `parseLyricData` already reads
   * them. Written by the download queue into the library's own lyric store,
   * because an audio container has one lyric field our reader can find again and a
   * `.lrc` sidecar is a plain lyric by definition.
   */
  tlyric: string | null;
  romalrc: string | null;
}

export interface LyricImport {
  file: string;
  kind: LyricKind;
  originalName: string;
  importedAt: number;
}

/**
 * Cover art the user picked for a track. Mirrors Rust's `CoverImport`.
 *
 * `file` names a picture in the same content-addressed cache directory the scan
 * writes to, so a cover applied across an album is one file. It is layered onto
 * the track's `coverKey` on the way out of Rust, which is why nothing here has to
 * know about it — `coverPath` is simply already right.
 */
export interface CoverImport {
  file: string;
  originalName: string;
  importedAt: number;
}

/**
 * A user's tag corrections. Every field is optional and `null`/absent means
 * "use what the file said", which is what makes reverting simply an empty patch.
 *
 * Sent whole, never merged: what the form holds is what gets stored.
 */
export interface TrackOverride {
  title?: string | null;
  artist?: string | null;
  album?: string | null;
  albumArtist?: string | null;
  trackNo?: number | null;
  discNo?: number | null;
  year?: number | null;
  updatedAt?: number;
}

/** The scanned row, before any correction. */
export type LocalScannedTrack = Omit<LocalTrack, "coverPath" | "favourite" | "overridden">;

export interface LocalLyricStatus {
  imported: LyricImport | null;
  hasSidecar: boolean;
}

export interface LocalCoverStatus {
  imported: CoverImport | null;
  /** Whether the file carried art of its own at scan time. */
  hasEmbedded: boolean;
  /** Tracks in this album, this one included. `> 1` is what offers the bulk apply. */
  albumTrackCount: number;
}

/** Everything the song detail page needs, in one round trip. */
export interface LocalTrackDetail {
  view: LocalTrack;
  scanned: LocalScannedTrack;
  patch: TrackOverride | null;
  lyric: LocalLyricStatus;
  cover: LocalCoverStatus;
  sourceName: string;
  sourceLocator: string | null;
}

// ── SongData mapping ─────────────────────────────────────────────

/**
 * The placeholder shown for a tag the file did not carry.
 *
 * Rust deliberately stores an empty string rather than a localized word: the
 * language lives here, and an index full of "未知艺术家" would be wrong the
 * moment the user switched languages.
 */
const UNKNOWN = "";

/**
 * Turn an index row into the `SongData` the player, the queue and `DataLists`
 * all expect.
 *
 * `markRaw`'d like every other list entry — the persist watcher is deep, and a
 * few thousand un-raw'd rows cost megabytes of dependency graph that nothing
 * ever triggers (see `utils/rawEntry`).
 */
export const localTrackToSongData = (track: LocalTrack): SongData => {
  // Two URLs for one image, on purpose. The webview can only load the asset
  // protocol; the OS media session can only load the real path.
  const cover = track.coverPath ? convertFileSrc(track.coverPath) : "";
  return asRawEntry<SongData>({
    id: track.songId,
    name: track.title || track.displayName,
    artist: [{ id: 0, name: track.artist || UNKNOWN }],
    album: { id: 0, name: track.album || UNKNOWN, picUrl: cover },
    alia: [],
    time: getSongTime(track.durationMs),
    dt: track.durationMs,
    // Never a paid track: `fee`/`pc` drive the Netease VIP pre-check, and a
    // local file must not be run through it.
    fee: 0,
    pc: undefined,
    mv: 0,
    // The one field business logic branches on.
    local: {
      uri: track.key,
      sourceId: track.sourceId,
      coverPath: track.coverPath ?? undefined,
      coverKey: track.coverKey ?? undefined,
      needsCache: track.needsCache,
    },
    // Kept for the library UI; harmless elsewhere.
    localMeta: {
      codec: track.codec,
      sampleRate: track.sampleRate,
      channels: track.channels,
      bitrateBps: track.bitrateBps,
      trackNo: track.trackNo,
      discNo: track.discNo,
      year: track.year,
      relativeDir: track.relativeDir,
      favourite: track.favourite,
    },
  } as SongData);
};

/** Whether a song is an imported local file. */
export const isLocalSong = (song: SongData | null | undefined): boolean =>
  typeof song?.local?.uri === "string" && song.local.uri.length > 0;

/** The locator for a local song, or `null`. */
export const localKeyOf = (song: SongData | null | undefined): string | null =>
  isLocalSong(song) ? (song!.local!.uri as string) : null;

// ── Commands ─────────────────────────────────────────────────────

const guard = <T>(fallback: T): T => fallback;

export const localSourceList = async (): Promise<LocalSource[]> => {
  if (!isTauri()) return guard([]);
  return invoke<LocalSource[]>("local_source_list");
};

/**
 * Open the platform picker and index whatever the user chose.
 *
 * The picker choice is made in Rust, not here: desktop opens the native folder
 * dialog, Android opens `ACTION_OPEN_DOCUMENT_TREE` and takes a persistable
 * grant. Resolves to `null` when the user cancelled.
 */
export const localSourceAddDirectory = async (
  onProgress?: (progress: ScanProgress) => void,
): Promise<LocalScanSummary | null> => {
  if (!isTauri()) return guard(null);
  return invoke<LocalScanSummary | null>("local_source_add_directory", {
    progress: progressChannel(onProgress),
  });
};

/**
 * Pick individual audio files and index them.
 *
 * `playlistId` also appends what was picked to that local playlist, so "add
 * these songs to this playlist" stays one action instead of a pick followed by
 * finding the same files again in the library.
 */
export const localSourceAddFiles = async (
  onProgress?: (progress: ScanProgress) => void,
  playlistId?: string,
): Promise<LocalScanSummary | null> => {
  if (!isTauri()) return guard(null);
  return invoke<LocalScanSummary | null>("local_source_add_files", {
    progress: progressChannel(onProgress),
    playlistId,
  });
};

export const localSourceRescan = async (
  sourceId: string,
  onProgress?: (progress: ScanProgress) => void,
): Promise<LocalScanSummary | null> => {
  if (!isTauri()) return guard(null);
  return invoke<LocalScanSummary>("local_source_rescan", {
    sourceId,
    progress: progressChannel(onProgress),
  });
};

export const localSourceRemove = async (sourceId: string): Promise<number> => {
  if (!isTauri()) return guard(0);
  return invoke<number>("local_source_remove", { sourceId });
};

export const localScanCancel = async (): Promise<void> => {
  if (!isTauri()) return;
  await invoke("local_scan_cancel");
};

/**
 * The progress channel is a required argument on the Rust side.
 *
 * `Option<Channel<T>>` cannot be a Tauri command parameter — `Option` is
 * deserialized while `Channel` is a `CommandArg` — so the channel always exists
 * and simply drops its messages when no callback was given.
 */
const progressChannel = (onProgress?: (progress: ScanProgress) => void): Channel<ScanProgress> => {
  const channel = new Channel<ScanProgress>();
  channel.onmessage = (progress) => onProgress?.(progress);
  return channel;
};

export const localLibraryList = async (
  query: LocalLibraryQuery = {},
): Promise<LocalLibraryPage> => {
  if (!isTauri()) return guard({ tracks: [], total: 0 });
  return invoke<LocalLibraryPage>("local_library_list", { query });
};

export const localTrackGet = async (key: string): Promise<LocalTrack | null> => {
  if (!isTauri()) return guard(null);
  return invoke<LocalTrack | null>("local_track_get", { key });
};

/** Resolve rows for ids the frontend already holds — a restored queue, mostly. */
export const localTracksBySongIds = async (songIds: number[]): Promise<LocalTrack[]> => {
  if (!isTauri() || !songIds.length) return guard([]);
  return invoke<LocalTrack[]>("local_tracks_by_song_ids", { songIds });
};

export const localGroups = async (kind: "album" | "artist" | "folder"): Promise<LocalGroup[]> => {
  if (!isTauri()) return guard([]);
  return invoke<LocalGroup[]>("local_groups", { kind });
};

export const localFavouriteSet = async (key: string, favourite: boolean): Promise<boolean> => {
  if (!isTauri()) return guard(false);
  return invoke<boolean>("local_favourite_set", { key, favourite });
};

export const localFavouriteList = async (): Promise<string[]> => {
  if (!isTauri()) return guard([]);
  return invoke<string[]>("local_favourite_list");
};

export const localPlaylistList = async (): Promise<LocalPlaylist[]> => {
  if (!isTauri()) return guard([]);
  return invoke<LocalPlaylist[]>("local_playlist_list");
};

export const localPlaylistCreate = async (
  name: string,
  description?: string,
): Promise<LocalPlaylist> => invoke<LocalPlaylist>("local_playlist_create", { name, description });

export const localPlaylistUpdate = async (
  playlistId: string,
  name?: string,
  description?: string,
): Promise<boolean> => invoke<boolean>("local_playlist_update", { playlistId, name, description });

export const localPlaylistDelete = async (playlistId: string): Promise<boolean> =>
  invoke<boolean>("local_playlist_delete", { playlistId });

export const localPlaylistAddTracks = async (playlistId: string, keys: string[]): Promise<number> =>
  invoke<number>("local_playlist_add_tracks", { playlistId, keys });

export const localPlaylistRemoveTracks = async (
  playlistId: string,
  keys: string[],
): Promise<number> => invoke<number>("local_playlist_remove_tracks", { playlistId, keys });

export const localPlaylistReorder = async (
  playlistId: string,
  from: number,
  to: number,
): Promise<boolean> => invoke<boolean>("local_playlist_reorder", { playlistId, from, to });

/**
 * Import an M3U/M3U8 file as a local playlist.
 *
 * Without `path` the native open dialog runs in Rust. Resolves to `null` when the
 * user cancelled. Desktop only — a SAF document cannot see its own parent
 * directory, which is where a playlist's relative entries point.
 */
export const localPlaylistImportM3u = async (path?: string): Promise<M3uImportResult | null> =>
  invoke<M3uImportResult | null>("local_playlist_import_m3u", { path });

/**
 * Write a playlist out as M3U8.
 *
 * Without `path` the native save dialog runs in Rust — the frontend has no
 * dialog plugin, and adding one for this alone would also mean an ACL entry.
 * Resolves to `0` when the user cancelled.
 */
export const localPlaylistExportM3u = async (playlistId: string, path?: string): Promise<number> =>
  invoke<number>("local_playlist_export_m3u", { playlistId, path });

/**
 * Lyrics for a local track: an import the user made, then the embedded tag, then
 * a sibling `.lrc`/`.ttml`.
 *
 * `kind` is a *hint* about which family the file claimed, not a verdict: the
 * exact word-timed dialect (YRC / QRC / ESLrc) is still detected from the content
 * by `LyricsProcessor`, so a mislabelled import renders correctly anyway.
 *
 * Never Netease. `fetchAndParseLyric(id)` stays Netease-only — handing it a
 * negative id would be a request for a track that does not exist.
 */
export const localLyricFor = async (key: string): Promise<LocalLyric | null> => {
  if (!isTauri()) return guard(null);
  return invoke<LocalLyric | null>("local_lyric_for", { key });
};

/** Everything the song detail page shows, in one call. */
export const localTrackDetail = async (key: string): Promise<LocalTrackDetail | null> => {
  if (!isTauri()) return guard(null);
  return invoke<LocalTrackDetail | null>("local_track_detail", { key });
};

/**
 * Replace one track's tag corrections.
 *
 * The audio file is never written to — see `src-tauri/src/local/model.rs`. An
 * empty patch reverts to what the file says.
 */
export const localTrackOverrideSet = async (
  key: string,
  patch: TrackOverride,
): Promise<LocalTrack | null> => {
  if (!isTauri()) return guard(null);
  const row = await invoke<LocalTrack | null>("local_track_override_set", { key, patch });
  notifyLocalTracksChanged({ keys: [key], kind: "tags" });
  return row;
};

/**
 * Read the file's tags again, replacing the indexed row.
 *
 * Leaves any override in place: a correction the user typed outranks the file,
 * and silently dropping it here would make this button destructive.
 */
export const localTrackReprobe = async (key: string): Promise<LocalTrack | null> => {
  if (!isTauri()) return guard(null);
  const row = await invoke<LocalTrack | null>("local_track_reprobe", { key });
  notifyLocalTracksChanged({ keys: [key], kind: "tags" });
  return row;
};

/** Store lyric text the user pasted. `extension` is only a format hint. */
export const localLyricImportText = async (
  key: string,
  text: string,
  extension?: string,
  originalName?: string,
): Promise<LyricImport> => {
  const entry = await invoke<LyricImport>("local_lyric_import_text", {
    key,
    text,
    extension,
    originalName,
  });
  notifyLocalTracksChanged({ keys: [key], kind: "lyric" });
  return entry;
};

/**
 * Pick a lyric file and import it. Resolves to `null` when cancelled.
 *
 * The picker runs in Rust on both platforms: desktop opens the native dialog,
 * Android opens a SAF document picker that reads the bytes without taking a
 * persistable grant.
 */
export const localLyricImportFile = async (key: string): Promise<LyricImport | null> => {
  const entry = await invoke<LyricImport | null>("local_lyric_import_file", { key });
  // A cancelled picker changed nothing, so it must not wake the subscribers.
  if (entry) notifyLocalTracksChanged({ keys: [key], kind: "lyric" });
  return entry;
};

/** Drop an imported lyric and delete its file. */
export const localLyricClear = async (key: string): Promise<boolean> => {
  if (!isTauri()) return guard(false);
  const removed = await invoke<boolean>("local_lyric_clear", { key });
  if (removed) notifyLocalTracksChanged({ keys: [key], kind: "lyric" });
  return removed;
};

export interface CoverImportResult {
  entry: CoverImport;
  /** Every track that now points at this picture. */
  applied: string[];
}

/**
 * Give a track — or its whole album — a cover the user picked.
 *
 * The bytes go over as base64 because that is the one encoding an `<input
 * type="file">` can produce on every platform this runs on. Rust decodes, refuses
 * a non-image, and hands it to the same store the scan uses, so the result is
 * downscaled and content-addressed exactly like extracted art.
 *
 * Nothing is written to the audio file, matching `localTrackOverrideSet`.
 */
export const localCoverImport = async (
  key: string,
  dataBase64: string,
  mediaType?: string,
  originalName?: string,
  applyToAlbum = false,
): Promise<CoverImportResult> => {
  if (!isTauri()) throw new Error("local covers need the desktop or mobile app");
  const result = await invoke<CoverImportResult>("local_cover_import", {
    key,
    dataBase64,
    mediaType,
    originalName,
    applyToAlbum,
  });
  notifyLocalTracksChanged({ keys: result.applied, kind: "cover" });
  return result;
};

/**
 * Forget a picked cover, falling back to whatever the file itself carried.
 *
 * Resolves to the keys that were cleared. The picture is not deleted — it is
 * shared by content — so the cache prune collects it once nothing refers to it.
 */
export const localCoverClear = async (key: string, applyToAlbum = false): Promise<string[]> => {
  if (!isTauri()) return guard<string[]>([]);
  const cleared = await invoke<string[]>("local_cover_clear", { key, applyToAlbum });
  notifyLocalTracksChanged({ keys: cleared, kind: "cover" });
  return cleared;
};

/**
 * Wipe the local library.
 *
 * Only the system-reset dialog's explicit, unchecked-by-default option calls
 * this. Everything else about local data must survive an account change: local
 * favourites, sources and playlists live in `$APPDATA` and share no layer with
 * the Netease session, so login, logout and account switching cannot reach them.
 */
export const localLibraryReset = async (): Promise<void> => {
  if (!isTauri()) return;
  await invoke("local_library_reset");
};
