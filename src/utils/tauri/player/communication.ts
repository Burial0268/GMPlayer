import { toRaw } from "vue";
import { musicStore, settingStore, siteStore } from "@/store";
import { getProcessedLyrics, type AMLLLine, type SongLyric } from "@/utils/LyricsProcessor";
// Aliased: this module has its own `coverUrl` wrapper that adds the empty-string
// contract the slave windows expect.
import { coverUrl as toCoverUrl } from "@/utils/coverUrl";
import { isTauri } from "../core/runtime";
import { windowManager } from "../window/manager";
import {
  PLAYER_COMMUNICATION_EVENTS,
  PLAYER_CONTENT_WINDOW_LABELS,
  PLAYER_STATE_WINDOW_LABELS,
  type PlayerFullStatePayload,
  type PlayerLyricPayload,
  type PlayerSettingsPayload,
  type PlayerStatePayload,
  type PlayerTimePayload,
} from "./types";

export {
  PLAYER_COMMUNICATION_EVENTS,
  PLAYER_CONTENT_WINDOW_LABELS,
  PLAYER_STATE_WINDOW_LABELS,
} from "./types";

export type {
  PlayerContentWindowLabel,
  PlayerFullStatePayload,
  PlayerLyricPayload,
  PlayerSettingsPayload,
  PlayerStatePayload,
  PlayerStateWindowLabel,
  PlayerTimePayload,
} from "./types";

export interface MainPlayerCommunicationOptions {
  seek: (time: number) => void;
}

type MusicStore = ReturnType<typeof musicStore>;
type SettingStore = ReturnType<typeof settingStore>;
type SiteStore = ReturnType<typeof siteStore>;

// Anchor-based time broadcasting: slaves extrapolate locally from the last
// anchor (see playerBridge.ts), so the master only needs to send when the
// timeline becomes discontinuous (play/pause/seek/track/lyric-line change)
// plus a low-rate heartbeat for drift correction — instead of a 20fps stream.
const TIME_HEARTBEAT_MS = 2_000;
/** Master-side drift between the real clock and the last sent anchor beyond
 * which a correction anchor is sent (catches seeks — including while paused). */
const TIME_DRIFT_THRESHOLD_S = 0.25;
/** Burst guard for unforced sends. */
const TIME_MIN_GAP_MS = 30;
// How often to reconcile the open-content-window set against the real window
// list. Pure safety net for a dropped event: opens arrive through the
// `slaveReady` handshake, and both ways a window can leave the set — hide and
// destroy — are pushed by Rust as `managed-window-visibility`. Only runs while
// at least one content window is believed open, and each pass costs one
// `get_window_state` invoke per content label, so it stays slow on purpose.
const CONTENT_WINDOW_RECONCILE_MS = 15_000;
const noop = () => {};

interface TimeBroadcastAnchor {
  time: number;
  at: number;
  isPlaying: boolean;
  songId: number | null;
  duration: number;
  lyricIndex: number;
}

let cachedMusic: MusicStore | null = null;
let cachedSetting: SettingStore | null = null;
let cachedSite: SiteStore | null = null;
let mainListenersStarted = false;
let lastTimeAnchor: TimeBroadcastAnchor | null = null;
let timeBroadcastSeq = 0;
let cachedLyricSource: SongLyric | null = null;
let cachedLyricSongId: number | null = null;
let cachedLyricSettingsKey = "";
let cachedLyricPayload: PlayerLyricPayload | null = null;

// Which content windows (mini-player / desktop-lyrics / taskbar-lyric) are
// currently open AND visible. The time broadcast is skipped entirely when this
// is empty, and only fans out to the labels that are actually open — so the
// common case (no lyric/mini windows) costs nothing, and one open window costs
// one `emitTo` instead of three. Hidden-but-alive windows count as closed: Rust
// announces show/hide/destroy via `managed-window-visibility`, and a re-shown
// window is healed with a full-state push.
const openContentWindows = new Set<string>();
let openContentLabelsCache: string[] | null = null;
let contentWindowReconcileTimer: ReturnType<typeof setInterval> | null = null;

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

// ── Open content-window tracking (gates the 20fps time broadcast) ────────────

function isContentWindowLabel(label: string): boolean {
  return (PLAYER_CONTENT_WINDOW_LABELS as readonly string[]).includes(label);
}

/** Labels of content windows believed open, in canonical order. Cached — the
 * 30Hz time-broadcast path calls this per tick, so recomputing the filter
 * would allocate ~100k throwaway arrays per hour. */
function openContentBroadcastLabels(): string[] {
  openContentLabelsCache ??= PLAYER_CONTENT_WINDOW_LABELS.filter((label) =>
    openContentWindows.has(label),
  );
  return openContentLabelsCache;
}

function addContentWindow(label: string): boolean {
  if (openContentWindows.has(label)) return false;
  openContentWindows.add(label);
  openContentLabelsCache = null;
  return true;
}

function removeContentWindow(label: string): void {
  if (openContentWindows.delete(label)) {
    openContentLabelsCache = null;
  }
}

/**
 * Mark a content window open. Called from the `slaveReady` handshake, which the
 * slave re-sends with retries — so opens are never missed. Closes are handled
 * by the visibility events plus the reconcile loop, so this only ever *adds*.
 */
function markContentWindowOpen(label: string): void {
  if (!isContentWindowLabel(label)) return;
  addContentWindow(label);
  ensureContentWindowReconcile();
}

function ensureContentWindowReconcile(): void {
  if (contentWindowReconcileTimer !== null) return;
  contentWindowReconcileTimer = setInterval(() => {
    void reconcileContentWindows();
  }, CONTENT_WINDOW_RECONCILE_MS);
}

function stopContentWindowReconcileIfIdle(): void {
  if (openContentWindows.size === 0 && contentWindowReconcileTimer !== null) {
    clearInterval(contentWindowReconcileTimer);
    contentWindowReconcileTimer = null;
  }
}

/**
 * Reconcile the open-window belief against real window state. A window counts
 * as open only while it exists AND is visible — a hidden-but-alive window
 * (e.g. mini-player toggled away) must not keep the heartbeat/broadcast
 * machinery running for the rest of the session. Never *under-reports* on a
 * failed query: keep the current belief so a live slave is never starved.
 * When nothing is open, the reconcile loop stops (opens restart it via
 * `markContentWindowOpen` or the visibility event).
 */
async function reconcileContentWindows(): Promise<void> {
  const states = await Promise.all(
    PLAYER_CONTENT_WINDOW_LABELS.map(async (label) => ({
      label,
      state: await windowManager.getWindowState(label).catch(() => null),
    })),
  );
  for (const { label, state } of states) {
    if (!state) continue;
    if (state.exists && state.visible) {
      // Discovered outside the handshake (master reload while slaves stayed
      // open, missed visibility event) — heal it with a full-state push.
      if (addContentWindow(label)) {
        broadcastPlayerFullState(label);
      }
    } else {
      removeContentWindow(label);
    }
  }
  stopContentWindowReconcileIfIdle();
  if (openContentWindows.size > 0) {
    // Windows discovered outside the handshake must still be pruned when they
    // close later.
    ensureContentWindowReconcile();
  }
}

/**
 * Cover URL for a slave window's payload.
 *
 * Returns `""` rather than the default image when there is no cover: the slave
 * windows treat an empty string as "draw no artwork", and a placeholder would
 * paint a grey square over their own fallback.
 *
 * A local track's cover is an asset-protocol URL, which every window can load —
 * each webview registers its own handler — but which must not be rewritten,
 * hence the shared helper.
 */
function coverUrl(picUrl: string | undefined, size: number) {
  return picUrl ? toCoverUrl(picUrl, size) : "";
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
  const playTime = music.getPlaySongTime;
  timeBroadcastSeq = (timeBroadcastSeq + 1) >>> 0 || 1;
  return {
    currentTime: playTime.currentTime,
    lyricIndex: music.playSongLyricIndex,
    duration: playTime.duration,
    songId: music.getPlaySongData?.id ?? null,
    isPlaying: music.getPlayState,
    seq: timeBroadcastSeq,
    sentAt: Date.now(),
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

function clearLyricBroadcastCache(): void {
  cachedLyricSource = null;
  cachedLyricSongId = null;
  cachedLyricSettingsKey = "";
  cachedLyricPayload = null;
}

function buildPlayerLyricPayload(force = false): PlayerLyricPayload | null {
  const music = getMusic();
  const setting = getSetting();
  const songData = music.getPlaySongData;
  if (!songData) {
    // Queue cleared — drop the module-level refs so the previous track's
    // lyric payload doesn't stay pinned after the store has reset.
    clearLyricBroadcastCache();
    return null;
  }

  const songLyric = toRaw(music.songLyric) as SongLyric | null;
  if (!songLyric) {
    clearLyricBroadcastCache();
    return null;
  }

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
    // getProcessedLyrics owns the processedLyrics cache and validates it against
    // its own hash, so there is no useful shortcut to take here.
    amllLines = getProcessedLyrics(songLyric, {
      showYrc: setting.showYrc,
      showRoma: setting.showRoma,
      showTransl: setting.showTransl,
    });
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
    desktopLyricsFontSizeOffset: setting.desktopLyricsFontSizeOffset,
    lyricFont: setting.lyricFont,
    lyricFontWeight: setting.lyricFontWeight,
    lyricLetterSpacing: setting.lyricLetterSpacing,
    lyricLineHeight: setting.lyricLineHeight,
    lyricsBlur: setting.lyricsBlur,
    hidePassedLines: setting.hidePassedLines,
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
  const base = 240;
  const match = color?.match(/(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/);
  const r = match ? Math.round(base * 0.85 + Number.parseInt(match[1], 10) * 0.15) : base;
  const g = match ? Math.round(base * 0.85 + Number.parseInt(match[2], 10) * 0.15) : base;
  const b = match ? Math.round(base * 0.85 + Number.parseInt(match[3], 10) * 0.15) : base;
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

  // Nothing to update when no content window is open — skip the payload build
  // and the per-window emit entirely (the common case).
  const labels = openContentBroadcastLabels();
  if (labels.length === 0) return;

  const music = getMusic();
  const playTime = music.getPlaySongTime;
  const currentTime = playTime?.currentTime || 0;
  const duration = playTime?.duration || 0;
  const isPlaying = music.getPlayState;
  const songId = music.getPlaySongData?.id ?? null;
  const lyricIndex = music.playSongLyricIndex;
  const now = Date.now();

  if (!force && lastTimeAnchor) {
    if (now - lastTimeAnchor.at < TIME_MIN_GAP_MS) return;

    // Only send when the timeline breaks from what slaves already extrapolate:
    // state flips, track/duration/lyric-line changes, a seek (drift — works
    // while paused too, fixing stale slaves after pause+drag), or heartbeat.
    const expected = lastTimeAnchor.isPlaying
      ? lastTimeAnchor.time + (now - lastTimeAnchor.at) / 1_000
      : lastTimeAnchor.time;
    const discontinuous =
      isPlaying !== lastTimeAnchor.isPlaying ||
      songId !== lastTimeAnchor.songId ||
      duration !== lastTimeAnchor.duration ||
      lyricIndex !== lastTimeAnchor.lyricIndex ||
      Math.abs(currentTime - expected) > TIME_DRIFT_THRESHOLD_S;
    if (!discontinuous && now - lastTimeAnchor.at < TIME_HEARTBEAT_MS) return;
  }

  lastTimeAnchor = { time: currentTime, at: now, isPlaying, songId, duration, lyricIndex };
  emitToLabels(PLAYER_COMMUNICATION_EVENTS.time, buildPlayerTimePayload(), labels);
}

export function broadcastPlayerLyrics(force = false) {
  if (!isTauri()) return;
  // Only visible content windows need the payload — anything (re)opening later
  // is healed by the full-state push, so skip the (expensive) payload build
  // and the per-window IPC serialization entirely when none is visible.
  const labels = openContentBroadcastLabels();
  if (labels.length === 0) {
    // Nothing will rebuild the payload while every content window is closed, so
    // holding it would pin the last broadcast track's processed lines (a full
    // word-level graph) for the rest of the session. Drop it; reopening a window
    // triggers a forced rebuild anyway.
    clearLyricBroadcastCache();
    return;
  }
  const payload = buildPlayerLyricPayload(force);
  if (!payload) return;
  emitToLabels(PLAYER_COMMUNICATION_EVENTS.lyric, payload, labels);
}

export function broadcastPlayerSettings() {
  if (!isTauri()) return;
  const labels = openContentBroadcastLabels();
  if (labels.length === 0) return;
  emitToLabels(PLAYER_COMMUNICATION_EVENTS.settings, buildPlayerSettingsPayload(), labels);
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

  await tauri.event.listen<{ offset?: unknown }>(
    PLAYER_COMMUNICATION_EVENTS.slaveSetDesktopLyricsFontSizeOffset,
    (event) => {
      const offset = event.payload?.offset;
      if (typeof offset !== "number" || !Number.isFinite(offset)) return;
      setting.desktopLyricsFontSizeOffset = Math.max(-20, Math.min(40, offset));
    },
  );

  await tauri.event.listen<{ label?: unknown }>(PLAYER_COMMUNICATION_EVENTS.slaveReady, (event) => {
    const label = event.payload?.label;
    if (typeof label === "string") {
      markContentWindowOpen(label);
      broadcastPlayerFullState(label);
    }
  });

  // Rust announces show/hide of managed windows. Hidden slaves drop out of the
  // broadcast set (stopping the heartbeat/reconcile machinery when nothing is
  // visible); re-shown slaves rejoin and are healed with a full-state push —
  // they don't re-run the slaveReady handshake because the page never reloads.
  await tauri.event.listen<{ label?: unknown; visible?: unknown }>(
    "managed-window-visibility",
    (event) => {
      const label = event.payload?.label;
      const visible = event.payload?.visible;
      if (typeof label !== "string" || typeof visible !== "boolean") return;
      if (!isContentWindowLabel(label)) return;
      if (visible) {
        const added = addContentWindow(label);
        ensureContentWindowReconcile();
        if (added) broadcastPlayerFullState(label);
      } else {
        removeContentWindow(label);
        stopContentWindowReconcileIfIdle();
      }
    },
  );

  // Master (re)started while slave windows were already open (reload, crash
  // recovery): they never re-handshake, so discover them and re-push the full
  // state — reconcileContentWindows heals newly discovered windows inline.
  void reconcileContentWindows();
}
