import type { AMLLLine } from "@/utils/LyricsProcessor";

export const PLAYER_CONTENT_WINDOW_LABELS = [
  "mini-player",
  "desktop-lyrics",
  "taskbar-lyric",
] as const;

export const PLAYER_STATE_WINDOW_LABELS = [...PLAYER_CONTENT_WINDOW_LABELS, "tray-popup"] as const;

export const PLAYER_COMMUNICATION_EVENTS = {
  state: "player-state-update",
  time: "player-time-update",
  lyric: "player-lyric-update",
  settings: "player-settings-update",
  fullState: "player-full-state",
  slaveReady: "slave-window-opened",
  slavePlayPause: "slave-play-pause",
  slavePrevTrack: "slave-prev-track",
  slaveNextTrack: "slave-next-track",
  slaveSeek: "slave-seek",
  slaveVolume: "slave-volume",
  slaveCyclePlayMode: "slave-cycle-play-mode",
  slaveLikeSong: "slave-like-song",
  slaveSetLyricsFontSize: "slave-set-lyrics-font-size",
  slaveSetDesktopLyricsFontSizeOffset: "slave-set-desktop-lyrics-font-size-offset",
} as const;

export type PlayerContentWindowLabel = (typeof PLAYER_CONTENT_WINDOW_LABELS)[number];
export type PlayerStateWindowLabel = (typeof PLAYER_STATE_WINDOW_LABELS)[number];

export interface PlayerStatePayload {
  title: string;
  artist: string;
  artistList: { id: number; name: string }[];
  coverUrl: string;
  coverUrlLarge: string;
  songId: number | null;
  isPlaying: boolean;
  isLoading: boolean;
  isLiked: boolean;
  accentColor: string;
  currentTime: number;
  duration: number;
  volume: number;
  playMode: "normal" | "random" | "single";
}

export interface PlayerTimePayload {
  currentTime: number;
  lyricIndex: number;
  duration?: number;
  songId?: number | null;
  isPlaying?: boolean;
  seq?: number;
  sentAt?: number;
}

export interface PlayerLyricPayload {
  songId: number;
  lrc: { time: number; content: string }[];
  amllLines: AMLLLine[];
  hasYrc: boolean;
  hasLrcTran: boolean;
  hasLrcRoma: boolean;
}

export interface PlayerSettingsPayload {
  lyricTimeOffset: number;
  lyricsFontSize: number;
  desktopLyricsFontSizeOffset: number;
  lyricFont: string;
  lyricFontWeight: string;
  lyricLetterSpacing: string;
  lyricLineHeight: number;
  lyricsBlur: boolean;
  lyricsBlock: string;
  lyricsPosition: string;
  showYrc: boolean;
  showYrcAnimation: boolean;
  showTransl: boolean;
  showRoma: boolean;
  springParams: {
    posX: { mass: number; damping: number; stiffness: number };
    posY: { mass: number; damping: number; stiffness: number };
    scale: { mass: number; damping: number; stiffness: number };
  };
}

export interface PlayerFullStatePayload {
  state: PlayerStatePayload;
  time: PlayerTimePayload;
  lyric: PlayerLyricPayload | null;
  settings: PlayerSettingsPayload;
}
