import { reactive, ref, shallowRef, onMounted, onUnmounted } from "vue";
import type { AMLLLine } from "@/utils/LyricsProcessor";

// ── Payload Types ──────────────────────────────────────────────────────────

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
  lyricFont: "HarmonyOS Sans SC",
  lyricFontWeight: "normal",
  lyricLetterSpacing: "normal",
  lyricLineHeight: 1.8,
  lyricsBlur: true,
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

// ── Helper ─────────────────────────────────────────────────────────────────

function getTauri() {
  return window.__TAURI__;
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

  // ── Receive events from master ──────────────────────────────────────

  async function connect(): Promise<void> {
    const tauri = getTauri();
    if (!tauri) return;

    // Player state (song metadata, playback state, etc.)
    const u1 = await tauri.event.listen<PlayerStatePayload>("player-state-update", (e) => {
      Object.assign(state, e.payload);
    });
    unlisteners.push(u1);

    // Time updates (~20fps)
    const u2 = await tauri.event.listen<PlayerTimePayload>("player-time-update", (e) => {
      currentTime.value = e.payload.currentTime;
      lyricIndex.value = e.payload.lyricIndex;
    });
    unlisteners.push(u2);

    // Lyric data (once per song)
    const u3 = await tauri.event.listen<PlayerLyricPayload>("player-lyric-update", (e) => {
      lyricData.value = e.payload;
    });
    unlisteners.push(u3);

    // Settings changes
    const u4 = await tauri.event.listen<PlayerSettingsPayload>("player-settings-update", (e) => {
      Object.assign(settings, e.payload);
    });
    unlisteners.push(u4);

    // Full state snapshot (response to slave-window-opened)
    const u5 = await tauri.event.listen<{
      state: PlayerStatePayload;
      time: PlayerTimePayload;
      lyric: PlayerLyricPayload | null;
      settings: PlayerSettingsPayload;
    }>("player-full-state", (e) => {
      Object.assign(state, e.payload.state);
      currentTime.value = e.payload.time.currentTime;
      lyricIndex.value = e.payload.time.lyricIndex;
      if (e.payload.lyric) {
        lyricData.value = e.payload.lyric;
      }
      Object.assign(settings, e.payload.settings);
    });
    unlisteners.push(u5);

    // Notify master that we're ready
    const windowLabel = window.location.pathname.includes("mini-player")
      ? "mini-player"
      : window.location.pathname.includes("desktop-lyrics")
        ? "desktop-lyrics"
        : "unknown";

    await tauri.event.emit("slave-window-opened", { label: windowLabel });
  }

  function disconnect(): void {
    unlisteners.forEach((fn) => fn());
    unlisteners.length = 0;
  }

  // ── Send commands to master ─────────────────────────────────────────

  function playPause(): void {
    getTauri()?.event.emit("slave-play-pause", null);
  }

  function prevTrack(): void {
    getTauri()?.event.emit("slave-prev-track", null);
  }

  function nextTrack(): void {
    getTauri()?.event.emit("slave-next-track", null);
  }

  function seek(time: number): void {
    getTauri()?.event.emit("slave-seek", { time });
  }

  function setVolume(volume: number): void {
    getTauri()?.event.emit("slave-volume", { volume });
  }

  function cyclePlayMode(): void {
    getTauri()?.event.emit("slave-cycle-play-mode", null);
  }

  // ── Auto-connect lifecycle ──────────────────────────────────────────

  onMounted(() => {
    connect();
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

    // Lifecycle
    connect,
    disconnect,
  };
}
