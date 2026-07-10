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
  playSongTime: createDefaultPlaySongTime(),
  playbackSnapshot: createDefaultPlaybackSnapshot(),
  playVolume: 0.7,
  playVolumeMute: 0,
  playlistState: 0,
  playHistory: [],
});
