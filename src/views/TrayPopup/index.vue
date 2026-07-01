<template>
  <div
    ref="popupRef"
    :class="['tray-popup', { 'native-effect': hasNativeEffect, 'is-playing': isPlaying }]"
  >
    <button class="song-section" type="button" :title="title || 'GMPlayer'" @click="showMainWindow">
      <img class="cover" :src="coverUrl || '/images/pic/default.png'" alt="" />
      <span class="song-text">
        <span class="title">{{ title || "GMPlayer" }}</span>
        <span class="artist">{{ artist }}</span>
      </span>
    </button>

    <div class="divider"></div>

    <div class="controls">
      <button
        :class="['ctrl-btn', { liked: isLiked }]"
        type="button"
        :title="likeTitle"
        @click="toggleLike"
      >
        <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor">
          <path :d="likePath" />
        </svg>
      </button>
      <button class="ctrl-btn" type="button" title="Previous" @click="prevTrack">
        <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor">
          <path d="M6 6h2v12H6zm3.5 6l8.5 6V6z" />
        </svg>
      </button>
      <button
        class="ctrl-btn play"
        type="button"
        :title="isPlaying ? 'Pause' : 'Play'"
        @click="playPause"
      >
        <svg class="icon-play" viewBox="0 0 24 24" width="26" height="26" fill="currentColor">
          <path d="M8 5v14l11-7z" />
        </svg>
        <svg class="icon-pause" viewBox="0 0 24 24" width="26" height="26" fill="currentColor">
          <path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z" />
        </svg>
      </button>
      <button class="ctrl-btn" type="button" title="Next" @click="nextTrack">
        <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor">
          <path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z" />
        </svg>
      </button>
      <button class="ctrl-btn" type="button" :title="modeTitle" @click="cyclePlayMode">
        <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor">
          <path :d="modePath" />
        </svg>
      </button>
    </div>

    <div class="volume-row">
      <button class="volume-icon" type="button" :title="volumeTitle" @click="toggleMute">
        <svg viewBox="0 0 24 24" width="17" height="17" fill="currentColor">
          <path :d="volumePath" />
        </svg>
      </button>
      <BouncingSlider
        class="volume-slider"
        :value="volume"
        :min="0"
        :max="1"
        :change-on-drag="true"
        @update:value="handleVolumeUpdate"
      />
      <span class="volume-value">{{ volumePercent }}%</span>
    </div>

    <div class="divider"></div>

    <div class="menu-section">
      <button class="menu-item" type="button" @click="showMainWindow">
        <svg class="mi-icon" viewBox="0 0 24 24" fill="currentColor">
          <path
            d="M21 3H3c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h18c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zm0 16H3V5h18v14zM5 15h14v2H5z"
          />
        </svg>
        <span>{{ t.openMain }}</span>
      </button>
      <button class="menu-item" type="button" @click="openMiniPlayer">
        <svg class="mi-icon" viewBox="0 0 24 24" fill="currentColor">
          <path
            d="M19 11h-8v6h8v-6zm4 8V4.98C23 3.88 22.1 3 21 3H3c-1.1 0-2 .88-2 1.98V19c0 1.1.9 2 2 2h18c1.1 0 2-.9 2-2zm-2 .02H3V4.97h18v14.05z"
          />
        </svg>
        <span>{{ t.miniPlayer }}</span>
      </button>
      <button class="menu-item" type="button" @click="openDesktopLyrics">
        <svg class="mi-icon" viewBox="0 0 24 24" fill="currentColor">
          <path
            d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"
          />
        </svg>
        <span>{{ t.desktopLyrics }}</span>
      </button>
      <button
        v-if="showTaskbarLyricsEntry"
        class="menu-item taskbar"
        type="button"
        @click="openTaskbarLyrics"
      >
        <svg class="mi-icon" viewBox="0 0 24 24" fill="currentColor">
          <path
            d="M5 5h14a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-5.4l-3.1 3.1a1 1 0 0 1-1.7-.7V16H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2Zm0 2v7h5.8v2l2-2H19V7H5Zm2 2h4v2H7V9Zm6 0h4v2h-4V9Z"
          />
        </svg>
        <span>
          <span class="menu-title">{{ t.taskbarLyrics }}</span>
          <span class="menu-subtitle">{{ t.windowsOnly }}</span>
        </span>
      </button>
      <button class="menu-item" type="button" @click="openSettings">
        <svg class="mi-icon" viewBox="0 0 24 24" fill="currentColor">
          <path
            d="M19.14 12.94c.04-.3.06-.61.06-.94 0-.32-.02-.64-.07-.94l2.03-1.58c.18-.14.23-.41.12-.61l-1.92-3.32c-.12-.22-.37-.29-.59-.22l-2.39.96c-.5-.38-1.03-.7-1.62-.94l-.36-2.54c-.04-.24-.24-.41-.48-.41h-3.84c-.24 0-.43.17-.47.41l-.36 2.54c-.59.24-1.13.57-1.62.94l-2.39-.96c-.22-.08-.47 0-.59.22L2.74 8.87c-.12.21-.08.47.12.61l2.03 1.58c-.05.3-.07.62-.07.94s.02.64.07.94l-2.03 1.58c-.18.14-.23.41-.12.61l1.92 3.32c.12.22.37.29.59.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.05.24.24.41.48.41h3.84c.24 0 .44-.17.47-.41l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.47 0 .59-.22l1.92-3.32c.12-.22.07-.47-.12-.61l-2.01-1.58zM12 15.6c-1.98 0-3.6-1.62-3.6-3.6s1.62-3.6 3.6-3.6 3.6 1.62 3.6 3.6-1.62 3.6-3.6 3.6z"
          />
        </svg>
        <span>{{ t.settings }}</span>
      </button>
    </div>

    <div class="divider"></div>

    <button class="menu-item quit" type="button" @click="quitApp">
      <svg class="mi-icon" viewBox="0 0 24 24" fill="currentColor">
        <path
          d="M13 3h-2v10h2V3zm4.83 2.17l-1.42 1.42C17.99 7.86 19 9.81 19 12c0 3.87-3.13 7-7 7s-7-3.13-7-7c0-2.19 1.01-4.14 2.58-5.42L6.17 5.17C4.23 6.82 3 9.26 3 12c0 4.97 4.03 9 9 9s9-4.03 9-9c0-2.74-1.23-5.18-3.17-6.83z"
        />
      </svg>
      <span>{{ t.quit }}</span>
    </button>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import BouncingSlider from "@/components/Player/BouncingSlider.vue";

type PlayMode = "normal" | "random" | "single";

interface PlayerStatePayload {
  title?: string;
  artist?: string;
  coverUrl?: string;
  isPlaying?: boolean;
  isLiked?: boolean;
  volume?: number;
  playMode?: PlayMode;
}

const title = ref("");
const artist = ref("");
const coverUrl = ref("");
const isPlaying = ref(false);
const isLiked = ref(false);
const volume = ref(0.7);
const previousVolume = ref(0.7);
const playMode = ref<PlayMode>("normal");
const showTaskbarLyricsEntry = ref(false);
const popupRef = ref<HTMLElement | null>(null);

const unlisteners: Array<() => void> = [];
let layoutFrame = 0;

const lang = (navigator.language || "").startsWith("zh") ? "zh" : "en";
const t = {
  zh: {
    normal: "列表循环",
    random: "随机播放",
    single: "单曲循环",
    like: "喜欢歌曲",
    unlike: "取消喜欢",
    volume: "音量",
    openMain: "打开主窗口",
    miniPlayer: "迷你播放器",
    desktopLyrics: "桌面歌词",
    taskbarLyrics: "任务栏歌词",
    windowsOnly: "Windows 10/11 only",
    settings: "设置",
    quit: "退出",
  },
  en: {
    normal: "List Loop",
    random: "Shuffle",
    single: "Single Loop",
    like: "Like Song",
    unlike: "Unlike",
    volume: "Volume",
    openMain: "Main Window",
    miniPlayer: "Mini Player",
    desktopLyrics: "Desktop Lyrics",
    taskbarLyrics: "Taskbar Lyrics",
    windowsOnly: "Windows 10/11 only",
    settings: "Settings",
    quit: "Quit",
  },
}[lang];

const ua = navigator.userAgent || "";
const hasNativeEffect = /Windows|Macintosh/i.test(ua);

const likeIcons = {
  liked:
    "M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z",
  unliked:
    "M16.5 3c-1.74 0-3.41.81-4.5 2.09C10.91 3.81 9.24 3 7.5 3 4.42 3 2 5.42 2 8.5c0 3.78 3.4 6.86 8.55 11.54L12 21.35l1.45-1.32C18.6 15.36 22 12.28 22 8.5 22 5.42 19.58 3 16.5 3zm-4.4 15.55l-.1.1-.1-.1C7.14 14.24 4 11.39 4 8.5 4 6.5 5.5 5 7.5 5c1.54 0 3.04.99 3.57 2.36h1.87C13.46 5.99 14.96 5 16.5 5c2 0 3.5 1.5 3.5 3.5 0 2.89-3.14 5.74-7.9 10.05z",
};

const modeIcons: Record<PlayMode, string> = {
  normal: "M7 7h10v3l4-4-4-4v3H5v6h2V7zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4z",
  random:
    "M10.59 9.17L5.41 4 4 5.41l5.17 5.18 1.42-1.42zM14.5 4l2.04 2.04L4 18.59 5.41 20 17.96 7.46 20 9.5V4h-5.5zm.33 9.41l-1.41 1.41 3.13 3.13L14.5 20H20v-5.5l-2.04 2.04-3.13-3.13z",
  single:
    "M7 7h10v3l4-4-4-4v3H5v6h2V7zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4zm-4-2V9h-1l-2 1v1h1.5v4H13z",
};

const volumeIcons = {
  muted:
    "M16.5 12c0-1.77-1-3.29-2.5-4.03v2.21l2.45 2.45c.03-.2.05-.41.05-.63zM19 12c0 .94-.2 1.82-.54 2.64l1.51 1.51C20.62 14.91 21 13.5 21 12c0-4.28-2.99-7.86-7-8.77v2.06c2.89.86 5 3.54 5 6.71zM4.27 3 3 4.27 7.73 9H3v6h4l5 5v-6.73L16.25 17.52c-.67.52-1.43.93-2.25 1.18v2.06c1.38-.31 2.63-.95 3.69-1.81L19.73 21 21 19.73l-9-9L4.27 3zM12 4 9.91 6.09 12 8.18V4z",
  low: "M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1-3.29-2.5-4.03v8.05c1.5-.73 2.5-2.25 2.5-4.02z",
  high: "M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1-3.29-2.5-4.03v8.05c1.5-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z",
};

const volumePercent = computed(() => Math.round(volume.value * 100));
const likePath = computed(() => (isLiked.value ? likeIcons.liked : likeIcons.unliked));
const likeTitle = computed(() => (isLiked.value ? t.unlike : t.like));
const modePath = computed(() => modeIcons[playMode.value] ?? modeIcons.normal);
const modeTitle = computed(() => t[playMode.value]);
const volumeTitle = computed(() => `${t.volume} ${volumePercent.value}%`);
const volumePath = computed(() => {
  if (volume.value <= 0) return volumeIcons.muted;
  if (volume.value < 0.5) return volumeIcons.low;
  return volumeIcons.high;
});

function getTauri() {
  return window.__TAURI__;
}

function isWindowsTauri() {
  if (!getTauri()) return false;
  const platform = navigator.platform || "";
  return /Win/i.test(platform) || /Windows/i.test(ua);
}

function getTaskbarLyricsEnabled() {
  const raw = localStorage.getItem("settingData");
  if (!raw) return true;
  try {
    const parsed = JSON.parse(raw);
    return parsed.taskbarLyrics !== false;
  } catch {
    return true;
  }
}

function refreshTaskbarLyricsEntry() {
  showTaskbarLyricsEntry.value = isWindowsTauri() && getTaskbarLyricsEnabled();
}

function scheduleTrayPopupLayoutUpdate() {
  if (layoutFrame) cancelAnimationFrame(layoutFrame);
  layoutFrame = requestAnimationFrame(async () => {
    layoutFrame = 0;
    await nextTick();
    updateTrayPopupLayout();
  });
}

function updateTrayPopupLayout() {
  const el = popupRef.value;
  const tauri = getTauri();
  if (!el || !tauri) return;

  const width = Math.ceil(el.offsetWidth || 260);
  const height = Math.ceil(Math.max(el.offsetHeight, el.scrollHeight));
  tauri.core
    .invoke("update_tray_popup_layout", { width, height })
    .catch((error) => console.warn("[TrayPopup] Failed to update layout:", error));
}

function updateState(state: PlayerStatePayload) {
  title.value = state.title || "";
  artist.value = state.artist || "";
  coverUrl.value = state.coverUrl || "";
  isPlaying.value = Boolean(state.isPlaying);
  isLiked.value = Boolean(state.isLiked);
  if (typeof state.volume === "number") {
    volume.value = Math.max(0, Math.min(1, state.volume));
    if (volume.value > 0) previousVolume.value = volume.value;
  }
  if (state.playMode && state.playMode in modeIcons) {
    playMode.value = state.playMode;
  }
}

function emitVolume(nextVolume: number) {
  volume.value = Math.max(0, Math.min(1, nextVolume));
  if (volume.value > 0) previousVolume.value = volume.value;
  getTauri()?.event.emit("slave-volume", { volume: volume.value });
}

function handleVolumeUpdate(nextVolume: number) {
  emitVolume(nextVolume);
}

function toggleMute() {
  emitVolume(volume.value > 0 ? 0 : previousVolume.value || 0.7);
}

function playPause() {
  getTauri()?.event.emit("tray-play-pause", null);
}

function prevTrack() {
  getTauri()?.event.emit("tray-prev-track", null);
}

function nextTrack() {
  getTauri()?.event.emit("tray-next-track", null);
}

function toggleLike() {
  getTauri()?.event.emit("tray-like-song", null);
}

function cyclePlayMode() {
  getTauri()?.event.emit("tray-cycle-play-mode", null);
}

function showMainWindow() {
  getTauri()?.core.invoke("show_window", { label: "main" });
}

function openMiniPlayer() {
  getTauri()?.core.invoke("create_window", { label: "mini-player" });
}

function openDesktopLyrics() {
  getTauri()?.core.invoke("create_window", { label: "desktop-lyrics" });
}

function openTaskbarLyrics() {
  getTauri()?.core.invoke("plugin:taskbar-lyric|open_taskbar_lyric");
}

function openSettings() {
  getTauri()?.core.invoke("create_window", { label: "settings" });
}

function quitApp() {
  getTauri()?.core.invoke("quit_app");
}

function handleStorage(event: StorageEvent) {
  if (event.key && event.key !== "settingData") return;
  refreshTaskbarLyricsEntry();
  scheduleTrayPopupLayoutUpdate();
}

watch(showTaskbarLyricsEntry, () => {
  scheduleTrayPopupLayoutUpdate();
});

onMounted(async () => {
  document.addEventListener("contextmenu", preventDefault);
  document.addEventListener("keydown", preventRefresh);
  window.addEventListener("storage", handleStorage);
  refreshTaskbarLyricsEntry();
  scheduleTrayPopupLayoutUpdate();

  const tauri = getTauri();
  if (!tauri) return;

  unlisteners.push(
    await tauri.event.listen<PlayerStatePayload>("player-state-update", (event) => {
      updateState(event.payload || {});
    }),
  );
  unlisteners.push(
    await tauri.event.listen("tray-popup-opened", () => {
      refreshTaskbarLyricsEntry();
      scheduleTrayPopupLayoutUpdate();
    }),
  );
});

onBeforeUnmount(() => {
  if (layoutFrame) cancelAnimationFrame(layoutFrame);
  document.removeEventListener("contextmenu", preventDefault);
  document.removeEventListener("keydown", preventRefresh);
  window.removeEventListener("storage", handleStorage);
  unlisteners.forEach((unlisten) => unlisten());
});

function preventDefault(event: Event) {
  event.preventDefault();
}

function preventRefresh(event: KeyboardEvent) {
  if (event.key === "F5" || event.keyCode === 116) event.preventDefault();
}
</script>

<style lang="scss" scoped>
.tray-popup {
  width: 260px;
  height: auto;
  background: var(--popup-bg);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border: 1px solid var(--popup-border);
  color: var(--text-primary);
  font-family:
    "HarmonyOS_Regular",
    "Segoe UI",
    system-ui,
    -apple-system,
    sans-serif;
  display: flex;
  flex-direction: column;
  padding: 8px 0;
  overflow: hidden;

  --popup-bg: rgba(245, 245, 245, 0.92);
  --popup-border: rgba(0, 0, 0, 0.06);
  --text-primary: rgba(30, 30, 30, 0.88);
  --text-secondary: rgba(30, 30, 30, 0.5);
  --icon-color: rgba(30, 30, 30, 0.55);
  --divider: rgba(0, 0, 0, 0.06);
  --hover-bg: rgba(0, 0, 0, 0.04);
  --ctrl-hover-bg: rgba(0, 0, 0, 0.06);
  --ctrl-play-bg: rgba(0, 0, 0, 0.06);
  --ctrl-play-hover: rgba(0, 0, 0, 0.1);
  --cover-bg: rgba(0, 0, 0, 0.04);
  --quit-hover: rgba(220, 50, 50, 0.08);
  --quit-color: rgba(180, 40, 40, 0.75);
  --slider-track: rgba(0, 0, 0, 0.1);
  --slider-thumb: rgba(30, 30, 30, 0.78);

  &.native-effect {
    background: transparent;
    backdrop-filter: none;
    -webkit-backdrop-filter: none;
  }
}

@media (prefers-color-scheme: dark) {
  .tray-popup {
    --popup-bg: rgba(30, 30, 30, 0.92);
    --popup-border: rgba(255, 255, 255, 0.08);
    --text-primary: rgba(255, 255, 255, 0.9);
    --text-secondary: rgba(255, 255, 255, 0.5);
    --icon-color: rgba(255, 255, 255, 0.45);
    --divider: rgba(255, 255, 255, 0.06);
    --hover-bg: rgba(255, 255, 255, 0.06);
    --ctrl-hover-bg: rgba(255, 255, 255, 0.08);
    --ctrl-play-bg: rgba(255, 255, 255, 0.08);
    --ctrl-play-hover: rgba(255, 255, 255, 0.14);
    --cover-bg: rgba(255, 255, 255, 0.05);
    --quit-hover: rgba(255, 70, 70, 0.12);
    --quit-color: rgba(255, 100, 100, 0.85);
    --slider-track: rgba(255, 255, 255, 0.13);
    --slider-thumb: rgba(255, 255, 255, 0.86);
  }
}

.song-section,
.ctrl-btn,
.volume-icon,
.menu-item {
  border: none;
  font: inherit;
}

.song-section {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 46px;
  padding: 4px 14px 6px;
  background: transparent;
  color: inherit;
  cursor: pointer;
  transition: background 0.15s;

  &:hover {
    background: var(--hover-bg);

    .title {
      text-decoration: underline;
    }
  }
}

.cover {
  width: 36px;
  height: 36px;
  border-radius: 6px;
  object-fit: cover;
  flex-shrink: 0;
  background: var(--cover-bg);
}

.song-text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.title,
.artist {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  text-align: left;
}

.title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  line-height: 1.3;
}

.artist {
  min-height: 14px;
  font-size: 11px;
  color: var(--text-secondary);
  line-height: 1.3;
}

.divider {
  height: 1px;
  background: var(--divider);
  margin: 5px 12px;
  flex-shrink: 0;
}

.controls {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 4px 14px;
}

.ctrl-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  padding: 6px;
  border-radius: 50%;
  transition: all 0.15s ease;

  &:hover {
    background: var(--ctrl-hover-bg);
  }

  &:active {
    transform: scale(0.92);
  }

  &.play {
    background: var(--ctrl-play-bg);
    padding: 8px;

    &:hover {
      background: var(--ctrl-play-hover);
    }
  }

  &.liked {
    color: #ff4081;
  }
}

.icon-pause {
  display: none;
}

.is-playing {
  .icon-play {
    display: none;
  }

  .icon-pause {
    display: block;
  }
}

.volume-row {
  display: grid;
  grid-template-columns: 24px minmax(0, 1fr) 34px;
  align-items: center;
  gap: 8px;
  padding: 5px 14px 3px;
}

.volume-icon {
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: transparent;
  color: var(--icon-color);
  cursor: pointer;

  &:hover {
    background: var(--ctrl-hover-bg);
    color: var(--text-primary);
  }
}

.volume-slider {
  width: 100%;
  min-height: 24px;
  --bouncing-slider-icon-gap: 0;

  :deep(.inner) {
    border-radius: 999px;
    background-color: var(--slider-track);
  }

  :deep(.thumb) {
    background-color: var(--slider-thumb);
    opacity: 0.72;
  }
}

.volume-value {
  font-size: 11px;
  color: var(--text-secondary);
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.menu-section {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.menu-item {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 30px;
  padding: 0 14px;
  background: transparent;
  color: inherit;
  cursor: pointer;
  transition: background 0.15s;
  flex-shrink: 0;

  &:hover {
    background: var(--hover-bg);
  }

  .mi-icon {
    width: 18px;
    height: 18px;
    flex-shrink: 0;
    color: var(--icon-color);
  }

  span {
    min-width: 0;
    flex: 1;
    font-size: 13px;
    color: var(--text-primary);
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  &.taskbar {
    height: 36px;

    > span {
      display: flex;
      flex-direction: column;
      gap: 1px;
    }

    .menu-title {
      line-height: 15px;
    }

    .menu-subtitle {
      font-size: 10px;
      line-height: 12px;
      color: var(--text-secondary);
    }
  }

  &.quit:hover {
    background: var(--quit-hover);
  }

  &.quit .mi-icon,
  &.quit span {
    color: var(--quit-color);
  }
}
</style>
