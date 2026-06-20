import { toRaw } from "vue";
import { musicStore, settingStore, siteStore } from "@/store";
import { getProcessedLyrics, type AMLLLine, type SongLyric } from "@/utils/LyricsProcessor";
import { windowManager, isTauri } from "./windowManager";
import {
  PLAYER_COMMUNICATION_EVENTS,
  PLAYER_CONTENT_WINDOW_LABELS,
  PLAYER_STATE_WINDOW_LABELS,
  type PlayerFullStatePayload,
  type PlayerLyricPayload,
  type PlayerSettingsPayload,
  type PlayerStatePayload,
  type PlayerTimePayload,
} from "./playerCommunicationTypes";

export {
  PLAYER_COMMUNICATION_EVENTS,
  PLAYER_CONTENT_WINDOW_LABELS,
  PLAYER_STATE_WINDOW_LABELS,
} from "./playerCommunicationTypes";

export type {
  PlayerContentWindowLabel,
  PlayerFullStatePayload,
  PlayerLyricPayload,
  PlayerSettingsPayload,
  PlayerStatePayload,
  PlayerStateWindowLabel,
  PlayerTimePayload,
} from "./playerCommunicationTypes";

export interface MainPlayerCommunicationOptions {
  seek: (time: number) => void;
}

type MusicStore = ReturnType<typeof musicStore>;
type SettingStore = ReturnType<typeof settingStore>;
type SiteStore = ReturnType<typeof siteStore>;

const TIME_BROADCAST_INTERVAL_MS = 50;
const noop = () => {};

let cachedMusic: MusicStore | null = null;
let cachedSetting: SettingStore | null = null;
let cachedSite: SiteStore | null = null;
let mainListenersStarted = false;
let lastTimeBroadcastAt = 0;
let cachedLyricSource: SongLyric | null = null;
let cachedLyricSongId: number | null = null;
let cachedLyricSettingsKey = "";
let cachedLyricPayload: PlayerLyricPayload | null = null;

function getMusic() {
  cachedMusic ??= musicStore();
  return cachedMusic;
}

function getSetting() {
  cachedSetting ??= settingStore();
  return cachedSetting;
}

function getSite() {
  cachedSite ??= siteStore();
  return cachedSite;
}

function getTauriEvent() {
  return window.__TAURI__?.event;
}

function emitToLabels(eventName: string, payload: unknown, labels: readonly string[]) {
  const tauriEvent = getTauriEvent();
  if (!tauriEvent) return;
  for (let i = 0; i < labels.length; i++) {
    tauriEvent.emitTo(labels[i], eventName, payload).catch(noop);
  }
}

export function emitToMain(eventName: string, payload?: unknown) {
  getTauriEvent()?.emitTo("main", eventName, payload).catch(noop);
}

function coverUrl(picUrl: string | undefined, size: number) {
  return picUrl ? `${picUrl.replace(/^http:/, "https:")}?param=${size}y${size}` : "";
}

function buildPlayerStatePayload(): PlayerStatePayload {
  const music = getMusic();
  const site = getSite();
  const songData = music.getPlaySongData;
  const playTime = music.getPlaySongTime;
  const artists = songData?.artist ?? [];

  return {
    title: songData?.name || "",
    artist: artists.map((artist) => artist.name).join(", "),
    artistList: artists.map((artist) => ({ id: artist.id, name: artist.name })),
    coverUrl: coverUrl(songData?.album?.picUrl, 128),
    coverUrlLarge: coverUrl(songData?.album?.picUrl, 512),
    songId: songData?.id ?? null,
    isPlaying: music.getPlayState,
    isLoading: music.isLoadingSong,
    isLiked: songData ? music.getSongIsLike(songData.id) : false,
    accentColor: site.songPicColor || "",
    currentTime: playTime?.currentTime || 0,
    duration: playTime?.duration || 0,
    volume: music.persistData.playVolume,
    playMode: music.persistData.playSongMode || "normal",
  };
}

function buildPlayerTimePayload(): PlayerTimePayload {
  const music = getMusic();
  return {
    currentTime: music.getPlaySongTime.currentTime,
    lyricIndex: music.playSongLyricIndex,
  };
}

function lyricSettingsKey(setting: SettingStore) {
  return String(
    (setting.showYrc ? 4 : 0) | (setting.showRoma ? 2 : 0) | (setting.showTransl ? 1 : 0),
  );
}

function buildLrcPayload(songLyric: SongLyric) {
  return Array.isArray(songLyric.lrc) ? songLyric.lrc : [];
}

function buildPlayerLyricPayload(force = false): PlayerLyricPayload | null {
  const music = getMusic();
  const setting = getSetting();
  const songData = music.getPlaySongData;
  if (!songData) return null;

  const songLyric = toRaw(music.songLyric) as SongLyric | null;
  if (!songLyric) return null;

  const settingsKey = lyricSettingsKey(setting);
  if (
    !force &&
    cachedLyricPayload &&
    cachedLyricSource === songLyric &&
    cachedLyricSongId === songData.id &&
    cachedLyricSettingsKey === settingsKey
  ) {
    return cachedLyricPayload;
  }

  let amllLines: AMLLLine[] = [];
  try {
    if (
      songLyric.processedLyrics &&
      songLyric.processedLyrics.length > 0 &&
      songLyric.settingsHash === settingsKey
    ) {
      amllLines = songLyric.processedLyrics as unknown as AMLLLine[];
    } else {
      amllLines = getProcessedLyrics(songLyric, {
        showYrc: setting.showYrc,
        showRoma: setting.showRoma,
        showTransl: setting.showTransl,
      });
    }
  } catch (err) {
    console.error("[PlayerCommunication] Failed to process lyrics for broadcast:", err);
  }

  cachedLyricSource = songLyric;
  cachedLyricSongId = songData.id;
  cachedLyricSettingsKey = settingsKey;
  cachedLyricPayload = {
    songId: songData.id,
    lrc: buildLrcPayload(songLyric),
    amllLines,
    hasYrc: songLyric.hasYrc || false,
    hasLrcTran: songLyric.hasLrcTran || false,
    hasLrcRoma: songLyric.hasLrcRoma || false,
  };

  return cachedLyricPayload;
}

function buildPlayerSettingsPayload(): PlayerSettingsPayload {
  const setting = getSetting();
  return {
    lyricTimeOffset: setting.lyricTimeOffset,
    lyricsFontSize: setting.lyricsFontSize,
    lyricFont: setting.lyricFont,
    lyricFontWeight: setting.lyricFontWeight,
    lyricLetterSpacing: setting.lyricLetterSpacing,
    lyricLineHeight: setting.lyricLineHeight,
    lyricsBlur: setting.lyricsBlur,
    lyricsBlock: setting.lyricsBlock,
    lyricsPosition: setting.lyricsPosition,
    showYrc: setting.showYrc,
    showYrcAnimation: setting.showYrcAnimation,
    showTransl: setting.showTransl,
    showRoma: setting.showRoma,
    springParams: setting.springParams,
  };
}

function syncTrayEffectColor() {
  const color = getSite().songPicColor;
  if (!color) return;

  const match = color.match(/(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/);
  if (!match) return;

  const isDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const base = isDark ? 30 : 240;
  const r = Math.round(base * 0.85 + Number.parseInt(match[1], 10) * 0.15);
  const g = Math.round(base * 0.85 + Number.parseInt(match[2], 10) * 0.15);
  const b = Math.round(base * 0.85 + Number.parseInt(match[3], 10) * 0.15);
  windowManager.setWindowEffectColor("tray-popup", r, g, b, 200).catch(noop);
}

export function broadcastPlayerState() {
  if (!isTauri()) return;
  emitToLabels(
    PLAYER_COMMUNICATION_EVENTS.state,
    buildPlayerStatePayload(),
    PLAYER_STATE_WINDOW_LABELS,
  );
  syncTrayEffectColor();
}

export function broadcastPlayerTime(force = false) {
  if (!isTauri()) return;
  const music = getMusic();
  if (!force && !music.getPlayState) return;

  const now = performance.now();
  if (!force && now - lastTimeBroadcastAt < TIME_BROADCAST_INTERVAL_MS) return;
  lastTimeBroadcastAt = now;

  emitToLabels(
    PLAYER_COMMUNICATION_EVENTS.time,
    buildPlayerTimePayload(),
    PLAYER_CONTENT_WINDOW_LABELS,
  );
}

export function broadcastPlayerLyrics(force = false) {
  if (!isTauri()) return;
  const payload = buildPlayerLyricPayload(force);
  if (!payload) return;
  emitToLabels(PLAYER_COMMUNICATION_EVENTS.lyric, payload, PLAYER_CONTENT_WINDOW_LABELS);
}

export function broadcastPlayerSettings() {
  if (!isTauri()) return;
  emitToLabels(
    PLAYER_COMMUNICATION_EVENTS.settings,
    buildPlayerSettingsPayload(),
    PLAYER_CONTENT_WINDOW_LABELS,
  );
}

export function broadcastPlayerFullState(targetLabel: string) {
  if (!isTauri() || !targetLabel) return;

  const payload: PlayerFullStatePayload = {
    state: buildPlayerStatePayload(),
    time: buildPlayerTimePayload(),
    lyric: buildPlayerLyricPayload(),
    settings: buildPlayerSettingsPayload(),
  };

  getTauriEvent()?.emitTo(targetLabel, PLAYER_COMMUNICATION_EVENTS.fullState, payload).catch(noop);
}

export async function setupMainPlayerCommunication(options: MainPlayerCommunicationOptions) {
  const tauri = window.__TAURI__;
  if (!tauri || mainListenersStarted) return;
  mainListenersStarted = true;

  const music = getMusic();
  const setting = getSetting();

  await tauri.event.listen("tray-play-pause", () => {
    music.setPlayState(!music.getPlayState);
  });

  await tauri.event.listen("tray-prev-track", () => {
    music.setPlaySongIndex("prev");
  });

  await tauri.event.listen("tray-next-track", () => {
    music.setPlaySongIndex("next");
  });

  await tauri.event.listen("tray-popup-opened", () => {
    broadcastPlayerState();
  });

  await tauri.event.listen("tray-cycle-play-mode", () => {
    music.setPlaySongMode();
    broadcastPlayerState();
  });

  await tauri.event.listen("tray-like-song", async () => {
    const songData = music.getPlaySongData;
    if (!songData) return;
    await music.changeLikeList(songData.id, !music.getSongIsLike(songData.id));
    broadcastPlayerState();
  });

  await tauri.event.listen(PLAYER_COMMUNICATION_EVENTS.slavePlayPause, () => {
    music.setPlayState(!music.getPlayState);
  });

  await tauri.event.listen(PLAYER_COMMUNICATION_EVENTS.slavePrevTrack, () => {
    music.setPlaySongIndex("prev");
  });

  await tauri.event.listen(PLAYER_COMMUNICATION_EVENTS.slaveNextTrack, () => {
    music.setPlaySongIndex("next");
  });

  await tauri.event.listen<{ time?: unknown }>(PLAYER_COMMUNICATION_EVENTS.slaveSeek, (event) => {
    const time = event.payload?.time;
    if (typeof time === "number") options.seek(time);
  });

  await tauri.event.listen<{ volume?: unknown }>(
    PLAYER_COMMUNICATION_EVENTS.slaveVolume,
    (event) => {
      const volume = event.payload?.volume;
      if (typeof volume === "number") {
        music.persistData.playVolume = Math.max(0, Math.min(1, volume));
      }
    },
  );

  await tauri.event.listen(PLAYER_COMMUNICATION_EVENTS.slaveCyclePlayMode, () => {
    music.setPlaySongMode();
    broadcastPlayerState();
  });

  await tauri.event.listen(PLAYER_COMMUNICATION_EVENTS.slaveLikeSong, async () => {
    const songData = music.getPlaySongData;
    if (!songData) return;
    await music.changeLikeList(songData.id, !music.getSongIsLike(songData.id));
    broadcastPlayerState();
  });

  await tauri.event.listen<{ size?: unknown }>(
    PLAYER_COMMUNICATION_EVENTS.slaveSetLyricsFontSize,
    (event) => {
      const size = event.payload?.size;
      if (typeof size !== "number") return;
      setting.lyricsFontSize = Math.max(2, Math.min(6, size));
      broadcastPlayerSettings();
    },
  );

  await tauri.event.listen<{ label?: unknown }>(PLAYER_COMMUNICATION_EVENTS.slaveReady, (event) => {
    const label = event.payload?.label;
    if (typeof label === "string") broadcastPlayerFullState(label);
  });
}
