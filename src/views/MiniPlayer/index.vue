<template>
  <div
    ref="containerRef"
    class="mini-player"
    :class="{ expanded }"
    :style="{ '--accent': state.accentColor ? `rgb(${state.accentColor})` : '#e74c3c' }"
    @contextmenu.prevent
  >
    <!-- Drag region -->
    <div class="drag-region" data-tauri-drag-region />
    <BackgroundRender
      class="bg"
      :playing="state.isPlaying"
      :album="state.coverUrlLarge || state.coverUrl || '/images/pic/default.png'"
    />

    <!-- Window controls -->
    <div class="window-controls">
      <button class="win-btn" @click="openMain" title="Open Main Window">
        <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor">
          <path
            d="M19 19H5V5h7V3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14c1.1 0 2-.9 2-2v-7h-2v7zM14 3v2h3.59l-9.83 9.83 1.41 1.41L19 6.41V10h2V3h-7z"
          />
        </svg>
      </button>
      <button class="win-btn" @click="toggleExpand">
        <svg v-if="!expanded" viewBox="0 0 24 24" width="14" height="14" fill="currentColor">
          <path d="M12 8l-6 6 1.41 1.41L12 10.83l4.59 4.58L18 14z" transform="rotate(180 12 12)" />
        </svg>
        <svg v-else viewBox="0 0 24 24" width="14" height="14" fill="currentColor">
          <path d="M12 8l-6 6 1.41 1.41L12 10.83l4.59 4.58L18 14z" />
        </svg>
      </button>
      <button class="win-btn close" @click="closeWindow">
        <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor">
          <path
            d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z"
          />
        </svg>
      </button>
    </div>

    <!-- Compact mode: cover + info + controls -->
    <div v-if="!expanded" class="compact-row" data-tauri-drag-region>
      <img class="cover" :src="state.coverUrl || '/images/pic/default.png'" alt="cover" />
      <div class="info" data-tauri-drag-region>
        <div class="title text-hidden">{{ state.title || "GMPlayer" }}</div>
        <div class="subtitle text-hidden">
          <template v-if="currentLyricLine && state.isPlaying">
            {{ currentLyricLine }}
          </template>
          <template v-else>
            {{ state.artist || "" }}
          </template>
        </div>
      </div>
      <div class="compact-controls">
        <button class="ctrl-btn win-action" @click="openMain" title="Open Main Window">
          <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor">
            <path d="M19 19H5V5h7V3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14c1.1 0 2-.9 2-2v-7h-2v7zM14 3v2h3.59l-9.83 9.83 1.41 1.41L19 6.41V10h2V3h-7z" />
          </svg>
        </button>
        <button class="ctrl-btn win-action" @click="toggleExpand">
          <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor">
            <path d="M12 8l-6 6 1.41 1.41L12 10.83l4.59 4.58L18 14z" transform="rotate(180 12 12)" />
          </svg>
        </button>
        <span class="compact-divider" />
        <button class="ctrl-btn" @click="bridge.prevTrack()">
          <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor">
            <path d="M6 6h2v12H6zm3.5 6l8.5 6V6z" />
          </svg>
        </button>
        <button class="ctrl-btn play" @click="bridge.playPause()">
          <svg v-if="state.isPlaying" viewBox="0 0 24 24" width="22" height="22" fill="currentColor">
            <path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z" />
          </svg>
          <svg v-else viewBox="0 0 24 24" width="22" height="22" fill="currentColor">
            <path d="M8 5v14l11-7z" />
          </svg>
        </button>
        <button class="ctrl-btn" @click="bridge.nextTrack()">
          <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor">
            <path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z" />
          </svg>
        </button>
        <button class="ctrl-btn win-action close" @click="closeWindow">
          <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor">
            <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z" />
          </svg>
        </button>
      </div>
    </div>

    <!-- Expanded mode: two-layer design -->
    <Transition name="expand-fade">
      <div
        v-if="expanded"
        class="expanded-container"
        :class="{ 'layer2-active': activeLayer === 2 }"
      >
        <!-- Animated cover frame (absolute, z-60) -->
        <MiniCoverFrame
          :visible="true"
          :frame-style="coverFrameStyle"
          :cover-url="state.coverUrlLarge || state.coverUrl || '/images/pic/default.png'"
          @click="switchLayer"
        />

        <!-- Layer 1: Cover View -->
        <div class="layer layer-cover">
          <!-- Phony big cover placeholder -->
          <div ref="phonyBigCoverRef" class="phony-big-cover" />

          <div class="cover-info">
            <div class="song-title text-hidden">{{ state.title || "GMPlayer" }}</div>
            <div class="song-artist text-hidden">{{ state.artist || "" }}</div>
            <button
              class="like-btn"
              :class="{ liked: state.isLiked }"
              @click="bridge.toggleLike()"
            >
              <svg v-if="state.isLiked" viewBox="0 0 24 24" width="16" height="16" fill="currentColor">
                <path d="M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z" />
              </svg>
              <svg v-else viewBox="0 0 24 24" width="16" height="16" fill="currentColor">
                <path d="M16.5 3c-1.74 0-3.41.81-4.5 2.09C10.91 3.81 9.24 3 7.5 3 4.42 3 2 5.42 2 8.5c0 3.78 3.4 6.86 8.55 11.54L12 21.35l1.45-1.32C18.6 15.36 22 12.28 22 8.5 22 5.42 19.58 3 16.5 3zm-4.4 15.55l-.1.1-.1-.1C7.14 14.24 4 11.39 4 8.5 4 6.5 5.5 5 7.5 5c1.54 0 3.04.99 3.57 2.36h1.87C13.46 5.99 14.96 5 16.5 5c2 0 3.5 1.5 3.5 3.5 0 2.89-3.14 5.74-7.9 10.05z" />
              </svg>
            </button>
          </div>

          <div class="layer-progress">
            <BouncingSlider
              :value="bridge.currentTime.value || 0"
              :min="0"
              :max="state.duration || 1"
              :is-playing="state.isPlaying"
              @update:value="handleSeek"
              @seek-start="seeking = true"
              @seek-end="seeking = false"
            />
            <div class="time-display">
              <span>{{ formatTime(bridge.currentTime.value) }}</span>
              <span>{{ formatTime(state.duration) }}</span>
            </div>
          </div>

          <div class="layer-controls">
            <button class="ctrl-btn small" @click="bridge.cyclePlayMode()" :title="state.playMode">
              <svg
                v-if="state.playMode === 'normal'"
                viewBox="0 0 24 24"
                width="16"
                height="16"
                fill="currentColor"
              >
                <path d="M7 7h10v3l4-4-4-4v3H5v6h2V7zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4z" />
              </svg>
              <svg
                v-else-if="state.playMode === 'random'"
                viewBox="0 0 24 24"
                width="16"
                height="16"
                fill="currentColor"
              >
                <path
                  d="M10.59 9.17L5.41 4 4 5.41l5.17 5.17 1.42-1.41zM14.5 4l2.04 2.04L4 18.59 5.41 20 17.96 7.46 20 9.5V4h-5.5zm.33 9.41l-1.41 1.41 3.13 3.13L14.5 20H20v-5.5l-2.04 2.04-3.13-3.13z"
                />
              </svg>
              <svg v-else viewBox="0 0 24 24" width="16" height="16" fill="currentColor">
                <path
                  d="M7 7h10v3l4-4-4-4v3H5v6h2V7zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4zm-4-2V9h-1l-2 1v1h1.5v4H13z"
                />
              </svg>
            </button>
            <button class="ctrl-btn" @click="bridge.prevTrack()">
              <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor">
                <path d="M6 6h2v12H6zm3.5 6l8.5 6V6z" />
              </svg>
            </button>
            <button class="ctrl-btn play-main" @click="bridge.playPause()">
              <svg
                v-if="state.isPlaying"
                viewBox="0 0 24 24"
                width="28"
                height="28"
                fill="currentColor"
              >
                <path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z" />
              </svg>
              <svg v-else viewBox="0 0 24 24" width="28" height="28" fill="currentColor">
                <path d="M8 5v14l11-7z" />
              </svg>
            </button>
            <button class="ctrl-btn" @click="bridge.nextTrack()">
              <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor">
                <path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z" />
              </svg>
            </button>
            <div class="volume-control">
              <button class="ctrl-btn small" @click="toggleMute">
                <svg
                  v-if="state.volume === 0"
                  viewBox="0 0 24 24"
                  width="16"
                  height="16"
                  fill="currentColor"
                >
                  <path
                    d="M16.5 12c0-1.77-1.02-3.29-2.5-4.03v2.21l2.45 2.45c.03-.2.05-.41.05-.63zm2.5 0c0 .94-.2 1.82-.54 2.64l1.51 1.51C20.63 14.91 21 13.5 21 12c0-4.28-2.99-7.86-7-8.77v2.06c2.89.86 5 3.54 5 6.71zM4.27 3L3 4.27 7.73 9H3v6h4l5 5v-6.73l4.25 4.25c-.67.52-1.42.93-2.25 1.18v2.06c1.38-.31 2.63-.95 3.69-1.81L19.73 21 21 19.73l-9-9L4.27 3zM12 4L9.91 6.09 12 8.18V4z"
                  />
                </svg>
                <svg v-else viewBox="0 0 24 24" width="16" height="16" fill="currentColor">
                  <path
                    d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z"
                  />
                </svg>
              </button>
              <BouncingSlider
                class="volume-slider"
                :value="state.volume"
                :min="0"
                :max="1"
                :change-on-drag="true"
                @update:value="handleVolumeChange"
              />
            </div>
          </div>
        </div>

        <!-- Layer 2: Lyrics View -->
        <div class="layer layer-lyrics">
          <!-- Top bar: small cover placeholder + song info + controls -->
          <div class="lyrics-top-bar">
            <div ref="phonySmallCoverRef" class="phony-small-cover" />
            <div class="lyrics-top-info">
              <div class="song-title text-hidden">{{ state.title || "GMPlayer" }}</div>
              <div class="song-artist text-hidden">{{ state.artist || "" }}</div>
            </div>
            <div class="lyrics-top-controls">
              <button class="ctrl-btn small" @click="bridge.prevTrack()">
                <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor">
                  <path d="M6 6h2v12H6zm3.5 6l8.5 6V6z" />
                </svg>
              </button>
              <button class="ctrl-btn small" @click="bridge.playPause()">
                <svg
                  v-if="state.isPlaying"
                  viewBox="0 0 24 24"
                  width="18"
                  height="18"
                  fill="currentColor"
                >
                  <path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z" />
                </svg>
                <svg v-else viewBox="0 0 24 24" width="18" height="18" fill="currentColor">
                  <path d="M8 5v14l11-7z" />
                </svg>
              </button>
              <button class="ctrl-btn small" @click="bridge.nextTrack()">
                <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor">
                  <path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z" />
                </svg>
              </button>
            </div>
          </div>

          <!-- AMLL Lyric Player -->
          <div class="lyrics-area">
            <LyricPlayer
              v-if="hasAmllLyrics"
              class="amll-lyric-player"
              :lyric-lines="amllLines"
              :current-time="adjustedTime"
              :playing="state.isPlaying"
              :enable-blur="bridge.settings.lyricsBlur"
              :enable-spring="bridge.settings.showYrcAnimation"
              :enable-scale="bridge.settings.showYrcAnimation"
              :word-fade-width="0.5"
              align-anchor="center"
              :align-position="0.5"
              :line-pos-x-spring-params="bridge.settings.springParams.posX"
              :line-pos-y-spring-params="bridge.settings.springParams.posY"
              :line-scale-spring-params="bridge.settings.springParams.scale"
              :enable-interlude-dots="true"
              @line-click="handleLineClick"
              :style="amllStyles"
              :key="playerKey"
              ref="amllPlayerRef"
            />
            <div v-else class="no-lyrics-hint">
              <span>{{ state.artist || "" }}</span>
            </div>
          </div>

          <!-- Bottom progress -->
          <div class="lyrics-bottom-progress">
            <BouncingSlider
              :value="bridge.currentTime.value || 0"
              :min="0"
              :max="state.duration || 1"
              :is-playing="state.isPlaying"
              @update:value="handleSeek"
              @seek-start="seeking = true"
              @seek-end="seeking = false"
            />
            <div class="time-display">
              <span>{{ formatTime(bridge.currentTime.value) }}</span>
              <span>{{ formatTime(state.duration) }}</span>
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, shallowRef, nextTick, onMounted, onUnmounted } from "vue";
import { usePlayerBridge } from "@/utils/tauri/playerBridge";
import { windowManager } from "@/utils/tauri/windowManager";
import { useMiniCoverFrame } from "./useMiniCoverFrame";
import MiniCoverFrame from "./MiniCoverFrame.vue";
import BouncingSlider from "@/components/Player/BouncingSlider.vue";
import BackgroundRender from "@/libs/apple-music-like/BackgroundRender.vue";
import { LyricPlayer, LyricPlayerRef } from "@applemusic-like-lyrics/vue";
import type { AMLLLine } from "@/utils/LyricsProcessor";
import "@applemusic-like-lyrics/core/style.css";

const bridge = usePlayerBridge();
const { state } = bridge;

const expanded = ref(false);
const activeLayer = ref(1); // 1 = cover view, 2 = lyrics view
const seeking = ref(false);
const previousVolume = ref(0.7);

// ── AMLL State ───────────────────────────────────────────────────────

const playerKey = ref(Symbol());
const amllPlayerRef = ref<LyricPlayerRef>();
const amllLines = shallowRef<AMLLLine[]>([]);

const handleLineClick = (evt: any) => {
  const targetTime = evt.line.getLine().startTime;
  amllPlayerRef.value?.lyricPlayer.value?.setCurrentTime(targetTime, true);
  amllPlayerRef.value?.lyricPlayer.value?.update();
  bridge.seek(targetTime / 1000);
};

const hasAmllLyrics = computed(() => amllLines.value && amllLines.value.length > 0);

watch(
  () => bridge.lyricData.value,
  (data) => {
    if (data?.amllLines && data.amllLines.length > 0) {
      amllLines.value = data.amllLines;
      playerKey.value = Symbol();
    } else {
      amllLines.value = [];
    }
  },
  { immediate: true },
);

const adjustedTime = computed(() => {
  return bridge.currentTime.value * 1000 + (bridge.settings.lyricTimeOffset ?? 0);
});

const amllStyles = computed(() => ({
  "--amll-lp-color": state.accentColor ? `rgb(${state.accentColor})` : "rgb(239, 239, 239)",
  "--amll-lyric-view-color": state.accentColor ? `rgb(${state.accentColor})` : "rgb(239, 239, 239)",
  "--amll-lp-font-size": "22px",
  "font-weight": bridge.settings.lyricFontWeight || "bold",
  "font-family": bridge.settings.lyricFont || "HarmonyOS Sans SC",
  "letter-spacing": bridge.settings.lyricLetterSpacing || "normal",
  "font-size": "22px",
}));

// Play state sync for AMLL
watch(
  () => state.isPlaying,
  (playing) => {
    if (playing) {
      amllPlayerRef.value?.lyricPlayer.value?.resume();
    } else {
      amllPlayerRef.value?.lyricPlayer.value?.pause();
    }
    amllPlayerRef.value?.lyricPlayer.value?.update();
  },
);

// ── Cover Frame (Phony Cover Pattern) ───────────────────────────────

const containerRef = ref<HTMLElement | null>(null);
const phonyBigCoverRef = ref<HTMLElement | null>(null);
const phonySmallCoverRef = ref<HTMLElement | null>(null);

const { coverFrameStyle, updateCoverStyle, setupResizeObserver, cleanupResizeObserver } =
  useMiniCoverFrame(containerRef, phonyBigCoverRef, phonySmallCoverRef, activeLayer);

// Update cover position when layer switches
watch(activeLayer, () => {
  nextTick(updateCoverStyle);
});

// ── Current lyric line (compact mode) ────────────────────────────────

const currentLyricLine = computed(() => {
  if (!bridge.lyricData.value?.lrc?.length) return "";
  const idx = bridge.lyricIndex.value;
  if (idx < 0 || idx >= bridge.lyricData.value.lrc.length) return "";
  return bridge.lyricData.value.lrc[idx]?.content || "";
});

// ── Playback Controls ────────────────────────────────────────────────

function handleSeek(val: number) {
  bridge.seek(val);
}

function handleVolumeChange(val: number) {
  bridge.setVolume(val);
}

function toggleMute() {
  if (state.volume > 0) {
    previousVolume.value = state.volume;
    bridge.setVolume(0);
  } else {
    bridge.setVolume(previousVolume.value);
  }
}

function switchLayer() {
  activeLayer.value = activeLayer.value === 1 ? 2 : 1;
}

// ── Window controls ──────────────────────────────────────────────────

async function toggleExpand() {
  expanded.value = !expanded.value;
  try {
    if (expanded.value) {
      await windowManager.resizeWindow("mini-player", 350, 520);
      // Set up cover frame after expand transition
      nextTick(() => {
        setupResizeObserver();
      });
    } else {
      cleanupResizeObserver();
      activeLayer.value = 1;
      await windowManager.resizeWindow("mini-player", 350, 80);
    }
  } catch (err) {
    console.warn("[MiniPlayer] Could not resize window:", err);
  }
}

function closeWindow() {
  cleanupResizeObserver();
  windowManager.closeWindow("mini-player");
}

function openMain() {
  windowManager.showWindow("main");
  windowManager.focusWindow("main");
}

// ── Utilities ────────────────────────────────────────────────────────

function formatTime(seconds: number): string {
  if (!seconds || seconds < 0) return "0:00";
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

// ── Lifecycle ────────────────────────────────────────────────────────

onUnmounted(() => {
  cleanupResizeObserver();
});
</script>

<style lang="scss" scoped>
.text-hidden {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.bg {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  overflow: hidden;
  z-index: -2;
  transition: filter 0.5s ease;
  will-change: filter, opacity;
}

.mini-player {
  width: 100%;
  height: 100%;
  background: rgba(30, 30, 30, 0.66);
  backdrop-filter: blur(24px);
  -webkit-backdrop-filter: blur(24px);
  color: #fff;
  font-family:
    "Segoe UI",
    system-ui,
    -apple-system,
    sans-serif;
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
  position: relative;
  overflow: hidden;
  user-select: none;
}

.drag-region {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 30px;
  z-index: 10;
}

.window-controls {
  position: absolute;
  top: 4px;
  right: 4px;
  display: flex;
  gap: 2px;
  z-index: 20;

  .win-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background: none;
    border: none;
    color: rgba(255, 255, 255, 0.4);
    cursor: pointer;
    border-radius: 6px;
    transition: all 0.15s ease;

    &:hover {
      background: rgba(255, 255, 255, 0.1);
      color: rgba(255, 255, 255, 0.8);
    }

    &.close:hover {
      background: rgba(232, 65, 66, 0.8);
      color: #fff;
    }
  }
}

// Hide absolute window-controls in compact mode
.mini-player:not(.expanded) {
  .window-controls {
    display: none;
  }
}

// ── Compact row ──────────────────────────────────────────────────────

.compact-row {
  display: flex;
  align-items: center;
  padding: 10px;
  gap: 10px;
  height: 76px;
  box-sizing: border-box;
  position: relative;
  z-index: 15;

  .cover {
    width: 52px;
    height: 52px;
    border-radius: 8px;
    object-fit: cover;
    flex-shrink: 0;
    background: rgba(255, 255, 255, 0.05);
  }

  .info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;

    .title {
      font-size: 13px;
      font-weight: 600;
      line-height: 1.3;
    }

    .subtitle {
      font-size: 11px;
      color: rgba(255, 255, 255, 0.55);
      line-height: 1.3;
    }
  }

  .compact-controls {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;

    .compact-divider {
      width: 1px;
      height: 14px;
      background: rgba(255, 255, 255, 0.15);
      margin: 0 3px;
    }

    .win-action {
      color: rgba(255, 255, 255, 0.4);
      padding: 5px;

      &:hover {
        background: rgba(255, 255, 255, 0.1);
        color: rgba(255, 255, 255, 0.8);
      }

      &.close:hover {
        background: rgba(232, 65, 66, 0.8);
        color: #fff;
      }
    }
  }
}

// ── Shared button styles ─────────────────────────────────────────────

.ctrl-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  background: none;
  border: none;
  color: rgba(255, 255, 255, 0.8);
  cursor: pointer;
  padding: 6px;
  border-radius: 50%;
  transition: all 0.15s ease;

  &:hover {
    background: rgba(255, 255, 255, 0.1);
    color: #fff;
  }

  &:active {
    transform: scale(0.92);
  }

  &.play {
    background: rgba(255, 255, 255, 0.12);

    &:hover {
      background: rgba(255, 255, 255, 0.2);
    }
  }

  &.play-main {
    background: var(--accent, #e74c3c);
    padding: 8px;

    &:hover {
      filter: brightness(1.15);
    }
  }

  &.small {
    padding: 4px;
  }
}

// ── Expand transition ────────────────────────────────────────────────

.expand-fade-enter-active,
.expand-fade-leave-active {
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}

.expand-fade-enter-from,
.expand-fade-leave-to {
  opacity: 0;
  transform: translateY(-10px);
}

// ── Expanded container ───────────────────────────────────────────────

.expanded-container {
  flex: 1;
  position: relative;
  overflow: hidden;
}

// ── Layers ───────────────────────────────────────────────────────────

.layer {
  position: absolute;
  top: 32px;
  left: 0;
  right: 0;
  bottom: 0;
  display: flex;
  flex-direction: column;
  transition: opacity 0.35s ease;
}

.layer-cover {
  padding: 0 16px 14px;
  justify-content: center;
  align-items: center;
  opacity: 1;
  pointer-events: auto;
  overflow-y: auto;

  .phony-big-cover {
    width: 100%;
    aspect-ratio: 1;
    max-width: 180px;
    flex-shrink: 0;
  }

  .cover-info {
    width: 100%;
    text-align: center;
    padding: 12px 0 4px;

    .song-title {
      font-size: 14px;
      font-weight: 600;
      line-height: 1.4;
    }

    .song-artist {
      font-size: 12px;
      color: rgba(255, 255, 255, 0.55);
      margin-top: 2px;
      line-height: 1.3;
    }
  }

  .like-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    color: rgba(255, 255, 255, 0.45);
    cursor: pointer;
    padding: 4px;
    border-radius: 50%;
    transition: all 0.2s ease;
    margin-top: 4px;

    &:hover {
      color: rgba(255, 255, 255, 0.8);
      transform: scale(1.15);
    }

    &:active {
      transform: scale(0.9);
    }

    &.liked {
      color: #ff4081;

      &:hover {
        color: #ff4081;
      }
    }
  }

  .layer-progress {
    padding: 4px 0;
    width: 90%;

    .time-display {
      display: flex;
      justify-content: space-between;
      font-size: 10px;
      color: rgba(255, 255, 255, 0.4);
      margin-top: 2px;
    }
  }

  .layer-controls {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding-top: 2px;
    padding-right: 4px;

    .volume-control {
      display: flex;
      align-items: center;
      gap: 4px;

      .volume-slider {
        width: 90px;
      }
    }
  }
}

.layer-lyrics {
  padding: 0 12px 10px;
  opacity: 0;
  pointer-events: none;

  .lyrics-top-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 4px 0;
    height: 56px;
    flex-shrink: 0;

    .phony-small-cover {
      width: 48px;
      height: 48px;
      flex-shrink: 0;
    }

    .lyrics-top-info {
      flex: 1;
      min-width: 0;

      .song-title {
        font-size: 13px;
        font-weight: 600;
        line-height: 1.3;
      }

      .song-artist {
        font-size: 11px;
        color: rgba(255, 255, 255, 0.55);
        margin-top: 2px;
        line-height: 1.3;
      }
    }

    .lyrics-top-controls {
      display: flex;
      align-items: center;
      gap: 2px;
      flex-shrink: 0;
    }
  }

  .lyrics-area {
    flex: 1;
    overflow: hidden;
    position: relative;
    margin: 4px 0;

    .amll-lyric-player {
      width: 100%;
      height: 100%;

      &.dom:deep(span[class^="_emphasizeWrapper"] span) {
        padding: 0.5em;
        margin: -0.5em;
      }
    }

    .no-lyrics-hint {
      width: 100%;
      height: 100%;
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 13px;
      color: rgba(255, 255, 255, 0.35);
    }
  }

  .lyrics-bottom-progress {
    flex-shrink: 0;
    padding: 0 4px;

    .time-display {
      display: flex;
      justify-content: space-between;
      font-size: 10px;
      color: rgba(255, 255, 255, 0.4);
      margin-top: 2px;
    }
  }
}

// ── Layer switching via parent class ────────────────────────────────

.layer2-active {
  .layer-cover {
    opacity: 0;
    pointer-events: none;
  }

  .layer-lyrics {
    opacity: 1;
    pointer-events: auto;
  }
}
</style>
