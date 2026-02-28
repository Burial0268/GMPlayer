<template>
  <div
    class="desktop-lyric"
    :class="{ locked: isLocked, hovering: isHovering && !isLocked }"
    :data-tauri-drag-region="!isLocked || undefined"
    @mousemove="onMouseMove"
    @mouseenter="onMouseEnter"
    @mouseleave="onMouseLeave"
    @contextmenu.prevent
  >
    <!-- Header bar -->
    <Transition name="header-fade">
      <div
        v-if="showHeader"
        class="header"
        :data-tauri-drag-region="!isLocked || undefined"
        @mouseenter="isHeaderHovering = true"
        @mouseleave="onHeaderLeave"
      >
        <template v-if="!isLocked">
          <div class="header-left">
            <span class="song-name" :title="state.title" :data-tauri-drag-region="!isLocked || undefined">
              {{ state.title || $t("desktopLyrics.noLyrics") }}
            </span>
          </div>
          <div class="header-center">
            <button
              class="ctrl-btn"
              @pointerdown.stop
              @click="bridge.prevTrack()"
              :title="$t('desktopLyrics.prev')"
            >
              <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor">
                <path d="M6 6h2v12H6zm3.5 6l8.5 6V6z" />
              </svg>
            </button>
            <button
              class="ctrl-btn play-btn"
              @pointerdown.stop
              @click="bridge.playPause()"
              :title="$t('desktopLyrics.playPause')"
            >
              <svg
                v-if="state.isPlaying"
                viewBox="0 0 24 24"
                width="22"
                height="22"
                fill="currentColor"
              >
                <path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z" />
              </svg>
              <svg v-else viewBox="0 0 24 24" width="22" height="22" fill="currentColor">
                <path d="M8 5v14l11-7z" />
              </svg>
            </button>
            <button
              class="ctrl-btn"
              @pointerdown.stop
              @click="bridge.nextTrack()"
              :title="$t('desktopLyrics.next')"
            >
              <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor">
                <path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z" />
              </svg>
            </button>
          </div>
          <div class="header-right">
            <button
              class="ctrl-btn font-btn"
              @pointerdown.stop
              @click="decreaseFontSize"
              :title="$t('desktopLyrics.smaller')"
            >
              A<span class="font-sign">-</span>
            </button>
            <button
              class="ctrl-btn font-btn"
              @pointerdown.stop
              @click="increaseFontSize"
              :title="$t('desktopLyrics.larger')"
            >
              A<span class="font-sign">+</span>
            </button>
            <button
              class="ctrl-btn"
              @pointerdown.stop
              @click="toggleLock"
              :title="$t('desktopLyrics.lock')"
            >
              <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor">
                <path
                  d="M18 8h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2zm-6 9c-1.1 0-2-.9-2-2s.9-2 2-2 2 .9 2 2-.9 2-2 2zm3.1-9H8.9V6c0-1.71 1.39-3.1 3.1-3.1s3.1 1.39 3.1 3.1v2z"
                />
              </svg>
            </button>
            <button
              class="ctrl-btn"
              @pointerdown.stop
              @click="handleClose"
              :title="$t('desktopLyrics.close')"
            >
              <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor">
                <path
                  d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z"
                />
              </svg>
            </button>
          </div>
        </template>
        <template v-else>
          <!-- Locked mode: show unlock button -->
          <div class="header-center locked-header">
            <svg
              viewBox="0 0 24 24"
              width="14"
              height="14"
              fill="currentColor"
              style="opacity: 0.7"
            >
              <path
                d="M18 8h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2zm-6 9c-1.1 0-2-.9-2-2s.9-2 2-2 2 .9 2 2-.9 2-2 2zm3.1-9H8.9V6c0-1.71 1.39-3.1 3.1-3.1s3.1 1.39 3.1 3.1v2z"
              />
            </svg>
            <span class="locked-label">{{ $t("desktopLyrics.locked") }}</span>
            <button class="ctrl-btn unlock-btn" @click="handleUnlock">
              <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor">
                <path
                  d="M12 17c1.1 0 2-.9 2-2s-.9-2-2-2-2 .9-2 2 .9 2 2 2zm6-9h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6h1.9c0-1.71 1.39-3.1 3.1-3.1s3.1 1.39 3.1 3.1v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2zm0 12H6V10h12v10z"
                />
              </svg>
              {{ $t("desktopLyrics.unlock") }}
            </button>
          </div>
        </template>
      </div>
    </Transition>

    <!-- Lyric content -->
    <div class="lyric-area">
      <TransitionGroup v-if="hasLyrics" name="lyric-slide" tag="div" class="lyric-container">
        <div
          v-for="line in visibleLines"
          :key="line.key"
          class="lyric-line"
          :class="{ current: line.isCurrent }"
          @click="seekToLine(line)"
        >
          <div class="lyric-inner" :style="lyricTextStyle">
            <template v-if="line.isCurrent && line.words.length > 1">
              <span
                v-for="(word, wi) in line.words"
                :key="wi"
                class="lyric-word"
                :style="getWordStyle(word)"
                >{{ word.word }}</span
              >
            </template>
            <span v-else class="lyric-text" :class="{ current: line.isCurrent }">{{
              line.text
            }}</span>
          </div>
          <div
            v-if="line.translatedLyric && bridge.settings.showTransl"
            class="lyric-tran"
            :style="tranStyle"
          >
            {{ line.translatedLyric }}
          </div>
        </div>
      </TransitionGroup>
      <div v-else class="no-lyrics" :style="lyricTextStyle">
        <span class="song-title">{{ state.title || $t("desktopLyrics.noLyrics") }}</span>
        <span v-if="state.artist" class="artist-name">{{ state.artist }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, shallowRef, onMounted, onUnmounted } from "vue";
import { usePlayerBridge } from "@/utils/tauri/playerBridge";
import { windowManager } from "@/utils/tauri/windowManager";
import type { AMLLLine, AMLLWord } from "@/utils/LyricsProcessor";

const bridge = usePlayerBridge();
const { state } = bridge;

// ── Lyric Data ────────────────────────────────────────────────────────

const amllLines = shallowRef<AMLLLine[]>([]);
const songGeneration = ref(0);

const hasLyrics = computed(() => amllLines.value && amllLines.value.length > 0);

watch(
  () => bridge.lyricData.value,
  (data) => {
    if (data?.amllLines && data.amllLines.length > 0) {
      amllLines.value = data.amllLines;
    } else {
      amllLines.value = [];
    }
    songGeneration.value++;
  },
  { immediate: true },
);

// ── Time Interpolation (RAF + performance.now anchor) ─────────────────

const interpolatedTimeMs = ref(0);
let timeAnchorMs = 0;
let perfAnchor = 0;
let rafId = 0;

watch(
  () => bridge.currentTime.value,
  (sec) => {
    const bridgeMs = sec * 1000 + (bridge.settings.lyricTimeOffset ?? 0);
    const now = performance.now();

    if (!state.isPlaying) {
      // When paused, snap directly to bridge time
      interpolatedTimeMs.value = bridgeMs;
      timeAnchorMs = bridgeMs;
      perfAnchor = now;
      return;
    }

    const predicted = timeAnchorMs + (now - perfAnchor);
    if (Math.abs(bridgeMs - predicted) > 300) {
      timeAnchorMs = bridgeMs;
      perfAnchor = now;
    }
  },
);

function tick() {
  if (state.isPlaying) {
    interpolatedTimeMs.value = timeAnchorMs + (performance.now() - perfAnchor);
  }
  rafId = requestAnimationFrame(tick);
}

// ── Lyric Index (binary search) ───────────────────────────────────────

const currentLineIndex = computed(() => {
  const timeMs = interpolatedTimeMs.value;
  const lines = amllLines.value;
  if (!lines || lines.length === 0) return -1;
  // Find the last line with startTime <= timeMs
  let lo = 0;
  let hi = lines.length - 1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (lines[mid].startTime <= timeMs) lo = mid + 1;
    else hi = mid - 1;
  }
  return hi;
});

// ── Visible Lines ─────────────────────────────────────────────────────

interface VisibleLine {
  key: string;
  isCurrent: boolean;
  words: AMLLWord[];
  text: string;
  translatedLyric: string;
  lineStartTime: number;
  lineEndTime: number;
}

function clamp(v: number, min: number, max: number) {
  return Math.max(min, Math.min(max, v));
}

const visibleLines = computed<VisibleLine[]>(() => {
  const lines = amllLines.value;
  if (!lines || lines.length === 0) return [];
  const idx = currentLineIndex.value;
  const gen = songGeneration.value;
  const result: VisibleLine[] = [];

  if (idx >= 0 && idx < lines.length) {
    const line = lines[idx];
    result.push({
      key: `${gen}-${idx}`,
      isCurrent: true,
      words: line.words,
      text: line.words.map((w) => w.word).join(""),
      translatedLyric: line.translatedLyric || "",
      lineStartTime: line.startTime,
      lineEndTime: line.endTime,
    });
  }

  const nextIdx = Math.max(0, idx + 1);
  if (nextIdx < lines.length && result.length < 2) {
    const line = lines[nextIdx];
    result.push({
      key: `${gen}-${nextIdx}`,
      isCurrent: false,
      words: line.words,
      text: line.words.map((w) => w.word).join(""),
      translatedLyric: line.translatedLyric || "",
      lineStartTime: line.startTime,
      lineEndTime: line.endTime,
    });
  }

  return result;
});

// ── YRC Word Gradient Style ───────────────────────────────────────────

function getWordStyle(word: AMLLWord) {
  const duration = word.endTime - word.startTime;
  if (duration <= 0) return {};
  const progress = clamp((interpolatedTimeMs.value - word.startTime) / duration, 0, 1);
  return {
    backgroundPosition: `${(1 - progress) * 100}% 0`,
  };
}

// ── Font Size ─────────────────────────────────────────────────────────

const fontSizeOffset = ref(0);
const windowHeight = ref(120);

const localFontSize = computed(() => {
  const base = clamp(20 + (windowHeight.value - 100) * 0.3, 20, 80);
  return clamp(Math.round(base + fontSizeOffset.value), 16, 96);
});

function increaseFontSize() {
  fontSizeOffset.value = Math.min(fontSizeOffset.value + 4, 40);
}

function decreaseFontSize() {
  fontSizeOffset.value = Math.max(fontSizeOffset.value - 4, -20);
}

// ── Computed Styles ───────────────────────────────────────────────────

const accentColor = computed(() =>
  state.accentColor ? `rgb(${state.accentColor})` : "rgb(255, 255, 255)",
);

const inactiveColor = computed(() =>
  state.accentColor ? `rgba(${state.accentColor}, 0.35)` : "rgba(255, 255, 255, 0.35)",
);

const lyricTextStyle = computed(() => ({
  fontSize: `${localFontSize.value}px`,
  fontWeight: bridge.settings.lyricFontWeight || "bold",
  fontFamily: bridge.settings.lyricFont || "HarmonyOS Sans SC",
  letterSpacing: bridge.settings.lyricLetterSpacing || "normal",
  "--active-color": accentColor.value,
  "--inactive-color": inactiveColor.value,
}));

const tranStyle = computed(() => ({
  fontSize: `${Math.max(14, Math.round(localFontSize.value * 0.45))}px`,
}));

// ── Drag Attrs (disabled when locked) ─────────────────────────────────

const dragAttrs = computed(() => (!isLocked.value ? { "data-tauri-drag-region": "" } : {}));

// ── Header Visibility ─────────────────────────────────────────────────

const isHovering = ref(false);
const showHeader = ref(false);
const isHeaderHovering = ref(false);
let headerTimeout: ReturnType<typeof setTimeout> | null = null;

function clearHeaderTimeout() {
  if (headerTimeout) {
    clearTimeout(headerTimeout);
    headerTimeout = null;
  }
}

function scheduleHideHeader() {
  clearHeaderTimeout();
  headerTimeout = setTimeout(() => {
    if (!isHeaderHovering.value) {
      showHeader.value = false;
      isHovering.value = false;
    }
  }, 1500);
}

function onMouseEnter() {
  if (isLocked.value) return;
  isHovering.value = true;
  showHeader.value = true;
  clearHeaderTimeout();
}

function onMouseMove() {
  if (isLocked.value) return;
  if (!showHeader.value) {
    showHeader.value = true;
    isHovering.value = true;
  }
  scheduleHideHeader();
}

function onMouseLeave() {
  if (isLocked.value) return;
  isHovering.value = false;
  scheduleHideHeader();
}

function onHeaderLeave() {
  isHeaderHovering.value = false;
  if (!isLocked.value) {
    scheduleHideHeader();
  } else if (isTempUnlocked.value) {
    scheduleReLock();
  }
}

// ── Lock Mechanism ────────────────────────────────────────────────────

const isLocked = ref(false);
const isTempUnlocked = ref(false);
let cursorPollInterval: ReturnType<typeof setInterval> | null = null;
let reLockTimeout: ReturnType<typeof setTimeout> | null = null;

function scheduleReLock() {
  if (reLockTimeout) clearTimeout(reLockTimeout);
  reLockTimeout = setTimeout(async () => {
    if (isLocked.value && isTempUnlocked.value && !isHeaderHovering.value) {
      isTempUnlocked.value = false;
      showHeader.value = false;
      await windowManager.setIgnoreCursorEvents("desktop-lyrics", true);
    }
  }, 500);
}

async function toggleLock() {
  isLocked.value = true;
  showHeader.value = false;
  isHovering.value = false;
  clearHeaderTimeout();
  await windowManager.setIgnoreCursorEvents("desktop-lyrics", true);
  startCursorPolling();
}

async function handleUnlock() {
  isLocked.value = false;
  isTempUnlocked.value = false;
  stopCursorPolling();
  if (reLockTimeout) {
    clearTimeout(reLockTimeout);
    reLockTimeout = null;
  }
  await windowManager.setIgnoreCursorEvents("desktop-lyrics", false);
}

function startCursorPolling() {
  stopCursorPolling();
  cursorPollInterval = setInterval(async () => {
    if (!isLocked.value) return;

    const cursor = await windowManager.getCursorPosition();
    const bounds = await windowManager.getWindowBounds("desktop-lyrics");
    if (!cursor || !bounds) return;

    const [cx, cy] = cursor;
    const [wx, wy, ww, wh] = bounds;
    const margin = 30;

    const isNear =
      cx >= wx - margin && cx <= wx + ww + margin && cy >= wy - margin && cy <= wy + wh + margin;

    if (isNear && !isTempUnlocked.value) {
      isTempUnlocked.value = true;
      await windowManager.setIgnoreCursorEvents("desktop-lyrics", false);
      showHeader.value = true;
    } else if (!isNear && isTempUnlocked.value && !isHeaderHovering.value) {
      isTempUnlocked.value = false;
      showHeader.value = false;
      await windowManager.setIgnoreCursorEvents("desktop-lyrics", true);
    }
  }, 150);
}

function stopCursorPolling() {
  if (cursorPollInterval) {
    clearInterval(cursorPollInterval);
    cursorPollInterval = null;
  }
}

// ── Seek on Line Click ────────────────────────────────────────────────

function seekToLine(line: VisibleLine) {
  if (isLocked.value) return;
  bridge.seek(line.lineStartTime / 1000);
}

// ── Close ─────────────────────────────────────────────────────────────

async function handleClose() {
  await windowManager.closeWindow("desktop-lyrics");
}

// ── Lifecycle ─────────────────────────────────────────────────────────

let unlistenUnlock: (() => void) | null = null;
let unlistenResized: (() => void) | null = null;

onMounted(async () => {
  // Set initial height from window
  windowHeight.value = window.innerHeight;

  // Start RAF loop for time interpolation
  rafId = requestAnimationFrame(tick);

  const tauri = window.__TAURI__;
  if (!tauri) return;

  // Unlock event from main window (tray/BigPlayer button)
  unlistenUnlock = await tauri.event.listen("desktop-lyrics-unlock", () => {
    if (!isLocked.value) return;
    handleUnlock();
  });

  // Window resize event for font size
  unlistenResized = await tauri.event.listen(
    "desktop-lyrics-resized",
    (e: { payload: [number, number] }) => {
      if (Array.isArray(e.payload)) {
        const [, physicalHeight] = e.payload;
        windowHeight.value = physicalHeight / (window.devicePixelRatio || 1);
      }
    },
  );
});

onUnmounted(() => {
  cancelAnimationFrame(rafId);
  clearHeaderTimeout();
  stopCursorPolling();
  if (reLockTimeout) clearTimeout(reLockTimeout);
  if (unlistenUnlock) unlistenUnlock();
  if (unlistenResized) unlistenResized();
});
</script>

<style lang="scss" scoped>
// ── Main container ────────────────────────────────────────────────────
.desktop-lyric {
  width: 100%;
  height: 100%;
  position: relative;
  overflow: hidden;
  user-select: none;
  background: transparent;
  transition: background 0.3s ease;

  &.hovering {
    background: rgba(0, 0, 0, 0.35);
    border-radius: 12px;
  }

  &.locked {
    background: transparent;
  }
}

// ── Drag layer ────────────────────────────────────────────────────────
.drag-layer {
  position: absolute;
  inset: 0;
  z-index: 0;
  cursor: grab;

  &:active {
    cursor: grabbing;
  }
}

// ── Header bar ────────────────────────────────────────────────────────
.header {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  z-index: 10;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 4px 10px;
  background: rgba(20, 20, 20, 0.75);
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
  border-bottom: 1px solid rgba(255, 255, 255, 0.08);
  min-height: 32px;
}

.header-left {
  flex: 1;
  min-width: 0;
  margin-right: 8px;
}

.song-name {
  display: block;
  font-size: 12px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.85);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-family:
    "HarmonyOS Sans SC",
    "Segoe UI",
    system-ui,
    -apple-system,
    sans-serif;
}

.header-center {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}

.header-right {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
  margin-left: 8px;
}

.ctrl-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  background: none;
  border: none;
  color: rgba(255, 255, 255, 0.85);
  cursor: pointer;
  padding: 4px;
  border-radius: 6px;
  transition: all 0.15s ease;
  font-family: inherit;

  &:hover {
    background: rgba(255, 255, 255, 0.12);
  }

  &:active {
    transform: scale(0.92);
  }
}

.play-btn {
  background: rgba(255, 255, 255, 0.1);
  padding: 4px 6px;
  border-radius: 8px;

  &:hover {
    background: rgba(255, 255, 255, 0.18);
  }
}

.font-btn {
  font-size: 12px;
  font-weight: 700;
  padding: 2px 5px;
  min-width: 28px;
}

.font-sign {
  font-size: 10px;
  margin-left: 1px;
}

// Locked header
.locked-header {
  gap: 8px;
  width: 100%;
  justify-content: center;
}

.locked-label {
  font-size: 12px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.7);
}

.unlock-btn {
  font-size: 12px;
  font-weight: 600;
  gap: 4px;
  padding: 3px 10px;
  background: rgba(255, 255, 255, 0.1);
  border-radius: 12px;

  &:hover {
    background: rgba(255, 255, 255, 0.2);
  }
}

// Header transitions
.header-fade-enter-active,
.header-fade-leave-active {
  transition: all 0.25s cubic-bezier(0.4, 0, 0.2, 1);
}

.header-fade-enter-from,
.header-fade-leave-to {
  opacity: 0;
  transform: translateY(-100%);
}

// ── Lyric area ────────────────────────────────────────────────────────
.lyric-area {
  position: relative;
  z-index: 1;
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  pointer-events: none;
  padding: 8px 24px;
  box-sizing: border-box;
}

.lyric-container {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  width: 100%;
  position: relative;
}

// ── Lyric line ────────────────────────────────────────────────────────
.lyric-line {
  pointer-events: auto;
  text-align: center;
  white-space: nowrap;
  overflow: hidden;
  max-width: 100%;
  cursor: default;
  padding: 2px 0;
  transition: opacity 0.3s ease;

  &.current {
    opacity: 1;
  }

  &:not(.current) {
    opacity: 0.45;
    filter: blur(0.5px);

    &:hover {
      opacity: 0.7;
      filter: blur(0);
    }
  }
}

.lyric-inner {
  display: inline;
  // Text stroke (4-dir shadow) + drop shadow for readability
  text-shadow:
    -1px -1px 0 rgba(0, 0, 0, 0.8),
    1px -1px 0 rgba(0, 0, 0, 0.8),
    -1px 1px 0 rgba(0, 0, 0, 0.8),
    1px 1px 0 rgba(0, 0, 0, 0.8),
    0 2px 8px rgba(0, 0, 0, 0.6),
    0 0 20px rgba(0, 0, 0, 0.4);
}

// ── YRC Word gradient ─────────────────────────────────────────────────
.lyric-word {
  display: inline;
  background: linear-gradient(
    to right,
    var(--active-color, rgb(255, 255, 255)) 50%,
    var(--inactive-color, rgba(255, 255, 255, 0.35)) 50%
  );
  background-size: 200% 100%;
  -webkit-background-clip: text;
  background-clip: text;
  -webkit-text-fill-color: transparent;
  color: transparent;
  // text-shadow doesn't work with background-clip:text in WebKit,
  // so use -webkit-text-stroke for the outline
  -webkit-text-stroke: 2px rgba(0, 0, 0, 0.5);
  paint-order: stroke fill;
  filter: drop-shadow(0 2px 4px rgba(0, 0, 0, 0.5));
}

// Plain text (non-YRC or next line)
.lyric-text {
  color: var(--active-color, rgb(255, 255, 255));

  &:not(.current) {
    color: rgba(255, 255, 255, 0.6);
  }
}

// Translation line
.lyric-tran {
  text-align: center;
  color: rgba(255, 255, 255, 0.7);
  margin-top: 2px;
  text-shadow:
    -1px -1px 0 rgba(0, 0, 0, 0.6),
    1px -1px 0 rgba(0, 0, 0, 0.6),
    -1px 1px 0 rgba(0, 0, 0, 0.6),
    1px 1px 0 rgba(0, 0, 0, 0.6),
    0 1px 4px rgba(0, 0, 0, 0.4);
  font-family:
    "HarmonyOS Sans SC",
    "Segoe UI",
    system-ui,
    -apple-system,
    sans-serif;
}

// ── Lyric line transitions ────────────────────────────────────────────
.lyric-slide-enter-active,
.lyric-slide-leave-active {
  transition: all 0.4s cubic-bezier(0.4, 0, 0.2, 1);
}

.lyric-slide-enter-from {
  opacity: 0;
  transform: translateY(100%);
}

.lyric-slide-leave-to {
  opacity: 0;
  transform: translateY(-100%);
}

.lyric-slide-move {
  transition: all 0.4s cubic-bezier(0.4, 0, 0.2, 1);
}

// Leaving elements need absolute positioning for FLIP animation
.lyric-slide-leave-active {
  position: absolute;
}

// ── No lyrics state ───────────────────────────────────────────────────
.no-lyrics {
  pointer-events: auto;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  text-align: center;
  animation: fadeIn 0.5s ease;
}

@keyframes fadeIn {
  from {
    opacity: 0;
    transform: translateY(10px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.song-title {
  color: rgba(255, 255, 255, 0.95);
  text-shadow:
    -1px -1px 0 rgba(0, 0, 0, 0.85),
    1px -1px 0 rgba(0, 0, 0, 0.85),
    -1px 1px 0 rgba(0, 0, 0, 0.85),
    1px 1px 0 rgba(0, 0, 0, 0.85),
    0 0 1px rgba(0, 0, 0, 0.9),
    0 2px 8px rgba(0, 0, 0, 0.5);
  letter-spacing: 0.02em;
}

.artist-name {
  font-size: 14px;
  font-weight: 400;
  color: rgba(255, 255, 255, 0.75);
  text-shadow:
    -1px -1px 0 rgba(0, 0, 0, 0.6),
    1px -1px 0 rgba(0, 0, 0, 0.6),
    -1px 1px 0 rgba(0, 0, 0, 0.6),
    1px 1px 0 rgba(0, 0, 0, 0.6),
    0 1px 4px rgba(0, 0, 0, 0.4);
}
</style>
