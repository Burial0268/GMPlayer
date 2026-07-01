import { reactive, ref, shallowRef, onMounted, onUnmounted } from "vue";
import {
  PLAYER_COMMUNICATION_EVENTS,
  type PlayerFullStatePayload,
  type PlayerLyricPayload,
  type PlayerSettingsPayload,
  type PlayerStatePayload,
  type PlayerTimePayload,
} from "./playerCommunicationTypes";

// ── Payload Types ──────────────────────────────────────────────────────────

export type {
  PlayerFullStatePayload,
  PlayerLyricPayload,
  PlayerSettingsPayload,
  PlayerStatePayload,
  PlayerTimePayload,
} from "./playerCommunicationTypes";

// ── Default Values ─────────────────────────────────────────────────────────

const defaultState: PlayerStatePayload = {
  title: "",
  artist: "",
  artistList: [],
  coverUrl: "",
  coverUrlLarge: "",
  songId: null,
  isPlaying: false,
  isLoading: false,
  isLiked: false,
  accentColor: "",
  currentTime: 0,
  duration: 0,
  volume: 0.7,
  playMode: "normal",
};

const defaultSettings: PlayerSettingsPayload = {
  lyricTimeOffset: 0,
  lyricsFontSize: 3.6,
  desktopLyricsFontSizeOffset: 0,
  lyricFont: "HarmonyOS Sans SC",
  lyricFontWeight: "normal",
  lyricLetterSpacing: "normal",
  lyricLineHeight: 1.8,
  lyricsBlur: true,
  hidePassedLines: false,
  lyricsBlock: "top",
  lyricsPosition: "left",
  showYrc: true,
  showYrcAnimation: true,
  showTransl: false,
  showRoma: false,
  springParams: {
    posX: { mass: 1, damping: 10, stiffness: 100 },
    posY: { mass: 1, damping: 15, stiffness: 100 },
    scale: { mass: 1, damping: 20, stiffness: 100 },
  },
};

const READY_RETRY_DELAYS = [150, 600, 1200] as const;
const noop = () => {};

// ── Helper ─────────────────────────────────────────────────────────────────

function getTauri() {
  return window.__TAURI__;
}

function emitToMain(eventName: string, payload?: unknown) {
  getTauri()?.event.emitTo("main", eventName, payload).catch(noop);
}

// ── Composable ─────────────────────────────────────────────────────────────

/**
 * Slave-side composable for Mini Player and Desktop Lyrics windows.
 * Receives state from the master (main window) via Tauri events
 * and sends commands back.
 */
export function usePlayerBridge() {
  const state = reactive<PlayerStatePayload>({ ...defaultState });
  const lyricData = shallowRef<PlayerLyricPayload | null>(null);
  const settings = reactive<PlayerSettingsPayload>({ ...defaultSettings });
  const currentTime = ref(0);
  const lyricIndex = ref(-1);

  const unlisteners: (() => void)[] = [];
  let lastAcceptedTimeSeq = 0;
  let lastAcceptedTimeSongId: number | null = null;

  function isStaleTimePayload(payload: PlayerTimePayload): boolean {
    const seq = payload.seq ?? 0;
    return seq > 0 && lastAcceptedTimeSeq > 0 && seq <= lastAcceptedTimeSeq;
  }

  function applyStatePayload(payload: PlayerStatePayload): void {
    const acceptedTime = currentTime.value;
    Object.assign(state, payload);
    if (
      lastAcceptedTimeSeq > 0 &&
      (payload.songId === null ||
        lastAcceptedTimeSongId === null ||
        payload.songId === lastAcceptedTimeSongId)
    ) {
      state.currentTime = acceptedTime;
    }
  }

  function applyTimePayload(payload: PlayerTimePayload): void {
    if (isStaleTimePayload(payload)) return;

    const time = Number.isFinite(payload.currentTime) ? Math.max(0, payload.currentTime) : 0;
    currentTime.value = time;
    lyricIndex.value = payload.lyricIndex;
    state.currentTime = time;

    if (typeof payload.duration === "number" && Number.isFinite(payload.duration)) {
      state.duration = Math.max(0, payload.duration);
    }
    if (typeof payload.isPlaying === "boolean") {
      state.isPlaying = payload.isPlaying;
    }

    const seq = payload.seq ?? 0;
    if (seq > 0) lastAcceptedTimeSeq = seq;
    lastAcceptedTimeSongId = payload.songId ?? state.songId ?? lastAcceptedTimeSongId;
  }

  // ── Receive events from master ──────────────────────────────────────

  async function connect(): Promise<void> {
    const tauri = getTauri();
    if (!tauri || unlisteners.length > 0) return;

    // Player state (song metadata, playback state, etc.)
    const u1 = await tauri.event.listen<PlayerStatePayload>(
      PLAYER_COMMUNICATION_EVENTS.state,
      (e) => {
        applyStatePayload(e.payload);
      },
    );
    unlisteners.push(u1);

    // Time updates (~20fps)
    const u2 = await tauri.event.listen<PlayerTimePayload>(
      PLAYER_COMMUNICATION_EVENTS.time,
      (e) => {
        applyTimePayload(e.payload);
      },
    );
    unlisteners.push(u2);

    // Lyric data (once per song)
    const u3 = await tauri.event.listen<PlayerLyricPayload>(
      PLAYER_COMMUNICATION_EVENTS.lyric,
      (e) => {
        lyricData.value = e.payload;
      },
    );
    unlisteners.push(u3);

    // Settings changes
    const u4 = await tauri.event.listen<PlayerSettingsPayload>(
      PLAYER_COMMUNICATION_EVENTS.settings,
      (e) => {
        Object.assign(settings, e.payload);
      },
    );
    unlisteners.push(u4);

    // Full state snapshot (response to slave-window-opened)
    const u5 = await tauri.event.listen<PlayerFullStatePayload>(
      PLAYER_COMMUNICATION_EVENTS.fullState,
      (e) => {
        if (isStaleTimePayload(e.payload.time)) return;
        applyStatePayload(e.payload.state);
        applyTimePayload(e.payload.time);
        if (e.payload.lyric) {
          lyricData.value = e.payload.lyric;
        }
        Object.assign(settings, e.payload.settings);
      },
    );
    unlisteners.push(u5);

    // Notify master that we're ready
    const routePath = window.location.hash || window.location.pathname;
    const windowLabel = routePath.includes("mini-player")
      ? "mini-player"
      : routePath.includes("desktop-lyrics")
        ? "desktop-lyrics"
        : routePath.includes("taskbar-lyric")
          ? "taskbar-lyric"
          : "unknown";

    const notifyMaster = () => {
      emitToMain(PLAYER_COMMUNICATION_EVENTS.slaveReady, { label: windowLabel });
    };

    notifyMaster();
    const retryTimers: number[] = [];
    for (const delay of READY_RETRY_DELAYS) {
      retryTimers.push(window.setTimeout(notifyMaster, delay));
    }
    unlisteners.push(() => retryTimers.forEach((timer) => window.clearTimeout(timer)));
  }

  function disconnect(): void {
    unlisteners.forEach((fn) => fn());
    unlisteners.length = 0;
  }

  // ── Send commands to master ─────────────────────────────────────────
  // Use emitTo to target the main window explicitly, since event.emit
  // in Tauri v2 only broadcasts to the current window by default.

  function playPause(): void {
    emitToMain(PLAYER_COMMUNICATION_EVENTS.slavePlayPause, null);
  }

  function prevTrack(): void {
    emitToMain(PLAYER_COMMUNICATION_EVENTS.slavePrevTrack, null);
  }

  function nextTrack(): void {
    emitToMain(PLAYER_COMMUNICATION_EVENTS.slaveNextTrack, null);
  }

  function seek(time: number): void {
    emitToMain(PLAYER_COMMUNICATION_EVENTS.slaveSeek, { time });
  }

  function setVolume(volume: number): void {
    emitToMain(PLAYER_COMMUNICATION_EVENTS.slaveVolume, { volume });
  }

  function cyclePlayMode(): void {
    emitToMain(PLAYER_COMMUNICATION_EVENTS.slaveCyclePlayMode, null);
  }

  function toggleLike(): void {
    emitToMain(PLAYER_COMMUNICATION_EVENTS.slaveLikeSong, null);
  }

  function setLyricsFontSize(size: number): void {
    emitToMain(PLAYER_COMMUNICATION_EVENTS.slaveSetLyricsFontSize, { size });
  }

  function setDesktopLyricsFontSizeOffset(offset: number): void {
    const nextOffset = Math.max(-20, Math.min(40, offset));
    settings.desktopLyricsFontSizeOffset = nextOffset;
    emitToMain(PLAYER_COMMUNICATION_EVENTS.slaveSetDesktopLyricsFontSizeOffset, {
      offset: nextOffset,
    });
  }

  // ── Auto-connect lifecycle ──────────────────────────────────────────

  onMounted(() => {
    connect().catch(noop);
  });

  onUnmounted(() => {
    disconnect();
  });

  return {
    // Reactive state
    state,
    lyricData,
    settings,
    currentTime,
    lyricIndex,

    // Commands
    playPause,
    prevTrack,
    nextTrack,
    seek,
    setVolume,
    cyclePlayMode,
    toggleLike,
    setLyricsFontSize,
    setDesktopLyricsFontSizeOffset,

    // Lifecycle
    connect,
    disconnect,
  };
}
