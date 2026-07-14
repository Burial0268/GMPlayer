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
      duration: payload.duration,
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

  /**
   * Live playback clock in milliseconds. Prefers the active sound's timeline
   * (anchor + extrapolation — accurate even right after a backend-initiated
   * track advance or a background wake-up) over the store snapshot, which is
   * only refreshed by the RAF/interval loop and can be seconds stale.
   */
  function getLivePlaybackMs(): { position: number; duration: number } {
    const playSongTime = music.getPlaySongTime;
    let positionSec = playSongTime?.currentTime || 0;
    let durationSec = playSongTime?.duration || 0;

    const player = window.$player;
    if (player) {
      try {
        const livePosition = player.seek();
        if (typeof livePosition === "number" && Number.isFinite(livePosition)) {
          positionSec = livePosition;
        }
        const liveDuration = player.duration();
        if (Number.isFinite(liveDuration) && liveDuration > 0) {
          durationSec = liveDuration;
        }
      } catch {
        /* destroyed/mid-swap sound — store snapshot fallback is fine */
      }
    }

    return {
      position: Math.round(Math.max(0, positionSec) * 1_000),
      duration: Math.round(Math.max(0, durationSec) * 1_000),
    };
  }

  function buildFullPayload(): NativeMediaPayload | null {
    const song = music.getPlaySongData;
    if (!song || !adapter) return null;

    const artworkUrl = song.album?.picUrl
      ? `${song.album.picUrl.replace(/^http:/, "https:")}?param=${adapter.artworkSize}y${
          adapter.artworkSize
        }`
      : "";
    const live = getLivePlaybackMs();

    return {
      title: song.name || "",
      artist: Array.isArray(song.artist)
        ? song.artist.map((artist: { name: string }) => artist.name).join(", ")
        : "",
      album: song.album?.name || "",
      isPlaying: music.getPlayState,
      position: live.position,
      duration: live.duration,
      artworkUrl,
      trackId: typeof song.id === "number" ? song.id : undefined,
    };
  }

  /** Metadata-identity hash — position/isPlaying deliberately excluded so the
   * debounced dedup only skips pushes when nothing user-visible changed
   * (position rides along on every push anyway). */
  function payloadMetaHash(payload: NativeMediaPayload): string {
    return JSON.stringify([
      payload.title,
      payload.artist,
      payload.album,
      payload.artworkUrl,
      payload.trackId,
      payload.duration,
    ]);
  }

  async function syncNotificationImmediate(): Promise<void> {
    if (!active.value || !adapter) return;
    syncNotification.cancel?.();

    const payload = buildFullPayload();
    if (!payload) {
      await adapter.clear();
      return;
    }

    lastPayloadHash = payloadMetaHash(payload);
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

    const hash = payloadMetaHash(payload);
    if (hash === lastPayloadHash) return;
    lastPayloadHash = hash;
    await adapter.updateFull(payload);
    await syncPlayMode();
  });

  async function syncProgress(seeked = false): Promise<void> {
    if (!active.value || !adapter || !music.getPlaySongData) return;

    const live = getLivePlaybackMs();
    await adapter.updateProgress({
      isPlaying: music.getPlayState,
      position: live.position,
      duration: live.duration,
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
      position: getLivePlaybackMs().position,
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
          const durationSec = getLivePlaybackMs().duration / 1_000;
          let seekSec = Math.max(0, payload.position / 1_000);
          if (durationSec > 0) seekSec = Math.min(seekSec, durationSec);
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

  // Audio-focus bookkeeping. Android expects apps to resume after a
  // *transient* loss (call / navigation prompt) but stay paused after a
  // permanent loss, and to restore the pre-duck volume on focus gain.
  let pausedByTransientFocusLoss = false;
  let volumeBeforeDuck: number | null = null;

  function restoreDuckedVolume(): void {
    if (volumeBeforeDuck === null) return;
    const duckedTarget = Math.max(0.1, volumeBeforeDuck * 0.2);
    // Only restore when the volume is still at the ducked level — a manual
    // change while ducked is user intent and must win.
    if (Math.abs(music.persistData.playVolume - duckedTarget) < 0.01) {
      music.persistData.playVolume = volumeBeforeDuck;
    }
    volumeBeforeDuck = null;
  }

  function handleAudioFocusChange(state: AudioFocusState): void {
    switch (state) {
      case "gain":
        restoreDuckedVolume();
        if (pausedByTransientFocusLoss) {
          pausedByTransientFocusLoss = false;
          if (!music.getPlayState) {
            music.setPlayState(true);
          }
        }
        break;
      case "loss":
        // Permanent loss: pause and stay paused (no auto-resume on gain).
        restoreDuckedVolume();
        pausedByTransientFocusLoss = false;
        if (music.getPlayState) {
          music.setPlayState(false);
        }
        break;
      case "loss_transient":
        if (music.getPlayState) {
          pausedByTransientFocusLoss = true;
          music.setPlayState(false);
        }
        break;
      case "loss_transient_can_duck":
        if (music.getPlayState && volumeBeforeDuck === null) {
          volumeBeforeDuck = music.persistData.playVolume;
          music.persistData.playVolume = Math.max(0.1, volumeBeforeDuck * 0.2);
        }
        break;
      default:
        break;
    }
  }

  function onVisibilityChange(): void {
    if (!active.value) return;
    if (document.visibilityState === "visible") {
      // Wake-up: the backend queue window may have advanced tracks while this
      // JS runtime was frozen — force a full metadata push (bypass the hash
      // guard) and hard re-anchor the seekbar position.
      lastPayloadHash = "";
      void syncNotificationImmediate();
      void syncProgress(true);
    } else {
      // Entering background: leave the freshest possible position anchor —
      // the system extrapolates from it while our timers are frozen.
      void syncProgress(true);
    }
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
      // Any duration change matters: 0 → X when the source loads, and X → Y
      // corrections (backend-advanced track adopted, more accurate decode).
      if (val && val !== oldVal) void syncNotificationImmediate();
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
