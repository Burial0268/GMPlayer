/**
 * PlayerFunctions - Exported player functions with store integration
 *
 * Key improvements:
 * - Visibility-aware spectrum updates (pause in background)
 * - Reduced debug logging in production
 * - Improved error handling
 */

import { h } from "vue";
import { songScrobbleV2, submitSongPlayState } from "@/api/song";
// Import stores directly to avoid circular dependency through barrel exports
// (musicData.ts imports from @/utils/AudioContext, which re-exports this file)
import useMusicDataStore from "@/store/musicData";
import useSettingDataStore from "@/store/settingData";
import useUserDataStore from "@/store/userData";

const musicStore = () => useMusicDataStore();
const settingStore = () => useSettingDataStore();
const userStore = () => useUserDataStore();
import { NIcon } from "naive-ui";
import { MusicNoteFilled } from "@vicons/material";
import getLanguageData from "@/utils/getLanguageData";
import { applyGlobalCoverPalette } from "@/utils/color/coverPalette";
import { BufferedSound } from "./BufferedSound";
import { SoundManager } from "./SoundManager";
import { AudioContextManager } from "./AudioContextManager";
import { getAutoMixEngine } from "./AutoMix";
import { getAudioPreloader } from "./AudioPreloader";
import { getNativeQueueRegistryEntry, prefillNativeQueue } from "./NativeQueuePrefill";
import { clearSpectrumFrame, setSpectrumFrame } from "./SpectrumFrame";
import type { ISound } from "./types";
import {
  NativeRustSound,
  isAudioBackendRuntimeAvailable,
  isNativeAudioBackendAvailable,
} from "../tauri/NativeRustSound";
import { getAudioBackendTransport } from "../tauri/audioIpc";
import {
  buildNeteaseDesktopCookie,
  buildNeteaseDesktopUserAgent,
  createNeteasePlaybackSessionId,
} from "../neteaseClient";

const IS_DEV = import.meta.env?.DEV ?? false;
const NATIVE_AUTOMIX_COMPLETE_EVENT = "gmplayer:native-automix-complete";
const NATIVE_AUTOMIX_SYNC_EVENT = "gmplayer:native-automix-sync";
/** How long a backend-initiated transition (native advance / AutoMix adoption)
 * shields the adopted song from being torn down and re-created by the debounced
 * `getPlaySongData` watcher. Covers the 500ms debounce plus async settling. */
const NATIVE_ADVANCE_HOLD_MS = 4000;

// Native-advance hold: identifies the song adopted from a backend-initiated
// transition so the Player watcher reuses the already-playing sound.
let nativeAdvanceHold: { songId: number; until: number } | null = null;

const beginNativeAdvanceHold = (songId: number | undefined | null): void => {
  if (!Number.isFinite(songId as number)) return;
  nativeAdvanceHold = { songId: Number(songId), until: Date.now() + NATIVE_ADVANCE_HOLD_MS };
};

/**
 * True when `songId` was just adopted from a backend-initiated transition:
 * the active native sound is already playing it, so URL resolution and
 * `createSound` must be skipped (lyrics/UI sync only).
 */
export const isNativeAdvanceHoldActiveFor = (songId: number | undefined | null): boolean => {
  if (!nativeAdvanceHold) return false;
  if (Date.now() > nativeAdvanceHold.until) {
    nativeAdvanceHold = null;
    return false;
  }
  return Number(songId) === nativeAdvanceHold.songId;
};

// 歌曲信息更新定时器
let timeupdateInterval: number | null = null;
let timeupdateGeneration = 0;
let playbackPresentationTimer: ReturnType<typeof setTimeout> | null = null;
let pendingPlaybackPresentation: {
  sound: ISound;
  music: ReturnType<typeof musicStore>;
  currentTime: number;
  duration: number;
  generation: number;
} | null = null;
// 听歌打卡延时器
let scrobbleTimeout: ReturnType<typeof setTimeout> | null = null;
let pendingScrobbleKey: string | null = null;
let lastScrobbleKey: string | null = null;
let lastScrobbleAt = 0;
let playStateSessionKey: string | null = null;
let playStateSessionId: string | null = null;
// 重试次数
let testNumber = 0;
// 频谱更新动画帧 ID
let spectrumAnimationId: number | null = null;
let spectrumUpdateGeneration = 0;
// 页面可见性状态
let isPageVisible = true;
let lastLowFreqStoreValue = 0;
let lastLowFreqStoreAt = 0;
// Background monitor interval ID (setInterval fallback when RAF is paused)
let backgroundMonitorId: ReturnType<typeof setInterval> | null = null;
let lastBackendSyncRequestAt = 0;
let lastBackendStateAuditAt = 0;
let lastPlaybackPresentationAt = 0;

const LOW_FREQ_STORE_INTERVAL_MS = 33;
const PLAYBACK_PRESENTATION_INTERVAL_MS = 1000 / 30;
const LOW_FREQ_STORE_EPSILON = 0.005;
const SCROBBLE_DELAY_MS = 3000;
const SCROBBLE_DUPLICATE_GUARD_MS = 10000;

const getUsableDuration = (duration: number, fallback = 0): number => {
  if (Number.isFinite(duration) && duration > 0) return duration;
  return Number.isFinite(fallback) && fallback > 0 ? fallback : 0;
};

const normalizePlaybackPosition = (position: number, duration = 0, fallback = 0): number => {
  const nextPosition = Number.isFinite(position) ? Math.max(0, position) : Math.max(0, fallback);
  return duration > 0 ? Math.min(nextPosition, duration) : nextPosition;
};

const resetPlaySongTime = (music: ReturnType<typeof musicStore>): void => {
  music.resetPlaySongTime();
};

const checkpointActivePosition = (sound: ISound, music: ReturnType<typeof musicStore>): void => {
  const duration = getUsableDuration(sound.duration(), music.getPlaySongTime.duration);
  const position = normalizePlaybackPosition(
    sound.seek(),
    duration,
    music.getPlaySongPlaybackCurrentTime(),
  );
  music.setPlaySongTime({ currentTime: position, duration, displayCurrentTime: position });
  music.checkpointPlaySongTime(true);
};

const getPlayStateSessionId = (key: string): string => {
  if (playStateSessionKey !== key || !playStateSessionId) {
    playStateSessionKey = key;
    playStateSessionId = createNeteasePlaybackSessionId();
  }
  return playStateSessionId;
};

const getRelayPlayMode = (mode: ReturnType<typeof musicStore>["persistData"]["playSongMode"]) => {
  if (mode === "random") return "shuffle";
  if (mode === "single") return "single_loop";
  return "list_loop";
};

const updateLowFreqStore = (music: ReturnType<typeof musicStore>, value: number): void => {
  const nextValue = Number.isFinite(value) ? value : 0;
  const now = performance.now();
  if (
    now - lastLowFreqStoreAt < LOW_FREQ_STORE_INTERVAL_MS &&
    Math.abs(nextValue - lastLowFreqStoreValue) < LOW_FREQ_STORE_EPSILON
  ) {
    return;
  }
  lastLowFreqStoreAt = now;
  lastLowFreqStoreValue = nextValue;
  music.lowFreqVolume = nextValue;
};

const requestNativeBackendSync = (): void => {
  const sound = SoundManager.getCurrentSound();
  if (!isNativeRustSoundLike(sound)) return;

  const now = Date.now();
  if (now - lastBackendSyncRequestAt < 250) return;
  lastBackendSyncRequestAt = now;
  getAudioBackendTransport().sendOrQueue({ type: "syncStatus" });
};

const isNativeRustSoundLike = (sound: ISound | undefined | null): sound is NativeRustSound => {
  return !!sound && typeof (sound as NativeRustSound).requestStatusSync === "function";
};

const getActiveNativeSound = (): NativeRustSound | null => {
  const currentSound = SoundManager.getCurrentSound();
  if (isNativeRustSoundLike(currentSound)) return currentSound;
  const windowPlayer = window.$player as ISound | undefined;
  if (isNativeRustSoundLike(windowPlayer)) return windowPlayer;
  return null;
};

const isDestroyedNativeSound = (sound: ISound | undefined | null): boolean => {
  return sound instanceof NativeRustSound && sound.isDestroyed();
};

const resolveSeekTarget = (sound: ISound | undefined): ISound | undefined => {
  const currentSound = SoundManager.getCurrentSound() ?? undefined;
  if (currentSound && !isDestroyedNativeSound(currentSound)) return currentSound;

  const windowPlayer = window.$player as ISound | undefined;
  if (windowPlayer && !isDestroyedNativeSound(windowPlayer)) return windowPlayer;

  if (sound && !isDestroyedNativeSound(sound)) return sound;
  return undefined;
};

const syncNativeAnalysisState = (
  sound: ISound | undefined | null,
  settings = settingStore(),
): void => {
  if (!(sound instanceof NativeRustSound)) return;
  const analysisEnabled = isPageVisible && (settings.musicFrequency || settings.dynamicFlowSpeed);
  sound.setAnalysisEnabled(analysisEnabled);
  sound.setFFTEnabled(analysisEnabled && settings.musicFrequency);
};

const disableActiveNativeAnalysis = (): void => {
  const sound = getActiveNativeSound();
  if (!sound) return;
  sound.setFFTEnabled(false);
  sound.setAnalysisEnabled(false);
};

const scheduleScrobble = (reason: string): void => {
  const music = musicStore();
  const user = userStore();
  const songId = Number(music.getPlaySongData?.id);
  const sourceId = Number(music.getPlaySongData?.sourceId || 0);

  if (!user.userLogin || !Number.isFinite(songId) || songId <= 0) return;

  const scrobbleKey = `${music.persistData.playSongIndex}:${songId}:${sourceId}`;
  const now = Date.now();
  if (pendingScrobbleKey === scrobbleKey) return;
  if (lastScrobbleKey === scrobbleKey && now - lastScrobbleAt < SCROBBLE_DUPLICATE_GUARD_MS) {
    return;
  }

  if (scrobbleTimeout) clearTimeout(scrobbleTimeout);
  pendingScrobbleKey = scrobbleKey;
  scrobbleTimeout = setTimeout(() => {
    pendingScrobbleKey = null;
    // 仅当当前曲目仍是计划打卡的曲目时才上报
    const liveData = music.getPlaySongData;
    if (Number(liveData?.id) !== songId) return;
    // 歌曲(总)时长由后端歌曲数据提供，作为播放时长 time 与总时长 total 上报
    const totalSec = Math.round(Number(music.getPlaySongTime?.duration) || 0);
    const progressSec = Math.max(0, Math.round(music.getPlaySongPlaybackCurrentTime()));
    const artist = Array.isArray(liveData?.artist)
      ? liveData.artist
          .map((a: { name?: string }) => a?.name)
          .filter(Boolean)
          .join("/")
      : undefined;
    const sessionId = getPlayStateSessionId(scrobbleKey);
    const desktopCookie = buildNeteaseDesktopCookie(user.cookie);

    submitSongPlayState(songId, {
      sessionId,
      progress: progressSec,
      playMode: getRelayPlayMode(music.persistData.playSongMode),
      type: "song",
      cookie: desktopCookie,
      ua: buildNeteaseDesktopUserAgent(),
    }).catch((err) => {
      console.error("播放状态提交失败：" + err);
    });

    songScrobbleV2(songId, totalSec, {
      sourceid: sourceId || undefined,
      source: "list",
      name: liveData?.name,
      artist,
      total: totalSec || undefined,
    })
      .then((res) => {
        lastScrobbleKey = scrobbleKey;
        lastScrobbleAt = Date.now();
        if (IS_DEV) console.log(`歌曲打卡完成 (${reason})`, res);
      })
      .catch((err) => {
        console.error("歌曲打卡失败：" + err);
      });
  }, SCROBBLE_DELAY_MS);
};

const applyNativeAutoMixCompletion = (
  currentIndex: number,
  playback?: { position?: number; duration?: number; musicId?: string },
): void => {
  const music = musicStore();
  const sound = getActiveNativeSound();
  if (!sound) return;

  const playlists = music.persistData.playlists;
  if (!Number.isInteger(currentIndex) || currentIndex < 0) {
    return;
  }

  const autoMixIndex = getAutoMixEngine().resolveActiveTransitionTargetIndex(currentIndex);
  let resolvedIndex = autoMixIndex >= 0 ? autoMixIndex : currentIndex;

  // Cross-check the backend-reported track identity against the prefill
  // registry: if the playlist was edited while the backend advanced, the
  // index may point at a different song — identity wins over index.
  const registryEntry = getNativeQueueRegistryEntry(playback?.musicId);
  if (registryEntry && playlists[resolvedIndex]?.id !== registryEntry.songId) {
    const indexBySongId = playlists.findIndex((song) => song.id === registryEntry.songId);
    if (indexBySongId >= 0) resolvedIndex = indexBySongId;
  }

  if (resolvedIndex < 0 || resolvedIndex >= playlists.length) {
    return;
  }

  // Shield the adopted song from the debounced watcher's teardown/recreate.
  beginNativeAdvanceHold(playlists[resolvedIndex]?.id);

  const changed = music.persistData.playSongIndex !== resolvedIndex;
  const position = playback?.position;
  const duration = playback?.duration;
  const hasPlaybackPosition =
    typeof position === "number" && position >= 0 && typeof duration === "number" && duration > 0;
  if (changed) {
    music.commitPlaySongIndex(
      resolvedIndex,
      hasPlaybackPosition ? { currentTime: position, duration } : undefined,
    );
    music.resetSongLyricState();
  } else if (hasPlaybackPosition) {
    music.setPlaySongTime({ currentTime: position, duration });
  }

  void syncNativeAutoMixCurrentSound(sound);
  scheduleScrobble("native-automix");
};

/**
 * Start a setInterval(1000) fallback for when the page is hidden.
 * RAF is paused in background tabs, but setInterval still fires (~1/sec).
 * This keeps AutoMix state machine and time tracking alive.
 */
const startBackgroundMonitor = (): void => {
  if (backgroundMonitorId !== null) return;
  backgroundMonitorId = setInterval(() => {
    const sound = SoundManager.getCurrentSound();
    if (!sound) {
      stopBackgroundMonitor();
      return;
    }
    const autoMix = getAutoMixEngine();
    autoMix.monitorPlayback(sound);
    checkAudioTime(sound, musicStore());
  }, 1000);
};

/**
 * Stop the background monitor interval.
 */
const stopBackgroundMonitor = (): void => {
  if (backgroundMonitorId !== null) {
    clearInterval(backgroundMonitorId);
    backgroundMonitorId = null;
  }
};

const stopTimeUpdate = (): void => {
  timeupdateGeneration++;
  pendingPlaybackPresentation = null;
  if (playbackPresentationTimer !== null) {
    clearTimeout(playbackPresentationTimer);
    playbackPresentationTimer = null;
  }
  if (timeupdateInterval !== null) {
    cancelAnimationFrame(timeupdateInterval);
    timeupdateInterval = null;
  }
};

const scheduleAudioTimePresentation = (
  sound: ISound,
  music: ReturnType<typeof musicStore>,
  generation: number,
): void => {
  if (!sound.playing()) return;
  if (isNativeRustSoundLike(sound)) {
    const now = Date.now();
    if (now - lastBackendStateAuditAt >= 2000) {
      lastBackendStateAuditAt = now;
      requestNativeBackendSync();
    }
  }
  pendingPlaybackPresentation = {
    sound,
    music,
    currentTime: sound.seek() as number,
    duration: sound.duration(),
    generation,
  };
  if (playbackPresentationTimer !== null) return;

  playbackPresentationTimer = setTimeout(() => {
    playbackPresentationTimer = null;
    const pending = pendingPlaybackPresentation;
    pendingPlaybackPresentation = null;
    if (
      !pending ||
      pending.generation !== timeupdateGeneration ||
      SoundManager.getCurrentSound() !== pending.sound ||
      (window.$player && window.$player !== pending.sound)
    ) {
      return;
    }
    pending.music.setPlaySongTime({
      currentTime: pending.currentTime,
      duration: pending.duration,
    });
  }, 0);
};

const startTimeUpdate = (sound: ISound, music: ReturnType<typeof musicStore>): void => {
  stopTimeUpdate();
  const generation = timeupdateGeneration;
  lastPlaybackPresentationAt = 0;

  const timeLoop = (frameTime = performance.now()): void => {
    if (
      generation !== timeupdateGeneration ||
      SoundManager.getCurrentSound() !== sound ||
      (window.$player && window.$player !== sound)
    ) {
      return;
    }

    if (frameTime - lastPlaybackPresentationAt >= PLAYBACK_PRESENTATION_INTERVAL_MS) {
      lastPlaybackPresentationAt = frameTime;
      scheduleAudioTimePresentation(sound, music, generation);
    }
    timeupdateInterval = requestAnimationFrame(timeLoop);
  };

  timeLoop();
  if (!isPageVisible) {
    startBackgroundMonitor();
  }
};

// Track page visibility for spectrum throttling
if (typeof document !== "undefined") {
  document.addEventListener("visibilitychange", () => {
    isPageVisible = document.visibilityState === "visible";
    if (!isPageVisible) {
      disableActiveNativeAnalysis();
      const sound = SoundManager.getCurrentSound();
      if (sound) checkpointActivePosition(sound, musicStore());
      // RAF is paused while hidden; keep playback time and AutoMix monitoring
      // on a low-rate timer independent of visual spectrum settings.
      startBackgroundMonitor();
    } else {
      requestNativeBackendSync();
      const sound = SoundManager.getCurrentSound();
      syncNativeAnalysisState(sound, settingStore());
      if (sound) {
        checkAudioTime(sound, musicStore());
      }
      stopBackgroundMonitor();
    }
  });
}

if (typeof window !== "undefined") {
  window.addEventListener(NATIVE_AUTOMIX_COMPLETE_EVENT, (event) => {
    const detail =
      (
        event as CustomEvent<{
          currentIndex?: number;
          musicId?: string;
          position?: number;
          duration?: number;
        }>
      ).detail ?? {};
    const currentIndex = Number(detail.currentIndex);
    applyNativeAutoMixCompletion(currentIndex, detail);
  });
  window.addEventListener(NATIVE_AUTOMIX_SYNC_EVENT, (event) => {
    const detail =
      (
        event as CustomEvent<{
          currentIndex?: number;
          musicId?: string;
          position?: number;
          duration?: number;
        }>
      ).detail ?? {};
    const currentIndex = Number(detail.currentIndex);
    applyNativeAutoMixCompletion(currentIndex, detail);
  });
}

/**
 * 停止频谱更新
 */
const stopSpectrumUpdate = (disableNativeAnalysis = true): void => {
  spectrumUpdateGeneration++;
  if (spectrumAnimationId !== null) {
    cancelAnimationFrame(spectrumAnimationId);
    spectrumAnimationId = null;
  }
  clearSpectrumFrame();
  stopBackgroundMonitor();
  if (disableNativeAnalysis) {
    disableActiveNativeAnalysis();
  }
};

/**
 * 启动频谱更新
 * Pauses updates when page is not visible (saves CPU in background)
 * On mobile, skips full spectrum copy when only lowFreqVolume is needed
 * @param sound - 音频对象
 * @param music - pinia store
 */
const startSpectrumUpdate = (sound: ISound, music: ReturnType<typeof musicStore>): void => {
  stopSpectrumUpdate(false);
  const generation = spectrumUpdateGeneration;

  // If page is already hidden, start background monitor immediately
  if (!isPageVisible) {
    startBackgroundMonitor();
  }

  const settings = settingStore();
  const autoMix = getAutoMixEngine();

  const updateLoop = (): void => {
    if (
      generation !== spectrumUpdateGeneration ||
      SoundManager.getCurrentSound() !== sound ||
      (window.$player && window.$player !== sound)
    ) {
      return;
    }

    // AutoMix: monitor playback position per frame
    autoMix.monitorPlayback(sound);

    const needsSpectrum = settings.musicFrequency;
    const needsLowFreq = settings.dynamicFlowSpeed;
    const needsAutoMix = settings.autoMixEnabled;
    syncNativeAnalysisState(sound, settings);
    if (!needsSpectrum && !needsLowFreq && !needsAutoMix) {
      stopSpectrumUpdate();
      return;
    }

    // Skip spectrum computation when page is not visible
    if (isPageVisible) {
      if (needsSpectrum) {
        // Native Rust backend: use the 2048-bin raw FFT frame delivered over
        // the native event Channel. Do not call getFrequencyData() here:
        // the web backend's AnalyserNode path is windowed/smoothed and only
        // 1024 bins by default.
        const spectrumData =
          sound instanceof NativeRustSound ? sound.getFFTData() : sound.getFrequencyData();
        const scale = Math.round((sound.getAverageAmplitude() / 255 + 1) * 100) / 100;
        setSpectrumFrame(spectrumData, scale);
      } else if (needsLowFreq && !(sound instanceof NativeRustSound)) {
        // Web backend only: populate AnalyserNode buffer for mobile fallback.
        // NativeRustSound receives lowFreqVolume from Rust over WS; normalizing
        // its 2048-bin FFT here would be extra main-thread work per RAF.
        sound.getFrequencyData();
      }

      if (needsLowFreq) {
        // 获取低频音量 (直接从 effectManager 计算，已内置平滑处理)
        updateLowFreqStore(music, sound.getLowFrequencyVolume());
      }
    }

    spectrumAnimationId = requestAnimationFrame(updateLoop);
  };

  updateLoop();
};

export const ensureSpectrumUpdate = (): void => {
  const settings = settingStore();
  if (!settings.musicFrequency && !settings.dynamicFlowSpeed && !settings.autoMixEnabled) return;

  const sound = SoundManager.getCurrentSound() ?? (window.$player as ISound | undefined);
  if (!sound) return;

  startSpectrumUpdate(sound, musicStore());
};

/**
 * 获取播放进度
 * @param sound - 音频对象
 * @param music - pinia
 */
const checkAudioTime = (sound: ISound, music: ReturnType<typeof musicStore>): void => {
  if (sound.playing()) {
    if (isNativeRustSoundLike(sound)) {
      const now = Date.now();
      if (now - lastBackendStateAuditAt >= 2000) {
        lastBackendStateAuditAt = now;
        requestNativeBackendSync();
      }
    }
    const currentTime = sound.seek() as number;
    const duration = sound.duration();
    music.setPlaySongTime({ currentTime, duration });
  }
};

/**
 * 生成 MediaSession
 * @param music - pinia
 */
const setMediaSession = (music: ReturnType<typeof musicStore>): void => {
  if ("mediaSession" in navigator && Object.keys(music.getPlaySongData).length) {
    const artists = music.getPlaySongData.artist.map((a: { name: string }) => a.name);
    const picUrl = music.getPlaySongData.album?.picUrl;
    const artwork = picUrl
      ? [
          { src: picUrl.replace(/^http:/, "https:") + "?param=96y96", sizes: "96x96" },
          { src: picUrl.replace(/^http:/, "https:") + "?param=128y128", sizes: "128x128" },
          { src: picUrl.replace(/^http:/, "https:") + "?param=512x512", sizes: "512x512" },
        ]
      : [];

    navigator.mediaSession.metadata = new MediaMetadata({
      title: music.getPlaySongData.name,
      artist: artists.join(" & "),
      album: music.getPlaySongData.album?.name || "",
      artwork,
    });
    navigator.mediaSession.setActionHandler("nexttrack", () => {
      music.setPlaySongIndex("next");
    });
    navigator.mediaSession.setActionHandler("previoustrack", () => {
      music.setPlaySongIndex("prev");
    });
    navigator.mediaSession.setActionHandler("play", () => {
      music.setPlayState(true);
    });
    navigator.mediaSession.setActionHandler("pause", () => {
      music.setPlayState(false);
    });

    // Mobile-specific: seekto support
    if (AudioContextManager.isMobile()) {
      try {
        navigator.mediaSession.setActionHandler("seekto", (details) => {
          if (details.seekTime !== undefined && window.$player) {
            setSeek(window.$player, details.seekTime);
          }
        });
      } catch {
        // seekto not supported
      }
    }
  }
};

/**
 * Set up a NativeRustSound with all the standard event handlers and return it.
 */
const setupNativeSound = (
  sound: NativeRustSound,
  autoPlay: boolean,
  options: { allowInitialBackendAttach?: boolean } = {},
): ISound => {
  const music = musicStore();
  const settings = settingStore();
  const user = userStore();

  SoundManager.setCurrentSound(sound);
  music.loadingStage = "buffering";

  applyGlobalCoverPalette(music.getPlaySongData.album.picUrl).catch((err) => {
    console.error("取色出错", err);
  });

  // Start download + decode in Rust (async). When `memoryLastPlaybackPosition`
  // is on, hand the saved position to `load()` so the Rust side opens the
  // source pre-seeked (via `decoder::open_source_with_fft_at`). This avoids
  // a separate post-load `seek()` round-trip — that round-trip used to race
  // with `SyncStatus` and overwrite the frontend's optimistic position
  // with stale 0, so the progress bar would jump from 0 up to the resumed
  // position instead of starting at it.
  const isMemoryAtLoad = settings.memoryLastPlaybackPosition;
  const savedDurationAtLoad = getUsableDuration(music.getPlaySongTime.duration);
  const savedPosAtLoad = isMemoryAtLoad
    ? normalizePlaybackPosition(music.getPlaySongPlaybackCurrentTime(), savedDurationAtLoad)
    : 0;
  if (savedPosAtLoad > 0) {
    music.setPlaySongTime({ currentTime: savedPosAtLoad, duration: savedDurationAtLoad });
  }

  // ── Load timeout guard ────────────────────────────────────────
  let _loadClearTimeout: ReturnType<typeof setTimeout> | null = setTimeout(() => {
    _loadClearTimeout = null;
    if (music.isLoadingSong) {
      console.warn("[NativeSound] Audio load timeout — force-clearing loading state");
      music.isLoadingSong = false;
      music.loadingStage = "idle";
    }
  }, 15_000);

  const _clearLoadTimeout = (): void => {
    if (_loadClearTimeout !== null) {
      clearTimeout(_loadClearTimeout);
      _loadClearTimeout = null;
    }
  };

  // ── Load ──────────────────────────────────────────────────────
  sound.once("load", () => {
    _clearLoadTimeout();
    music.loadingStage = "idle";
    const songId = music.getPlaySongData?.id;
    const sourceId = music.getPlaySongData?.sourceId ? music.getPlaySongData.sourceId : 0;
    const isLogin = user.userLogin;

    if (IS_DEV) {
      console.log("[Native] 首次缓冲完成：" + songId + " / 来源：" + sourceId);
    }

    if (!isMemoryAtLoad) {
      // Saved position is disabled — wipe persisted time so subsequent
      // sessions start fresh.
      resetPlaySongTime(music);
    } else if (savedPosAtLoad > 0) {
      const duration = getUsableDuration(sound.duration(), savedDurationAtLoad);
      const currentTime = normalizePlaybackPosition(
        (sound.seek() as number) || savedPosAtLoad,
        duration,
        savedPosAtLoad,
      );
      music.setPlaySongTime({ currentTime, duration });
    }
    // When `isMemoryAtLoad` is true, the resumed position was already baked into
    // the load via `jumpToSongAt` (see setupNativeSound's `sound.load(savedPos)`
    // call). No extra `sound.seek()` here — that used to race with the
    // first `SyncStatus` and overwrite the optimistic local position.
    music.isLoadingSong = false;
    if (isLogin) scheduleScrobble("native-load");

    // Sync volume from the store. The Rust backend creates a fresh
    // player with volume=1.0; we restore the user's setting.
    const vol = music.persistData.playVolume;

    // For native backend: the volume atomic doesn't click/pop on
    // changes, so a fade-in from 0 is unnecessary.  Setting volume(0)
    // then volume(vol) would only race two IPC calls — if the second
    // arrives before the first the volume stays at 0 permanently.
    // Just restore the user's volume directly.
    sound.volume(vol);

    if (autoPlay) {
      const alreadyPlaying = sound.playing();
      sound.play();
      if (alreadyPlaying) {
        handleNativePlay();
      }
    } else {
      sound.pause();
    }
  });

  // ── Play ──────────────────────────────────────────────────────
  function handleNativePlay(): void {
    if (window.$player && window.$player !== sound) return;
    const autoMixCheck = getAutoMixEngine();
    if (autoMixCheck.isCrossfading()) return;

    stopTimeUpdate();

    const playSongData = music.getPlaySongData;
    if (!Object.keys(playSongData).length) {
      window.$message.error(getLanguageData("songLoadError"));
      return;
    }

    const songName = playSongData?.name;
    const songArtist = playSongData.artist[0]?.name;

    testNumber = 0;
    music.setPlayState(true);

    if (typeof window.$message !== "undefined" && songArtist !== null) {
      window.$message.info(`${songName} - ${songArtist}`, {
        icon: () => h(NIcon, null, { default: () => h(MusicNoteFilled) }),
      });
    } else {
      window.$message.warning(getLanguageData("songNotDetails"));
    }

    if (IS_DEV) console.log(`[Native] 开始播放：${songName} - ${songArtist}`);

    setMediaSession(music);
    getAudioPreloader().preloadNext();
    void prefillNativeQueue();
    music.preloadUpcomingSongs();

    const songId = music.getPlaySongData?.id;
    if (songId) {
      getAutoMixEngine().onTrackStarted(sound, songId);
    }

    startTimeUpdate(sound, music);
    syncNativeAnalysisState(sound, settings);

    music.setPlayHistory(playSongData);
    window.document.title = `${songName} - ${songArtist} - ${import.meta.env.VITE_SITE_TITLE}`;

    if (settings.musicFrequency || settings.dynamicFlowSpeed || settings.autoMixEnabled) {
      startSpectrumUpdate(sound, music);
    }
  }

  sound.on("play", handleNativePlay);

  // ── Pause ─────────────────────────────────────────────────────
  sound.on("pause", () => {
    if (window.$player && window.$player !== sound) return;
    const autoMix = getAutoMixEngine();
    if (autoMix.isCrossfading()) return;
    checkpointActivePosition(sound, music);
    stopTimeUpdate();
    if (IS_DEV) console.log("[Native] 音乐暂停");
    music.setPlayState(false);
    window.$setSiteTitle("");
  });

  // ── End ───────────────────────────────────────────────────────
  sound.on("end", () => {
    if (window.$player && window.$player !== sound) return;
    stopTimeUpdate();
    stopSpectrumUpdate();
    if (IS_DEV) console.log("[Native] 歌曲播放结束");
    const autoMixEngine = getAutoMixEngine();
    if (!autoMixEngine.isCrossfading()) {
      music.setPlaySongIndex("next");
    }
  });

  // ── Errors ────────────────────────────────────────────────────
  sound.on("loaderror", () => {
    _clearLoadTimeout();
    music.loadingStage = "error";
    if (testNumber < 4) {
      testNumber++;
      if (music.getPlaylists[0]) window.$getPlaySongData(music.getPlaySongData);
    } else {
      window.$message.error(getLanguageData("songLoadTest"), { closable: true, duration: 0 });
      music.isLoadingSong = false;
    }
  });

  sound.on("playerror", () => {
    _clearLoadTimeout();
    music.loadingStage = "error";
    music.setPlayState(false);
    music.isLoadingSong = false;
    window.$message.error(getLanguageData("songPlayError"));
    console.error(getLanguageData("songPlayError"));
  });

  void sound.load(savedPosAtLoad, {
    allowInitialBackendAttach: options.allowInitialBackendAttach === true,
  });
  return (window.$player = sound);
};

/**
 * 创建音频对象
 * @param src - 音频文件地址
 * @param autoPlay - 是否自动播放（默认为 true）
 * @return 音频对象
 */
export const createSound = (
  src: string,
  autoPlay = true,
  preloadedSound?: BufferedSound,
): ISound | undefined => {
  try {
    // If AutoMix is crossfading, it handles sound creation — skip normal flow
    const autoMix = getAutoMixEngine();
    if (autoMix.isHandoffActive()) {
      if (IS_DEV) {
        console.log("[createSound] AutoMix handoff active, skipping normal sound creation");
      }
      return window.$player;
    }

    const allowInitialBackendAttach =
      isNativeAudioBackendAvailable() && !SoundManager.hasSound() && !window.$player;

    SoundManager.unload();
    stopSpectrumUpdate();

    // If not using a preloaded sound, clean up any orphaned preload
    if (!preloadedSound) {
      getAudioPreloader().cleanup();
    }

    // ── Rust audio backend (native Tauri or WebAssembly runtime) ────────────
    const backendAvailable = isAudioBackendRuntimeAvailable();
    if (IS_DEV) {
      console.log(
        "[createSound] audioBackendAvailable:",
        backendAvailable,
        "src:",
        !!src,
        "preloaded:",
        !!preloadedSound,
        "isTauri:",
        "__TAURI__" in window,
        window.__TAURI__,
      );
    }
    if (backendAvailable && src) {
      console.log(
        isNativeAudioBackendAvailable()
          ? "[createSound] Using NATIVE audio backend"
          : "[createSound] Using WASM audio backend",
      );
      const sound = new NativeRustSound(src);
      return setupNativeSound(sound, autoPlay, { allowInitialBackendAttach });
    }
    console.log("[createSound] Using WEB audio backend");

    // ── Web Audio backend (BufferedSound) ─────────────────────────────────────

    const music = musicStore();
    const settings = settingStore();
    const user = userStore();

    // Use preloaded sound or create a new BufferedSound
    const sound =
      preloadedSound ??
      new BufferedSound({
        src: [src],
        preload: true,
        volume: music.persistData.playVolume,
      });
    if (preloadedSound) {
      // Preloaded sound was created with volume=0 — restore actual volume
      sound.volume(music.persistData.playVolume);
    }
    SoundManager.setCurrentSound(sound);
    // Mark the loading stage so the UI can show "Buffering…" instead of a generic spinner.
    music.loadingStage = "buffering";

    // 更新取色
    applyGlobalCoverPalette(music.getPlaySongData.album.picUrl).catch((err) => {
      console.error("取色出错", err);
    });

    if (IS_DEV) {
      console.log("[createSound] autoPlay:", autoPlay, "getPlayState:", music.getPlayState);
    }

    const isMemoryAtLoad = settings.memoryLastPlaybackPosition;
    const savedDurationAtLoad = getUsableDuration(music.getPlaySongTime.duration);
    const savedPosAtLoad = isMemoryAtLoad
      ? normalizePlaybackPosition(music.getPlaySongPlaybackCurrentTime(), savedDurationAtLoad)
      : 0;

    if (savedPosAtLoad > 0) {
      sound.seek(savedPosAtLoad);
      music.setPlaySongTime({ currentTime: savedPosAtLoad, duration: savedDurationAtLoad });
    } else if (!isMemoryAtLoad) {
      resetPlaySongTime(music);
    }

    if (autoPlay) {
      fadePlayOrPause(sound, "play", music.persistData.playVolume);
    }

    // ── Load timeout guard ────────────────────────────────────────────────────────
    // If the audio element never fires "load" (e.g. hung network request), clear the
    // stuck loading state after 15 s so the spinner doesn't stay forever.
    let _loadClearTimeout: ReturnType<typeof setTimeout> | null = setTimeout(() => {
      _loadClearTimeout = null;
      if (music.isLoadingSong) {
        console.warn("[createSound] Audio load timeout — force-clearing loading state");
        music.isLoadingSong = false;
        music.loadingStage = "idle";
      }
    }, 15_000);

    const _clearLoadTimeout = (): void => {
      if (_loadClearTimeout !== null) {
        clearTimeout(_loadClearTimeout);
        _loadClearTimeout = null;
      }
    };

    // 首次加载事件
    sound?.once("load", () => {
      _clearLoadTimeout();
      music.loadingStage = "idle";
      const songId = music.getPlaySongData?.id;
      const sourceId = music.getPlaySongData?.sourceId ? music.getPlaySongData.sourceId : 0;
      const isLogin = user.userLogin;

      if (IS_DEV) {
        console.log("首次缓冲完成：" + songId + " / 来源：" + sourceId);
      }

      if (isMemoryAtLoad) {
        const duration = getUsableDuration(sound.duration(), savedDurationAtLoad);
        const currentTime = normalizePlaybackPosition(
          (sound.seek() as number) || savedPosAtLoad,
          duration,
          savedPosAtLoad,
        );
        if (currentTime > 0) {
          sound.seek(currentTime);
          music.setPlaySongTime({ currentTime, duration });
        }
      } else {
        resetPlaySongTime(music);
      }
      // 取消加载状态
      music.isLoadingSong = false;
      // 听歌打卡
      if (isLogin) scheduleScrobble("load");
    });

    // 播放事件
    sound?.on("play", () => {
      // If this sound is no longer the active player (e.g., AutoMix transitioned
      // to a new sound), don't run the full play handler — avoids duplicate
      // notifications, wrong time tracking, and wrong spectrum monitoring.
      if (window.$player && window.$player !== sound) return;
      // During AutoMix crossfade, this sound is the outgoing one being resumed —
      // skip to prevent side effects (window.$player may not be updated yet).
      const autoMixCheck = getAutoMixEngine();
      if (autoMixCheck.isCrossfading()) {
        if (IS_DEV) {
          console.log("[createSound play handler] Skipped during AutoMix crossfade");
        }
        return;
      }
      stopTimeUpdate();
      const playSongData = music.getPlaySongData;
      if (!Object.keys(playSongData).length) {
        window.$message.error(getLanguageData("songLoadError"));
        return;
      }

      const songName = playSongData?.name;
      const songArtist = playSongData.artist[0]?.name;

      testNumber = 0;
      music.setPlayState(true);

      // 播放通知
      if (typeof window.$message !== "undefined" && songArtist !== null) {
        window.$message.info(`${songName} - ${songArtist}`, {
          icon: () =>
            h(NIcon, null, {
              default: () => h(MusicNoteFilled),
            }),
        });
      } else {
        window.$message.warning(getLanguageData("songNotDetails"));
      }

      if (IS_DEV) {
        console.log(`开始播放：${songName} - ${songArtist}`);
      }
      setMediaSession(music);

      // 预加载下一首
      getAudioPreloader().preloadNext();
      music.preloadUpcomingSongs();

      // Notify AutoMix engine that a new track started
      const songId = music.getPlaySongData?.id;
      if (songId) {
        getAutoMixEngine().onTrackStarted(sound, songId);
      }

      startTimeUpdate(sound, music);

      // 写入播放历史
      music.setPlayHistory(playSongData);

      // 播放时页面标题
      window.document.title = `${songName} - ${songArtist} - ${import.meta.env.VITE_SITE_TITLE}`;

      // 启动频谱更新 (also needed for AutoMix playback monitoring)
      if (settings.musicFrequency || settings.dynamicFlowSpeed || settings.autoMixEnabled) {
        startSpectrumUpdate(sound, music);
      }
    });

    // 暂停事件
    sound?.on("pause", () => {
      // If this sound is no longer the active player, ignore
      if (window.$player && window.$player !== sound) return;
      // During AutoMix crossfade, outgoing sound pausing is expected — don't change play state
      const autoMix = getAutoMixEngine();
      if (autoMix.isCrossfading()) {
        if (IS_DEV) {
          console.log("[pause handler] Ignored during AutoMix crossfade");
        }
        return;
      }
      checkpointActivePosition(sound, music);
      stopTimeUpdate();
      if (IS_DEV) {
        console.log("音乐暂停");
      }
      music.setPlayState(false);
      // 更改页面标题
      window.$setSiteTitle("");
    });
    // 结束事件
    sound?.on("end", () => {
      // If this sound is no longer the active player (e.g., AutoMix transitioned
      // to a new sound), don't cancel the current player's time/spectrum loops
      if (window.$player && window.$player !== sound) return;
      stopTimeUpdate();
      stopSpectrumUpdate();
      if (IS_DEV) {
        console.log("歌曲播放结束");
      }
      // If AutoMix handled the transition, don't trigger next song again
      const autoMixEngine = getAutoMixEngine();
      if (!autoMixEngine.isCrossfading()) {
        music.setPlaySongIndex("next");
      }
    });
    // 错误事件
    sound?.on("loaderror", () => {
      _clearLoadTimeout();
      music.loadingStage = "error";
      if (testNumber > 2) {
        window.$message.error(getLanguageData("songPlayError"));
        console.error(getLanguageData("songPlayError"));
        music.setPlayState(false);
      }
      if (testNumber < 4) {
        if (music.getPlaylists[0]) window.$getPlaySongData(music.getPlaySongData);
        testNumber++;
      } else {
        window.$message.error(getLanguageData("songLoadTest"), {
          closable: true,
          duration: 0,
        });
        music.isLoadingSong = false;
      }
    });
    sound?.on("playerror", () => {
      _clearLoadTimeout();
      music.loadingStage = "error";
      window.$message.error(getLanguageData("songPlayError"));
      console.error(getLanguageData("songPlayError"));
      music.setPlayState(false);
      music.isLoadingSong = false;
    });

    // Stalled: network issue while buffering → surface it in the UI.
    sound?.on("stalled", () => {
      if (window.$player && window.$player !== sound) return;
      music.loadingStage = "stalled";
    });

    // Waiting: data ran out mid-playback (rebuffering) → reset to buffering stage.
    sound?.on("waiting", () => {
      if (window.$player && window.$player !== sound) return;
      if (music.isLoadingSong) music.loadingStage = "buffering";
    });

    // 返回音频对象
    return (window.$player = sound);
  } catch (err) {
    window.$message.error(getLanguageData("songLoadError"));
    console.error(getLanguageData("songLoadError"), err);
  }
};

/**
 * 设置音量
 * @param sound - 音频对象
 * @param volume - 设置的音量值，0-1之间的浮点数
 */
export const setVolume = (sound: ISound | undefined, volume: number): void => {
  sound?.volume(volume);

  // When AutoMix normalization is active, apply the persistent gain adjustment
  // to the GainNode so that manual volume changes maintain LUFS normalization.
  const autoMix = getAutoMixEngine();
  const gainAdj = autoMix.getActiveGainAdjustment();
  if (gainAdj !== 1 && sound) {
    const gainNode = (sound as any).getGainNode?.();
    if (gainNode) {
      gainNode.gain.value = volume * gainAdj;
    }
  }
};

/**
 * 设置进度
 * @param sound - 音频对象
 * @param seek - 设置的进度值（秒）
 */
export const setSeek = (sound: ISound | undefined, seek: number): void => {
  const music = musicStore();
  const target = resolveSeekTarget(sound);
  if (!target) {
    if (IS_DEV) {
      console.warn("[setSeek] ignored seek because no active sound is available", { seek });
    }
    return;
  }
  pendingPlaybackPresentation = null;
  if (playbackPresentationTimer !== null) {
    clearTimeout(playbackPresentationTimer);
    playbackPresentationTimer = null;
  }
  const storedDuration = getUsableDuration(music.getPlaySongTime.duration);
  const soundDuration = getUsableDuration(target.duration(), storedDuration);
  const currentTime = normalizePlaybackPosition(
    seek,
    soundDuration,
    music.getPlaySongPlaybackCurrentTime(),
  );
  // Cancel AutoMix crossfade on seek
  const autoMix = getAutoMixEngine();
  if (autoMix.isCrossfading()) {
    autoMix.cancelCrossfade();
  }
  target.seek(currentTime);
  // 直接调用 setPlaySongTime 确保 UI 状态立即更新
  music.setPlaySongTime({
    currentTime,
    duration: soundDuration,
    displayCurrentTime: currentTime,
  });
  music.checkpointPlaySongTime(true);
};

/**
 * 音频渐入渐出
 * @param sound - 音频对象
 * @param type - 渐入还是渐出 ('play' | 'pause')
 * @param volume - 渐出音量的大小，0-1之间的浮点数
 * @param duration - 渐出音量的时长，单位为毫秒
 */
export const fadePlayOrPause = (
  sound: ISound | undefined,
  type: "play" | "pause",
  volume: number,
  duration = 300,
): void => {
  if (IS_DEV) {
    console.log("[fadePlayOrPause] type:", type, "sound:", !!sound, "playing:", sound?.playing());
  }
  if (sound instanceof NativeRustSound) {
    if (type === "play") {
      sound.play();
    } else {
      sound.pause();
    }
    return;
  }

  const settingData = JSON.parse(localStorage.getItem("settingData") || "{}");
  const isFade = settingData.songVolumeFade ?? true;
  if (isFade) {
    if (type === "play") {
      if (sound?.playing()) {
        return;
      }
      sound?.play();
      sound?.once("play", () => {
        sound?.fade(0, volume, duration);
      });
    } else if (type === "pause") {
      sound?.fade(volume, 0, duration);
      sound?.once("fade", () => {
        sound?.pause();
      });
    }
  } else {
    if (type === "play") {
      sound?.play();
    } else {
      sound?.pause();
    }
  }
};

/**
 * 停止播放器
 * @param sound - 音频对象
 */
export const soundStop = (sound: ISound | undefined): void => {
  sound?.stop();
  setSeek(sound, 0);
};

/**
 * Adopt an incoming sound created by AutoMix.
 * Registers event handlers (pause/end/error), starts time tracking and spectrum updates.
 * Called after the incoming sound is already playing.
 * @param incomingSound - The incoming sound instance from AutoMix crossfade
 */
export const adoptIncomingSound = (incomingSound: ISound): void => {
  const music = musicStore();
  const settings = settingStore();

  // Stop any existing tracking from outgoing sound
  stopSpectrumUpdate();
  stopTimeUpdate();

  // Ensure play state is true (incoming is already playing)
  music.setPlayState(true);
  music.isLoadingSong = false;
  scheduleScrobble("automix-adopt");

  // Initialize time from the incoming sound immediately.
  // The song data watcher no longer resets time during crossfade (to prevent duration=0),
  // so we must do it here to avoid briefly showing the outgoing song's time.
  const initDuration = incomingSound.duration();
  const initTime = (incomingSound.seek() as number) || 0;
  music.setPlaySongTime({
    currentTime: initTime,
    duration: initDuration > 0 ? initDuration : 0,
  });

  // Update cover color for the new track (AutoMix bypasses createSound, so do it here)
  const coverUrl = music.getPlaySongData?.album?.picUrl;
  if (coverUrl) {
    applyGlobalCoverPalette(coverUrl).catch((err) => {
      console.error("取色出错 (AutoMix)", err);
    });
  }

  // Start time tracking immediately
  startTimeUpdate(incomingSound, music);
  syncNativeAnalysisState(incomingSound, settings);

  // Start spectrum update (also handles AutoMix monitorPlayback per frame)
  if (settings.musicFrequency || settings.dynamicFlowSpeed || settings.autoMixEnabled) {
    startSpectrumUpdate(incomingSound, music);
  }

  // Update media session with current song info
  setMediaSession(music);

  // Write play history & preload next songs (AutoMix bypasses createSound's play handler)
  const playSongData = music.getPlaySongData;
  if (playSongData) {
    music.setPlayHistory(playSongData);
    // Page title
    const songName = playSongData.name;
    const songArtist = playSongData.artist?.[0]?.name;
    if (songName && songArtist) {
      window.document.title = `${songName} - ${songArtist} - ${import.meta.env.VITE_SITE_TITLE}`;
    }
  }
  getAudioPreloader().preloadNext();
  music.preloadUpcomingSongs();

  // Register pause handler with crossfade guard
  incomingSound.on("pause", () => {
    // If this sound is no longer the active player, ignore
    if (window.$player && window.$player !== incomingSound) return;
    const autoMix = getAutoMixEngine();
    if (autoMix.isCrossfading()) {
      if (IS_DEV) {
        console.log("[adoptIncomingSound pause] Ignored during AutoMix crossfade");
      }
      return;
    }
    checkpointActivePosition(incomingSound, music);
    stopTimeUpdate();
    if (IS_DEV) {
      console.log("音乐暂停 (adopted sound)");
    }
    music.setPlayState(false);
    window.$setSiteTitle("");
  });

  // Register play handler to restart time loop + spectrum after pause/resume.
  // Without this, pausing and resuming after AutoMix crossfade permanently
  // kills time tracking (pause handler cancels timeupdateInterval but nothing restarts it).
  incomingSound.on("play", () => {
    if (window.$player && window.$player !== incomingSound) return;

    music.setPlayState(true);

    // Restart time tracking
    startTimeUpdate(incomingSound, music);
    syncNativeAnalysisState(incomingSound, settings);

    // Restart spectrum + AutoMix monitoring
    if (settings.musicFrequency || settings.dynamicFlowSpeed || settings.autoMixEnabled) {
      startSpectrumUpdate(incomingSound, music);
    }
  });

  // Register end handler
  incomingSound.on("end", () => {
    // If this sound is no longer the active player, don't cancel current loops
    if (window.$player && window.$player !== incomingSound) return;
    stopTimeUpdate();
    stopSpectrumUpdate();
    if (IS_DEV) {
      console.log("歌曲播放结束 (adopted sound)");
    }
    const autoMixEngine = getAutoMixEngine();
    if (!autoMixEngine.isCrossfading()) {
      music.setPlaySongIndex("next");
    }
  });

  // Register error handlers
  incomingSound.on("loaderror", () => {
    music.setPlayState(false);
  });
  incomingSound.on("playerror", () => {
    music.setPlayState(false);
  });

  if (IS_DEV) {
    console.log("[adoptIncomingSound] Adopted incoming sound with event handlers");
  }
};

/**
 * Sync frontend state after the native backend has promoted its prepared deck.
 * This intentionally does not register sound event handlers because the
 * NativeRustSound instance is still the same backend controller.
 */
export const syncNativeAutoMixCurrentSound = async (sound: ISound): Promise<void> => {
  const music = musicStore();
  const settings = settingStore();

  if (isNativeRustSoundLike(sound)) {
    await sound.requestStatusSync();
  }

  stopSpectrumUpdate();
  stopTimeUpdate();

  music.setPlayState(true);
  music.isLoadingSong = false;

  const duration = sound.duration();
  const currentTime = (sound.seek() as number) || 0;
  music.setPlaySongTime({
    currentTime,
    duration: duration > 0 ? duration : 0,
  });

  const coverUrl = music.getPlaySongData?.album?.picUrl;
  if (coverUrl) {
    applyGlobalCoverPalette(coverUrl).catch((err) => {
      console.error("取色出错 (Native AutoMix)", err);
    });
  }

  startTimeUpdate(sound, music);
  syncNativeAnalysisState(sound, settings);
  if (settings.musicFrequency || settings.dynamicFlowSpeed || settings.autoMixEnabled) {
    startSpectrumUpdate(sound, music);
  }

  setMediaSession(music);

  const playSongData = music.getPlaySongData;
  if (playSongData) {
    music.setPlayHistory(playSongData);
    const songName = playSongData.name;
    const songArtist = playSongData.artist?.[0]?.name;
    if (songName && songArtist) {
      window.document.title = `${songName} - ${songArtist} - ${import.meta.env.VITE_SITE_TITLE}`;
    }
  }

  getAudioPreloader().preloadNext();
  void prefillNativeQueue();
  music.preloadUpcomingSongs();
};

/**
 * 生成频谱数据 - 快速傅里叶变换（ FFT ）
 * @deprecated Use NativeSound.getFrequencyData() instead
 * @param sound - NativeSound 音频对象
 */
/**
 * After WebAudio AutoMix crossfade, switch the already-playing incoming track
 * back to the native backend so Rust FFT/lowfreq events resume.
 */
export const handoffAutoMixToNativeBackend = async (
  currentSound: ISound,
  sourceUrl: string | null | undefined,
): Promise<boolean> => {
  if (!sourceUrl || !isNativeAudioBackendAvailable()) return false;
  if (currentSound instanceof NativeRustSound) return false;
  if (!currentSound.playing()) return false;

  const music = musicStore();
  const position = Math.max(0, (currentSound.seek() as number) || 0);
  const volume = music.persistData.playVolume;
  const nativeSound = new NativeRustSound(sourceUrl);

  try {
    await nativeSound.load(position);

    if (SoundManager.getCurrentSound() !== currentSound || window.$player !== currentSound) {
      nativeSound.unload();
      return false;
    }

    const latestPosition = Math.max(0, (currentSound.seek() as number) || position);
    nativeSound.seek(latestPosition);
    nativeSound.volume(volume);

    SoundManager.setCurrentSound(nativeSound);
    window.$player = nativeSound;
    adoptIncomingSound(nativeSound);
    nativeSound.play();

    currentSound.stop();
    currentSound.unload();

    if (IS_DEV) {
      console.log(
        `[AutoMix handoff] Restored native backend at ${latestPosition.toFixed(2)}s for FFT/lowfreq`,
      );
    }
    return true;
  } catch (err) {
    nativeSound.unload();
    SoundManager.setCurrentSound(currentSound);
    window.$player = currentSound;
    if (IS_DEV) {
      console.warn("[AutoMix handoff] Native backend restore failed", err);
    }
    return false;
  }
};

/**
 * Generate spectrum data.
 * @deprecated Use NativeSound.getFrequencyData() instead.
 */
export const processSpectrum = (_sound: ISound | undefined): void => {
  // No longer needed - spectrum is handled internally by NativeSound
  if (IS_DEV) {
    console.log("processSpectrum called - now handled internally by NativeSound");
  }
};

/**
 * Set page visibility state for spectrum/animation suspension.
 * Called from Tauri main-window-visibility events (hide() doesn't trigger visibilitychange).
 */
export const setPageVisible = (visible: boolean): void => {
  isPageVisible = visible;
  if (!visible) {
    disableActiveNativeAnalysis();
    startBackgroundMonitor();
  } else if (visible) {
    requestNativeBackendSync();
    const sound = SoundManager.getCurrentSound();
    syncNativeAnalysisState(sound, settingStore());
    if (sound) {
      checkAudioTime(sound, musicStore());
    }
    stopBackgroundMonitor();
  }
};
