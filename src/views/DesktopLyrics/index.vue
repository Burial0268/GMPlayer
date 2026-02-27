<template>
  <div
    class="desktop-lyrics"
    :class="{ locked: isLocked, 'has-lyrics': hasLyrics }"
    @mouseenter="onMouseEnter"
    @mouseleave="onMouseLeave"
    @contextmenu.prevent
  >
    <!-- Drag handle (left side) -->
    <div v-if="!isLocked" class="drag-handle" data-tauri-drag-region title="Drag to move">
      <div class="drag-dots">
        <span></span>
        <span></span>
        <span></span>
      </div>
    </div>

    <!-- Hover controls -->
    <Transition name="controls-fade">
      <div v-if="showControls" class="controls-bar">
        <button
          class="ctrl-btn"
          :class="{ active: isLocked }"
          @click="toggleLock"
          :title="isLocked ? $t('desktopLyrics.unlock') : $t('desktopLyrics.lock')"
        >
          <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor">
            <path
              v-if="isLocked"
              d="M18 8h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2zm-6 9c-1.1 0-2-.9-2-2s.9-2 2-2 2 .9 2 2-.9 2-2 2zm3.1-9H8.9V6c0-1.71 1.39-3.1 3.1-3.1 1.71 0 3.1 1.39 3.1 3.1v2z"
            />
            <path
              v-else
              d="M12 17c1.1 0 2-.9 2-2s-.9-2-2-2-2 .9-2 2 .9 2 2 2zm6-9h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6h1.9c0-1.71 1.39-3.1 3.1-3.1 1.71 0 3.1 1.39 3.1 3.1v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2zm0 12H6V10h12v10z"
            />
          </svg>
        </button>
        <div class="ctrl-divider"></div>
        <button class="ctrl-btn" @click="decreaseFontSize" :title="$t('desktopLyrics.smaller')">
          <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor">
            <path d="M19 13H5v-2h14v2z" />
          </svg>
        </button>
        <span class="font-size-indicator">{{ localFontSize }}</span>
        <button class="ctrl-btn" @click="increaseFontSize" :title="$t('desktopLyrics.larger')">
          <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor">
            <path d="M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z" />
          </svg>
        </button>
        <div class="ctrl-divider"></div>
        <button class="ctrl-btn close" @click="closeWindow" :title="$t('desktopLyrics.close')">
          <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor">
            <path
              d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z"
            />
          </svg>
        </button>
      </div>
    </Transition>

    <!-- Lock indicator -->
    <Transition name="lock-indicator-fade">
      <div v-if="isLocked && showLockIndicator" class="lock-indicator">
        <svg viewBox="0 0 24 24" width="12" height="12" fill="currentColor">
          <path
            d="M18 8h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2zm-6 9c-1.1 0-2-.9-2-2s.9-2 2-2 2 .9 2 2-.9 2-2 2zm3.1-9H8.9V6c0-1.71 1.39-3.1 3.1-3.1 1.71 0 3.1 1.39 3.1 3.1v2z"
          />
        </svg>
        <span>{{ $t("desktopLyrics.locked") }}</span>
      </div>
    </Transition>

    <!-- Content container -->
    <div class="content-wrapper">
      <!-- AMLL Lyric Player -->
      <LyricPlayer
        v-if="hasLyrics"
        class="amll-lyric-player"
        :lyric-lines="amllLines"
        :current-time="adjustedTime"
        :playing="state.isPlaying"
        :enable-blur="false"
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
        :style="lyricStyles"
        :key="playerKey"
        ref="amllPlayerRef"
      />
      <div v-else class="no-lyrics">
        <div class="no-lyrics-content">
          <div class="music-icon">
            <svg viewBox="0 0 24 24" width="32" height="32" fill="currentColor">
              <path
                d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"
              />
            </svg>
          </div>
          <span class="song-title">{{ state.title || $t("desktopLyrics.noLyrics") }}</span>
          <span v-if="state.artist" class="artist-name">{{ state.artist }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, shallowRef, onMounted, onUnmounted } from "vue";
import { usePlayerBridge } from "@/utils/tauri/playerBridge";
import { windowManager } from "@/utils/tauri/windowManager";
import { LyricPlayer, LyricPlayerRef } from "@applemusic-like-lyrics/vue";
import type { AMLLLine } from "@/utils/LyricsProcessor";
import "@applemusic-like-lyrics/core/style.css";

const bridge = usePlayerBridge();
const { state } = bridge;

const playerKey = ref(Symbol());
const amllPlayerRef = ref<LyricPlayerRef>();
const showControls = ref(false);
const isLocked = ref(false);
const showLockIndicator = ref(false);
const localFontSize = ref(46);
const amllLines = shallowRef<AMLLLine[]>([]);
let lockIndicatorTimer: ReturnType<typeof setTimeout> | null = null;

// ── Cursor Polling State ────────────────────────────────────────────
// When locked (click-through), the window ignores all cursor events, so
// browser mouseenter never fires. We poll the screen cursor position from
// Rust and temporarily lift ignore_cursor_events when the cursor is over
// the window — allowing native hover to take over. On mouseleave we
// re-enable click-through and restart the poll.

let cursorPollTimer: ReturnType<typeof setInterval> | null = null;
const CURSOR_POLL_INTERVAL = 100; // ms

async function isCursorInWindow(): Promise<boolean> {
  try {
    const [cursor, bounds] = await Promise.all([
      windowManager.getCursorPosition(),
      windowManager.getWindowBounds("desktop-lyrics"),
    ]);
    if (!cursor || !bounds) return false;
    const [cx, cy] = cursor;
    const [wx, wy, ww, wh] = bounds;
    return cx >= wx && cx <= wx + ww && cy >= wy && cy <= wy + wh;
  } catch {
    return false;
  }
}

function startCursorPolling() {
  stopCursorPolling();
  cursorPollTimer = setInterval(async () => {
    if (await isCursorInWindow()) {
      // Cursor entered — temporarily lift click-through so browser hover works
      stopCursorPolling();
      try {
        await windowManager.setIgnoreCursorEvents("desktop-lyrics", false);
      } catch {
        // If this fails, restart polling
        if (isLocked.value) startCursorPolling();
      }
      // showControls will be set true by onMouseEnter from the browser
    }
  }, CURSOR_POLL_INTERVAL);
}

function stopCursorPolling() {
  if (cursorPollTimer !== null) {
    clearInterval(cursorPollTimer);
    cursorPollTimer = null;
  }
}

// ── AMLL Data ────────────────────────────────────────────────────────

const hasLyrics = computed(() => {
  return amllLines.value && amllLines.value.length > 0;
});

// Update AMLL lines when lyric data arrives from bridge
watch(
  () => bridge.lyricData.value,
  (data) => {
    if (data?.amllLines && data.amllLines.length > 0) {
      amllLines.value = data.amllLines;
      playerKey.value = Symbol(); // Force remount
    } else {
      amllLines.value = [];
    }
  },
  { immediate: true },
);

// ── Time Calculation ─────────────────────────────────────────────────

const adjustedTime = computed(() => {
  return bridge.currentTime.value * 1000 + (bridge.settings.lyricTimeOffset ?? 0);
});

// ── Styling ──────────────────────────────────────────────────────────

const lyricStyles = computed(() => ({
  "--amll-lp-color": state.accentColor ? `rgb(${state.accentColor})` : "rgb(239, 239, 239)",
  "--amll-lyric-view-color": state.accentColor ? `rgb(${state.accentColor})` : "rgb(239, 239, 239)",
  "--amll-lp-font-size": `${localFontSize.value}px`,
  "font-weight": bridge.settings.lyricFontWeight || "bold",
  "font-family": bridge.settings.lyricFont || "HarmonyOS Sans SC",
  "letter-spacing": bridge.settings.lyricLetterSpacing || "normal",
  "font-size": `${localFontSize.value}px`,
  cursor: "default",
  "-webkit-tap-highlight-color": "transparent",
}));

// ── Play state sync ──────────────────────────────────────────────────

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

// ── Lyric Click → Seek ──────────────────────────────────────────────

const handleLineClick = (evt: any) => {
  const targetTime = evt.line.getLine().startTime;
  amllPlayerRef.value?.lyricPlayer.value?.setCurrentTime(targetTime, true);
  amllPlayerRef.value?.lyricPlayer.value?.update();
  bridge.seek(targetTime / 1000);
};

// ── Mouse Events ────────────────────────────────────────────────────

function onMouseEnter() {
  showControls.value = true;
}

function onMouseLeave() {
  showControls.value = false;
  if (isLocked.value) {
    // Re-enable click-through and restart polling
    windowManager.setIgnoreCursorEvents("desktop-lyrics", true).catch(() => {});
    startCursorPolling();
  }
}

// ── Controls ─────────────────────────────────────────────────────────

async function toggleLock() {
  isLocked.value = !isLocked.value;
  try {
    if (isLocked.value) {
      // Entering lock: hide controls, enable click-through, start polling
      showControls.value = false;
      showLockIndicator.value = true;
      // Hide lock indicator after 2 seconds
      if (lockIndicatorTimer) clearTimeout(lockIndicatorTimer);
      lockIndicatorTimer = setTimeout(() => {
        showLockIndicator.value = false;
      }, 2000);
      await windowManager.setIgnoreCursorEvents("desktop-lyrics", true);
      startCursorPolling();
    } else {
      // Unlocking: disable click-through, stop polling
      stopCursorPolling();
      showLockIndicator.value = false;
      if (lockIndicatorTimer) clearTimeout(lockIndicatorTimer);
      await windowManager.setIgnoreCursorEvents("desktop-lyrics", false);
    }
  } catch (err) {
    console.warn("[DesktopLyrics] Could not set cursor events:", err);
  }
}

async function unlock() {
  if (!isLocked.value) return;
  isLocked.value = false;
  stopCursorPolling();
  try {
    await windowManager.setIgnoreCursorEvents("desktop-lyrics", false);
  } catch (err) {
    console.warn("[DesktopLyrics] Could not unlock:", err);
  }
}

function increaseFontSize() {
  localFontSize.value = Math.min(80, localFontSize.value + 4);
}

function decreaseFontSize() {
  localFontSize.value = Math.max(20, localFontSize.value - 4);
}

function closeWindow() {
  stopCursorPolling();
  windowManager.closeWindow("desktop-lyrics");
}

// ── Lifecycle ────────────────────────────────────────────────────────

let unlistenUnlock: (() => void) | null = null;

onMounted(async () => {
  // Listen for unlock event from main window (e.g., via tray or BigPlayer button)
  const tauri = window.__TAURI__;
  if (tauri) {
    const u = await tauri.event.listen("desktop-lyrics-unlock", () => {
      unlock();
    });
    unlistenUnlock = u;
  }
});

onUnmounted(() => {
  stopCursorPolling();
  if (unlistenUnlock) unlistenUnlock();
  if (lockIndicatorTimer) clearTimeout(lockIndicatorTimer);
});
</script>

<style lang="scss" scoped>
// ── Main container ────────────────────────────────────────────────────
.desktop-lyrics {
  width: 100%;
  height: 100%;
  position: relative;
  overflow: hidden;
  user-select: none;
  background: transparent;
  transition: background 0.3s ease;

  // Only show background when hovering and not locked
  &:hover:not(.locked) {
    background: rgba(0, 0, 0, 0.2);
    box-shadow:
      0 8px 32px rgba(0, 0, 0, 0.3),
      inset 0 1px 0 rgba(255, 255, 255, 0.1);
  }

  &.locked {
    background: transparent;
  }
}

// ── Drag handle (left side) ───────────────────────────────────────────
.drag-handle {
  position: absolute;
  top: 50%;
  left: 8px;
  transform: translateY(-50%);
  width: 28px;
  height: 80px;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: grab;
  z-index: 10;
  opacity: 0;
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.4);
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
  border: 1px solid rgba(255, 255, 255, 0.1);
  box-shadow:
    0 4px 12px rgba(0, 0, 0, 0.3),
    inset 0 1px 0 rgba(255, 255, 255, 0.08);
  transition: all 0.25s cubic-bezier(0.4, 0, 0.2, 1);

  .desktop-lyrics:not(.locked):hover & {
    opacity: 1;
  }

  &:hover {
    background: rgba(0, 0, 0, 0.6);
    border-color: rgba(255, 255, 255, 0.2);
    transform: translateY(-50%) scale(1.05);

    .drag-dots span {
      background: rgba(255, 255, 255, 0.7);
    }
  }

  &:active {
    cursor: grabbing;
    transform: translateY(-50%) scale(0.98);
  }
}

.drag-dots {
  display: flex;
  flex-direction: column;
  gap: 4px;

  span {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.5);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
    transition: background 0.2s ease;
  }
}

// ── Content wrapper ───────────────────────────────────────────────────
.content-wrapper {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 16px 32px;
  box-sizing: border-box;
}

// ── Controls bar ─────────────────────────────────────────────────────
.controls-fade-enter-active,
.controls-fade-leave-active {
  transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
}

.controls-fade-enter-from,
.controls-fade-leave-to {
  opacity: 0;
  transform: translateY(-8px);
}

.controls-bar {
  position: absolute;
  top: 12px;
  right: 12px;
  display: flex;
  align-items: center;
  gap: 2px;
  z-index: 20;
  background: rgba(0, 0, 0, 0.5);
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
  border-radius: 10px;
  padding: 5px;
  box-shadow:
    0 4px 16px rgba(0, 0, 0, 0.3),
    inset 0 1px 0 rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(255, 255, 255, 0.05);
}

.ctrl-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  background: transparent;
  border: none;
  color: rgba(255, 255, 255, 0.65);
  cursor: pointer;
  border-radius: 6px;
  transition: all 0.15s cubic-bezier(0.4, 0, 0.2, 1);

  &:hover {
    background: rgba(255, 255, 255, 0.12);
    color: #fff;
    transform: translateY(-1px);
  }

  &:active {
    transform: translateY(0);
  }

  &.active {
    background: rgba(255, 255, 255, 0.15);
    color: #fff;
  }

  &.close {
    &:hover {
      background: rgba(232, 65, 66, 0.85);
    }
  }
}

.ctrl-divider {
  width: 1px;
  height: 16px;
  background: rgba(255, 255, 255, 0.1);
  margin: 0 2px;
}

.font-size-indicator {
  font-size: 11px;
  font-weight: 500;
  color: rgba(255, 255, 255, 0.5);
  min-width: 20px;
  text-align: center;
  user-select: none;
}

// ── Lock indicator ───────────────────────────────────────────────────
.lock-indicator-fade-enter-active,
.lock-indicator-fade-leave-active {
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}

.lock-indicator-fade-enter-from,
.lock-indicator-fade-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(-10px);
}

.lock-indicator {
  position: absolute;
  top: 12px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  background: rgba(0, 0, 0, 0.5);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
  border-radius: 20px;
  color: rgba(255, 255, 255, 0.8);
  font-size: 12px;
  font-weight: 500;
  z-index: 15;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
  border: 1px solid rgba(255, 255, 255, 0.05);

  svg {
    opacity: 0.8;
  }
}

// ── AMLL Player ──────────────────────────────────────────────────────
.amll-lyric-player {
  width: 100%;
  height: 100%;

  // Emphasize span padding fix (from LyricPlayer.vue)
  &.dom:deep(span[class^="_emphasizeWrapper"] span) {
    padding: 1em;
    margin: -1em;
  }

  // Improve centering
  &:deep(.amll-lyric-view) {
    justify-content: center;
  }
}

// ── No lyrics state ──────────────────────────────────────────────────
.no-lyrics {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}

.no-lyrics-content {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  text-align: center;
  padding: 24px;
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

.music-icon {
  width: 56px;
  height: 56px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(255, 255, 255, 0.08);
  border-radius: 50%;
  color: rgba(255, 255, 255, 0.5);
  margin-bottom: 4px;
}

.song-title {
  font-size: 22px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.85);
  text-shadow:
    0 2px 8px rgba(0, 0, 0, 0.5),
    0 0 20px rgba(0, 0, 0, 0.3);
  font-family:
    "HarmonyOS Sans SC",
    "Segoe UI",
    system-ui,
    -apple-system,
    sans-serif;
  letter-spacing: 0.02em;
}

.artist-name {
  font-size: 14px;
  font-weight: 400;
  color: rgba(255, 255, 255, 0.5);
  text-shadow: 0 1px 4px rgba(0, 0, 0, 0.4);
}
</style>
