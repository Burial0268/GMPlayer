export interface Artist {
  id: number;
  name: string;
  [key: string]: any;
}

export interface Album {
  id: number;
  name: string;
  picUrl: string;
  [key: string]: any;
}

/**
 * Marks a song as an imported local file, and is the **only** thing that should
 * be branched on to tell one apart.
 *
 * Not `id < 0`. Local ids are negative for a reason — Netease ids are always
 * positive, so a path that forgets to branch degrades to "not plannable"
 * (`identityForSongId` returns `null` for `id <= 0`) instead of asking Netease
 * about track `-123` — but that is a *safety net*, not an interface. Business
 * logic keys on this field.
 */
export interface LocalTrackRef {
  /** Source locator: an absolute path on desktop, `content://…` on Android. */
  uri: string;
  /** The importing source, so a whole folder can be invalidated at once. */
  sourceId: string;
  /**
   * Absolute path to the extracted cover, or `undefined` when the file embedded
   * none. Deliberately the raw path rather than an `asset://` URL: the OS media
   * session fetches this natively and cannot resolve the webview's host. The
   * webview's own copy is `album.picUrl`.
   */
  coverPath?: string;
  /** Filename of the cover under the cover cache, for cheap comparisons. */
  coverKey?: string;
  /** Whether the source has to be spooled to a file before it can be decoded. */
  needsCache?: boolean;
}

export interface SongData {
  id: number;
  name: string;
  artist: Artist[];
  album: Album;
  alia?: string[];
  time: string;
  fee: number;
  pc?: any;
  mv?: number;
  /** Present only for imported local files. See [`LocalTrackRef`]. */
  local?: LocalTrackRef;
  [key: string]: any;
}

export interface PlaySongTime {
  currentTime: number;
  playbackCurrentTime?: number;
  duration: number;
  barMoveDistance: number;
  songTimePlayed: string;
  songTimeDuration: string;
}

export interface PlaybackSessionSnapshot {
  version: 1;
  revision: number;
  songId: number | null;
  playSongIndex: number;
  playSongTime: PlaySongTime;
  updatedAt: number;
}

export interface PersistData {
  searchHistory: string[];
  personalFmMode: boolean;
  personalFmData: SongData | Record<string, never>;
  playListMode: string;
  likeList: number[];
  playlists: SongData[];
  playSongIndex: number;
  playSongMode: "normal" | "random" | "single";
  /**
   * Song ids of `playlists` as they were ordered *before* the shuffle, or empty
   * when the queue is not shuffled.
   *
   * Random mode reorders `playlists` itself instead of drawing a new index at
   * every track change, so the permutation is persisted with the queue and
   * survives a WebView the OS killed. This is the other half of that: the way
   * back out. Ids, not positions — positions are exactly what a list edit
   * invalidates.
   */
  preShuffleOrder: number[];
  playSongTime: PlaySongTime;
  playbackSnapshot: PlaybackSessionSnapshot;
  playVolume: number;
  playVolumeMute: number;
  playlistState: number;
  playHistory: SongData[];
}

export const createDefaultPlaySongTime = (): PlaySongTime => ({
  currentTime: 0,
  playbackCurrentTime: 0,
  duration: 0,
  barMoveDistance: 0,
  songTimePlayed: "00:00",
  songTimeDuration: "00:00",
});

export const createDefaultPlaybackSnapshot = (): PlaybackSessionSnapshot => ({
  version: 1,
  revision: 0,
  songId: null,
  playSongIndex: 0,
  playSongTime: createDefaultPlaySongTime(),
  updatedAt: 0,
});

export const createDefaultPersistData = (): PersistData => ({
  searchHistory: [],
  personalFmMode: false,
  personalFmData: {},
  playListMode: "list",
  likeList: [],
  playlists: [],
  playSongIndex: 0,
  playSongMode: "normal",
  preShuffleOrder: [],
  playSongTime: createDefaultPlaySongTime(),
  playbackSnapshot: createDefaultPlaybackSnapshot(),
  playVolume: 0.7,
  playVolumeMute: 0,
  playlistState: 0,
  playHistory: [],
});
