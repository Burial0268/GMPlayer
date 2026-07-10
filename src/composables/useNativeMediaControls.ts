import { onMounted, onUnmounted, ref, watch } from "vue";
import { debounce } from "throttle-debounce";
import { storeToRefs } from "pinia";
import { musicStore } from "@/store";
import { isMobile, isTauri } from "@/utils/tauri";
import { setSeek } from "@/utils/AudioContext";
import {
  hideMediaNotification,
  initializeMediaNotification,
  listenAudioFocusChange,
  listenMediaAction,
  updateMediaNotification,
  updateMediaPlaybackState,
  updateMediaProgress,
  type AudioFocusState,
  type MediaActionPayload,
} from "@/utils/tauri/mediaNotification";
import {
  clearNowPlayingControls,
  initializeNowPlayingControls,
  listenNowPlayingAction,
  updateNowPlayingPlayMode,
  updateNowPlayingState,
  updateNowPlayingTimeline,
  type NowPlayingActionPayload,
} from "@/utils/tauri/nowPlayingControls";

type PlaybackState = "playing" | "paused" | "buffering";
type PlayMode = "normal" | "random" | "single";
type NativeMediaAction = MediaActionPayload | NowPlayingActionPayload;

interface NativeMediaPayload {
  title: string;
  artist: string;
  album: string;
  isPlaying: boolean;
  position: number;
  duration: number;
  artworkUrl: string;
  trackId?: number;
}

interface NativeMediaAdapter {
  name: "media-session" | "now-playing-controls";
  artworkSize: number;
  initialize: () => Promise<void | undefined>;
  updateFull: (payload: NativeMediaPayload) => Promise<void | undefined>;
  updateProgress: (payload: {
    isPlaying: boolean;
    position: number;
    duration: number;
    seeked?: boolean;
  }) => Promise<void | undefined>;
  updatePlaybackState: (payload: {
    state: PlaybackState;
    isPlaying: boolean;
    position: number;
  }) => Promise<void | undefined>;
  updatePlayMode: (mode: PlayMode) => Promise<void | undefined>;
  clear: () => Promise<void | undefined>;
  listenAction: (handler: (payload: NativeMediaAction) => void) => Promise<() => void>;
  listenAudioFocus?: (handler: (state: AudioFocusState) => void) => Promise<() => void>;
}

const mobileMediaSessionAdapter: NativeMediaAdapter = {
  name: "media-session",
  artworkSize: 256,
  initialize: initializeMediaNotification,
  updateFull: (payload) => updateMediaNotification(payload),
  updateProgress: (payload) =>
    updateMediaProgress({
      isPlaying: payload.isPlaying,
      position: payload.position,
    }),
  updatePlaybackState: (payload) =>
    updateMediaPlaybackState({
      state: payload.state,
      position: payload.position,
    }),
  updatePlayMode: async () => undefined,
  clear: hideMediaNotification,
  listenAction: listenMediaAction,
  listenAudioFocus: listenAudioFocusChange,
};

const desktopNowPlayingAdapter: NativeMediaAdapter = {
  name: "now-playing-controls",
  artworkSize: 512,
  initialize: initializeNowPlayingControls,
  updateFull: (payload) => updateNowPlayingState(payload),
  updateProgress: async (payload) => {
    await updateNowPlayingTimeline({
      position: payload.position,
      duration: payload.duration,
      seeked: payload.seeked,
    });
    await updateNowPlayingState({
      isPlaying: payload.isPlaying,
      playbackState: payload.isPlaying ? "playing" : "paused",
    });
  },
  updatePlaybackState: (payload) =>
    updateNowPlayingState({
      playbackState: payload.state,
      isPlaying: payload.isPlaying,
      position: payload.position,
    }),
  updatePlayMode: (mode) => updateNowPlayingPlayMode({ mode }),
  clear: clearNowPlayingControls,
  listenAction: listenNowPlayingAction,
};

let instanceCount = 0;

export function useNativeMediaControls() {
  const music = musicStore();
  const { persistData } = storeToRefs(music);
  const active = ref(false);
  const adapterName = ref<NativeMediaAdapter["name"] | null>(null);

  let adapter: NativeMediaAdapter | null = null;
  let unlistenMediaAction: (() => void) | null = null;
  let unlistenAudioFocus: (() => void) | null = null;
  let lastPayloadHash = "";
  let lastProgressSyncAt = 0;

  const PROGRESS_SYNC_INTERVAL = 5_000;

  function buildFullPayload(): NativeMediaPayload | null {
    const song = music.getPlaySongData;
    if (!song || !adapter) return null;

    const artworkUrl = song.album?.picUrl
      ? `${song.album.picUrl.replace(/^http:/, "https:")}?param=${adapter.artworkSize}y${
          adapter.artworkSize
        }`
      : "";
    const playSongTime = music.getPlaySongTime;

    return {
      title: song.name || "",
      artist: Array.isArray(song.artist)
        ? song.artist.map((artist: { name: string }) => artist.name).join(", ")
        : "",
      album: song.album?.name || "",
      isPlaying: music.getPlayState,
      position: Math.round((playSongTime?.currentTime || 0) * 1_000),
      duration: Math.round((playSongTime?.duration || 0) * 1_000),
      artworkUrl,
      trackId: typeof song.id === "number" ? song.id : undefined,
    };
  }

  async function syncNotificationImmediate(): Promise<void> {
    if (!active.value || !adapter) return;
    syncNotification.cancel?.();

    const payload = buildFullPayload();
    if (!payload) {
      await adapter.clear();
      return;
    }

    lastPayloadHash = JSON.stringify(payload);
    await adapter.updateFull(payload);
    await syncPlayMode();
  }

  const syncNotification = debounce(300, async () => {
    if (!active.value || !adapter) return;

    const payload = buildFullPayload();
    if (!payload) {
      await adapter.clear();
      return;
    }

    const hash = JSON.stringify(payload);
    if (hash === lastPayloadHash) return;
    lastPayloadHash = hash;
    await adapter.updateFull(payload);
    await syncPlayMode();
  });

  async function syncProgress(seeked = false): Promise<void> {
    if (!active.value || !adapter || !music.getPlaySongData) return;

    const playSongTime = music.getPlaySongTime;
    await adapter.updateProgress({
      isPlaying: music.getPlayState,
      position: Math.round((playSongTime?.currentTime || 0) * 1_000),
      duration: Math.round((playSongTime?.duration || 0) * 1_000),
      seeked,
    });
    lastProgressSyncAt = Date.now();
  }

  async function syncPlaybackState(): Promise<void> {
    if (!active.value || !adapter || !music.getPlaySongData) return;

    const state: PlaybackState = music.isLoadingSong
      ? "buffering"
      : music.getPlayState
        ? "playing"
        : "paused";
    await adapter.updatePlaybackState({
      state,
      isPlaying: !music.isLoadingSong && music.getPlayState,
      position: Math.round((music.getPlaySongTime?.currentTime || 0) * 1_000),
    });
  }

  async function syncPlayMode(): Promise<void> {
    if (!active.value || !adapter) return;
    await adapter.updatePlayMode(persistData.value.playSongMode || "normal");
  }

  function maybeSyncProgress(): void {
    if (!active.value) return;
    const now = Date.now();
    if (now - lastProgressSyncAt < PROGRESS_SYNC_INTERVAL) return;
    void syncProgress();
  }

  function handleMediaAction(payload: NativeMediaAction): void {
    switch (payload.action) {
      case "play":
        music.setPlayState(true);
        break;
      case "pause":
        music.setPlayState(false);
        break;
      case "next":
        music.setPlaySongIndex("next");
        break;
      case "previous":
        music.setPlaySongIndex("prev");
        break;
      case "stop":
        music.setPlayState(false);
        void adapter?.clear();
        break;
      case "seek":
        if (typeof payload.position === "number") {
          const seekSec = payload.position / 1_000;
          if (window.$player) {
            setSeek(window.$player, seekSec);
          }
          music.setPlaySongTime({
            currentTime: seekSec,
            duration: music.getPlaySongTime?.duration || 0,
          });
          void syncProgress(true);
        }
        break;
      case "toggleShuffle":
        music.setPlaySongMode(persistData.value.playSongMode === "random" ? "normal" : "random");
        break;
      case "toggleRepeat":
        music.setPlaySongMode(persistData.value.playSongMode === "single" ? "normal" : "single");
        break;
      case "setVolume":
        if (typeof payload.volume === "number") {
          persistData.value.playVolume = Math.max(0, Math.min(1, payload.volume));
        }
        break;
      case "setRate":
        break;
      default:
        console.warn("[NativeMediaControls] Unknown media action:", payload);
    }
  }

  function handleAudioFocusChange(state: AudioFocusState): void {
    switch (state) {
      case "gain":
        if (music.getPlayState && window.$player && !window.$player.playing()) {
          music.setPlayState(true);
        }
        break;
      case "loss":
      case "loss_transient":
        if (music.getPlayState) {
          music.setPlayState(false);
        }
        break;
      case "loss_transient_can_duck":
        if (music.getPlayState && window.$player) {
          if (!window._originalVolumeBeforeDuck) {
            window._originalVolumeBeforeDuck = music.persistData.playVolume;
          }
          music.persistData.playVolume = Math.max(0.1, window._originalVolumeBeforeDuck * 0.2);
        }
        break;
      default:
        break;
    }
  }

  function onVisibilityChange(): void {
    if (!active.value || document.visibilityState !== "visible") return;
    void syncNotificationImmediate();
  }

  onMounted(async () => {
    if (instanceCount > 0) return;
    instanceCount++;

    if (!isTauri()) return;

    adapter = (await isMobile()) ? mobileMediaSessionAdapter : desktopNowPlayingAdapter;
    adapterName.value = adapter.name;
    active.value = true;

    await adapter.initialize();
    unlistenMediaAction = await adapter.listenAction(handleMediaAction);
    if (adapter.listenAudioFocus) {
      unlistenAudioFocus = await adapter.listenAudioFocus(handleAudioFocusChange);
    }
    document.addEventListener("visibilitychange", onVisibilityChange);
    void syncNotificationImmediate();
  });

  onUnmounted(() => {
    instanceCount = Math.max(0, instanceCount - 1);
    unlistenMediaAction?.();
    unlistenMediaAction = null;
    unlistenAudioFocus?.();
    unlistenAudioFocus = null;
    document.removeEventListener("visibilitychange", onVisibilityChange);
    if (active.value) {
      void adapter?.clear();
    }
    active.value = false;
    adapterName.value = null;
    adapter = null;
  });

  watch(
    () => music.getPlaySongData,
    (val, oldVal) => {
      if (!active.value) return;
      if (val?.id !== oldVal?.id) {
        lastPayloadHash = "";
        lastProgressSyncAt = 0;
        void syncNotificationImmediate();
      } else {
        void syncNotification();
      }

      if (!val) {
        void adapter?.clear();
      }
    },
    { deep: true },
  );

  watch(
    () => music.getPlaySongTime?.duration,
    (val, oldVal) => {
      if (!active.value) return;
      if (val && !oldVal) void syncNotificationImmediate();
    },
  );

  watch(
    () => music.getPlayState,
    () => {
      if (!active.value) return;
      void syncProgress();
    },
  );

  watch(
    () => music.isLoadingSong,
    (isLoading) => {
      if (!active.value) return;
      void syncPlaybackState();
      if (!isLoading) {
        void syncProgress();
      }
    },
  );

  watch(
    () => music.getPlaySongTime?.currentTime,
    () => {
      if (!active.value) return;
      maybeSyncProgress();
    },
  );

  watch(
    () => persistData.value.playSongMode,
    () => {
      void syncPlayMode();
    },
  );

  return {
    active,
    adapterName,
    syncNotification,
    syncNotificationImmediate,
    syncProgress,
    syncPlaybackState,
    syncPlayMode,
  };
}
