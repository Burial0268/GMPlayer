<template>
  <div
    class="desktop-lyric"
    :class="{
      locked: isLocked,
      hovered: isHovering,
      'temp-unlocked': isTempUnlocked,
      'no-animation': !animationsEnabled,
    }"
    :data-tauri-drag-region="!isLocked || undefined"
    @mousemove="onMouseMove"
    @mouseenter="onMouseEnter"
    @mouseleave="onMouseLeave"
    @contextmenu.prevent
  >
    <!-- Header bar -->
    <div
      ref="headerRef"
      class="header"
      :data-tauri-drag-region="!isLocked || undefined"
      @mouseenter="isHeaderHovering = true"
      @mouseleave="onHeaderLeave"
    >
      <template v-if="!isLocked">
        <div class="header-left">
          <span
            class="song-name"
            :title="state.title"
            :data-tauri-drag-region="!isLocked || undefined"
          >
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
            class="ctrl-btn lock-btn"
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
        <div class="header-left locked-info">
          <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor">
            <path
              d="M18 8h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2zm-6 9c-1.1 0-2-.9-2-2s.9-2 2-2 2 .9 2 2-.9 2-2 2zm3.1-9H8.9V6c0-1.71 1.39-3.1 3.1-3.1s3.1 1.39 3.1 3.1v2z"
            />
          </svg>
          <span class="locked-label">{{ $t("desktopLyrics.locked") }}</span>
        </div>
        <div class="header-center">
          <button class="ctrl-btn" @pointerdown.stop @click="bridge.prevTrack()">
            <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor">
              <path d="M6 6h2v12H6zm3.5 6l8.5 6V6z" />
            </svg>
          </button>
          <button class="ctrl-btn play-btn" @pointerdown.stop @click="bridge.playPause()">
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
          <button class="ctrl-btn" @pointerdown.stop @click="bridge.nextTrack()">
            <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor">
              <path d="M6 18l8.5-6L6 6v12zM16 6v12h2V6h-2z" />
            </svg>
          </button>
        </div>
        <div class="header-right">
          <button class="ctrl-btn unlock-btn" @pointerdown.stop @click="handleUnlock">
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

    <!-- Lyric content -->
    <div class="lyric-area" :class="{ 'no-animation': !animationsEnabled }">
      <TransitionGroup
        v-if="displayState === 'hasLyrics'"
        :name="animationsEnabled ? 'lyric-slide' : ''"
        tag="div"
        class="lyric-container"
        :class="positionClass"
      >
        <div
          v-for="(line, index) in renderLyricLines"
          :key="line.key"
          :ref="(el) => setLyricLineRef(el as HTMLElement, line.key)"
          class="lyric-line"
          :class="{ current: line.isCurrent, title: line.isTitle }"
          :style="{
            top: getLineTop(index),
            fontSize: index > 0 ? '0.8em' : '1em',
          }"
          @click="seekToLine(line)"
        >
          <div class="lyric-inner" :style="lyricTextStyle">
            <template v-if="line.isCurrent && line.words.length > 1 && bridge.settings.showYrc">
              <span v-for="(word, wi) in line.words" :key="wi" class="lyric-word">
                <span class="word-bg">{{ word.word }}</span>
                <span class="word-fill" :style="getWordStyle(word)">{{ word.word }}</span>
              </span>
            </template>
            <template v-else>
              <LyricScroll
                class="lyric-scroll-line"
                :text="line.text"
                :progress="line.isCurrent ? lineScrollProgress(line) : 0"
                :align="scrollAlign"
              >
                <span class="lyric-text" :class="{ current: line.isCurrent }">{{ line.text }}</span>
              </LyricScroll>
            </template>
          </div>
          <div
            v-if="line.translatedLyric && bridge.settings.showTransl"
            class="lyric-tran"
            :style="tranStyle"
          >
            {{ line.translatedLyric }}
          </div>
          <div
            v-if="line.romanLyric && bridge.settings.showRoma"
            class="lyric-tran lyric-roma"
            :style="tranStyle"
          >
            {{ line.romanLyric }}
          </div>
        </div>
      </TransitionGroup>
      <div
        v-else-if="displayState === 'noLyrics'"
        ref="noLyricsRef"
        class="no-lyrics"
        :style="lyricTextStyle"
      >
        <span class="song-title">{{ $t("desktopLyrics.pureMusic") }}</span>
      </div>
      <div v-else ref="noLyricsRef" class="no-lyrics" :style="lyricTextStyle">
        <span class="song-title">{{ state.title || $t("desktopLyrics.noLyrics") }}</span>
        <span v-if="state.artist" class="artist-name">{{ state.artist }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, shallowRef, onMounted, onUnmounted, nextTick } from "vue";
import { usePlayerBridge } from "@/utils/tauri/playerBridge";
import { windowManager } from "@/utils/tauri/windowManager";
import LyricScroll from "@/components/Lyric/LyricScroll.vue";
import type { AMLLLine, AMLLWord } from "@/utils/LyricsProcessor";

const bridge = usePlayerBridge();
const { state } = bridge;

// ── Constants ─────────────────────────────────────────────────────────

/** YRC word animations start this many ms early for perceived responsiveness */
const LYRIC_LOOKAHEAD = 300;
/** Line switching needs a smaller lookahead than per-word fill to avoid feeling late. */
const LINE_SWITCH_LOOKAHEAD = 180;
const TIME_SYNC_THRESHOLD = 80;

// ── Lyric Data ────────────────────────────────────────────────────────

const amllLines = shallowRef<AMLLLine[]>([]);
const songGeneration = ref(0);
let lyricPayloadKey = "";

const hasLyrics = computed(() => amllLines.value && amllLines.value.length > 0);

function getLyricPayloadKey(data: typeof bridge.lyricData.value) {
  const lines = data?.amllLines ?? [];
  const first = lines[0];
  const last = lines[lines.length - 1];
  return [
    data?.songId ?? "none",
    lines.length,
    first?.startTime ?? 0,
    last?.startTime ?? 0,
    last?.endTime ?? 0,
  ].join(":");
}

watch(
  () => bridge.lyricData.value,
  (data) => {
    const nextKey = getLyricPayloadKey(data);
    const lyricChanged = nextKey !== lyricPayloadKey;
    lyricPayloadKey = nextKey;

    if (data?.amllLines && data.amllLines.length > 0) {
      amllLines.value = data.amllLines;
    } else {
      amllLines.value = [];
    }

    if (lyricChanged) {
      songGeneration.value++;
    }
  },
  { immediate: true },
);

/** Falls back to next line's startTime when endTime is invalid */
function getSafeEndTime(lines: AMLLLine[], idx: number): number {
  const line = lines[idx];
  if (line.endTime > line.startTime) return line.endTime;
  if (idx + 1 < lines.length) return lines[idx + 1].startTime;
  return line.startTime + 5000;
}

// ── Time Interpolation (RAF + performance.now anchor) ─────────────────

const interpolatedTimeMs = ref(0);
let timeAnchorMs = 0;
let perfAnchor = 0;
let lastTimePacketAt = 0;
let rafId = 0;

// Re-anchor the interpolation clock to an authoritative time packet from the master.
// Snap the displayed time when paused, on the initial sync (forceSnap), or when drift
// exceeds the threshold; otherwise keep the smoothly-predicted value to avoid jitter.
function syncBridgeTime(sec: number, forceSnap = false) {
  const bridgeMs = sec * 1000 + (bridge.settings.lyricTimeOffset ?? 0);
  const now = performance.now();
  const predicted = timeAnchorMs + (now - perfAnchor);
  const shouldSnap =
    forceSnap || !state.isPlaying || Math.abs(bridgeMs - predicted) > TIME_SYNC_THRESHOLD;
  timeAnchorMs = bridgeMs;
  perfAnchor = now;
  lastTimePacketAt = now;
  interpolatedTimeMs.value = shouldSnap ? bridgeMs : predicted;
}

watch(
  () => bridge.currentTime.value,
  (sec) => syncBridgeTime(sec),
);

function tick() {
  const recentlyReceivedTime = performance.now() - lastTimePacketAt < 300;
  if (state.isPlaying || recentlyReceivedTime) {
    interpolatedTimeMs.value = timeAnchorMs + (performance.now() - perfAnchor);
  }
  rafId = requestAnimationFrame(tick);
}

// ── Lyric Index (binary search) ───────────────────────────────────────

const currentLineIndex = computed(() => {
  const timeMs = interpolatedTimeMs.value + LINE_SWITCH_LOOKAHEAD;
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
  romanLyric: string;
  lineStartTime: number;
  lineEndTime: number;
  lineIndex: number;
  isTitle: boolean;
}

function clamp(v: number, min: number, max: number) {
  return Math.max(min, Math.min(max, v));
}

const renderLyricLines = computed<VisibleLine[]>(() => {
  const lines = amllLines.value;
  if (!lines || lines.length === 0) return [];
  const idx = currentLineIndex.value;
  const gen = songGeneration.value;
  const result: VisibleLine[] = [];

  // Before first lyric line: show song title + artist as placeholder
  if (idx < 0) {
    result.push({
      key: `${gen}-title`,
      isCurrent: true,
      words: [],
      text: state.title || "",
      translatedLyric: state.artist || "",
      romanLyric: "",
      lineStartTime: 0,
      lineEndTime: lines[0]?.startTime ?? 0,
      lineIndex: -1,
      isTitle: true,
    });
    return result;
  }

  // Current line
  if (idx < lines.length) {
    const line = lines[idx];
    const endTime = getSafeEndTime(lines, idx);
    result.push({
      key: `${gen}-${idx}`,
      isCurrent: true,
      words: line.words,
      text: line.words.map((w) => w.word).join(""),
      translatedLyric: line.translatedLyric || "",
      romanLyric: line.romanLyric || "",
      lineStartTime: line.startTime,
      lineEndTime: endTime,
      lineIndex: idx,
      isTitle: false,
    });
  }

  // Next line (double-line mode: only when translation is off)
  if (!bridge.settings.showTransl) {
    const nextIdx = idx + 1;
    if (nextIdx < lines.length && result.length < 2) {
      const line = lines[nextIdx];
      const endTime = getSafeEndTime(lines, nextIdx);
      result.push({
        key: `${gen}-${nextIdx}`,
        isCurrent: false,
        words: line.words,
        text: line.words.map((w) => w.word).join(""),
        translatedLyric: line.translatedLyric || "",
        romanLyric: line.romanLyric || "",
        lineStartTime: line.startTime,
        lineEndTime: endTime,
        lineIndex: nextIdx,
        isTitle: false,
      });
    }
  }

  return result;
});

// ── YRC Word Gradient Style ───────────────────────────────────────────

function getWordStyle(word: AMLLWord) {
  const duration = word.endTime - word.startTime;
  if (duration <= 0) return { clipPath: "inset(0 0% 0 0)" };
  const progress = clamp(
    (interpolatedTimeMs.value - word.startTime + LYRIC_LOOKAHEAD) / duration,
    0,
    1,
  );
  return {
    clipPath: `inset(0 ${(1 - progress) * 100}% 0 0)`,
  };
}

// ── Font Size ─────────────────────────────────────────────────────────

const fontSizeOffset = ref(0);
const windowHeight = ref(120);

const localFontSize = computed(() => {
  const base = clamp(20 + (windowHeight.value - 100) * 0.3, 20, 80);
  return clamp(Math.round(base + fontSizeOffset.value), 16, 96);
});

function getLineTop(index: number) {
  if (index === 0) return "0px";
  return `${localFontSize.value * 1.9}px`;
}

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

// ── Lyric Position / Alignment ────────────────────────────────────────

const positionClass = computed(() => {
  const pos = bridge.settings.lyricsPosition;
  if (pos === "left") return "pos-left";
  if (pos === "right") return "pos-right";
  return "pos-center";
});

// ── Animation Toggle ──────────────────────────────────────────────────

const animationsEnabled = computed(() => bridge.settings.showYrcAnimation !== false);

// ── Display State ─────────────────────────────────────────────────────

const displayState = computed(() => {
  if (state.isLoading) return "loading";
  if (hasLyrics.value) return "hasLyrics";
  return "noLyrics";
});

// ── Horizontal Scroll for Long Lyrics ─────────────────────────────────
// Overflow measurement + transform live inside the shared LyricScroll component;
// here we just feed it the per-line playback progress and the desired alignment.

const scrollAlign = computed<"left" | "center" | "right">(() => {
  const pos = bridge.settings.lyricsPosition;
  if (pos === "center") return "center";
  if (pos === "right") return "right";
  return "left";
});

function lineScrollProgress(line: VisibleLine) {
  const duration = Math.max(800, line.lineEndTime - line.lineStartTime);
  return clamp((interpolatedTimeMs.value - line.lineStartTime) / duration, 0, 1);
}

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
// cursorPollInterval removed — replaced by rdev-based global mouse listener
let reLockTimeout: ReturnType<typeof setTimeout> | null = null;

function scheduleReLock() {
  if (reLockTimeout) clearTimeout(reLockTimeout);
  reLockTimeout = setTimeout(() => {
    if (isLocked.value && isTempUnlocked.value && !isHeaderHovering.value) {
      isTempUnlocked.value = false;
      isHovering.value = false;
      showHeader.value = false;
      // Re-enable full click-through immediately
      windowManager.setIgnoreCursorEvents("desktop-lyrics", true);
    }
  }, 1500);
}

async function toggleLock() {
  isLocked.value = true;
  showHeader.value = false;
  isHovering.value = false;
  clearHeaderTimeout();
  // Enter full click-through mode immediately.
  // The rdev listener will detect when the cursor approaches the
  // unlock-trigger zone and temporarily disable click-through.
  await windowManager.setIgnoreCursorEvents("desktop-lyrics", true);
  await startMouseThrough();
}

async function handleUnlock() {
  isLocked.value = false;
  isTempUnlocked.value = false;
  stopCursorPolling();
  if (reLockTimeout) {
    clearTimeout(reLockTimeout);
    reLockTimeout = null;
  }
  // Cancel any pending mouse-through event listener
  if (unlistenMouseThrough) {
    unlistenMouseThrough();
    unlistenMouseThrough = null;
  }
  await stopMouseThrough();
  // In unlocked mode the window should NOT be click-through so normal
  // mouseenter/leave/mousemove handlers can drive the UI.
  await windowManager.setIgnoreCursorEvents("desktop-lyrics", false);
}

function startCursorPolling() {
  // Replaced by rdev-based global mouse listener (startMouseThrough).
  // Kept as no-op for backward compatibility in case any caller remains.
}

function stopCursorPolling() {
  // Replaced by stopMouseThrough.
}

// ── Seek on Line Click ────────────────────────────────────────────────

function seekToLine(line: VisibleLine) {
  if (isLocked.value || line.isTitle) return;
  bridge.seek(line.lineStartTime / 1000);
}

// ── Close ─────────────────────────────────────────────────────────────

async function handleClose() {
  await windowManager.closeWindow("desktop-lyrics");
}

// ── Transparent-Through Mouse Tracking ────────────────────────────────

/** Whether the native mouse-through listener is active */
const mouseThroughActive = ref(false);
let unlistenMouseThrough: (() => void) | null = null;

/** DOM refs for hit regions that should capture mouse when locked */
const headerRef = ref<HTMLElement | null>(null);
const lyricLineMap = new Map<string, HTMLElement>();
const noLyricsRef = ref<HTMLElement | null>(null);

function setLyricLineRef(el: HTMLElement | null, key: string) {
  if (el) {
    lyricLineMap.set(key, el);
  } else {
    lyricLineMap.delete(key);
  }
}

interface HitRegion {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Collect current interactive hit regions for the Rust listener */
function collectHitRegions(): HitRegion[] {
  const regions: HitRegion[] = [];

  // In locked mode, the ONLY interactive element is the unlock header.
  // We define a "trigger zone" at the top of the window — when the cursor
  // enters this zone we show the header and temporarily disable click-through.
  // The zone is always present (regardless of showHeader) so the user can
  // trigger it without first being inside a hit region.
  if (isLocked.value) {
    // Trigger zone: full window width, top 40px
    regions.push({
      id: "unlock-trigger",
      x: 0,
      y: 0,
      width: window.innerWidth,
      height: 40,
    });

    // When header is actually shown, expand the hit region to cover the
    // entire header so the user can reach the unlock button.
    if (showHeader.value && headerRef.value) {
      const rect = headerRef.value.getBoundingClientRect();
      regions.push({
        id: "header",
        x: rect.left,
        y: rect.top,
        width: rect.width,
        height: rect.height,
      });
    }
  }

  // Lyric lines (clickable for seek) — only when NOT locked
  if (!isLocked.value) {
    for (const [key, el] of lyricLineMap) {
      const rect = el.getBoundingClientRect();
      regions.push({
        id: `lyric-${key}`,
        x: rect.left,
        y: rect.top,
        width: rect.width,
        height: rect.height,
      });
    }

    // No-lyrics placeholder (also clickable)
    if (noLyricsRef.value) {
      const rect = noLyricsRef.value.getBoundingClientRect();
      regions.push({
        id: "no-lyrics",
        x: rect.left,
        y: rect.top,
        width: rect.width,
        height: rect.height,
      });
    }
  }

  return regions;
}

/** Start the global mouse-through listener */
async function startMouseThrough() {
  const tauri = window.__TAURI__;
  if (!tauri) return;

  // Wait for DOM to settle so rects are accurate
  await nextTick();
  const regions = collectHitRegions();

  await tauri.core.invoke("start_mouse_through", {
    label: "desktop-lyrics",
    regions,
  });

  mouseThroughActive.value = true;

  // Listen for state changes from Rust
  unlistenMouseThrough = await tauri.event.listen<boolean>("mouse-through-state", (e) => {
    // e.payload = true when cursor is inside a hit region
    const inside = e.payload;

    // ── Locked mode logic ──────────────────────────────────────────
    if (isLocked.value) {
      if (inside) {
        // Cursor is in the unlock-trigger zone or the header itself.
        // Disable click-through so the webview can receive mouse events.
        windowManager.setIgnoreCursorEvents("desktop-lyrics", false);

        // Show the unlock header if not already visible.
        if (!isTempUnlocked.value) {
          isTempUnlocked.value = true;
          isHovering.value = true;
          showHeader.value = true;
        }
      } else {
        // Cursor left all hit regions (trigger zone + header).
        // Re-enable full click-through after a short grace period.
        if (isTempUnlocked.value && !isHeaderHovering.value) {
          scheduleReLock();
        }
      }
      return;
    }

    // ── Unlocked mode logic ────────────────────────────────────────
    // Only lyric lines / no-lyrics placeholder are interactive.
    windowManager.setIgnoreCursorEvents("desktop-lyrics", !inside);
  });
}

/** Stop the global mouse-through listener */
async function stopMouseThrough() {
  const tauri = window.__TAURI__;
  if (!tauri) return;

  await tauri.core.invoke("stop_mouse_through", {
    label: "desktop-lyrics",
  });

  mouseThroughActive.value = false;

  if (unlistenMouseThrough) {
    unlistenMouseThrough();
    unlistenMouseThrough = null;
  }
}

/** Update hit regions without restarting the listener */
async function updateMouseThroughRegions() {
  const tauri = window.__TAURI__;
  if (!tauri || !mouseThroughActive.value) return;

  await nextTick();
  const regions = collectHitRegions();

  await tauri.core.invoke("update_mouse_through_regions", {
    label: "desktop-lyrics",
    regions,
  });
}

// Watch for header visibility changes to update hit regions
watch(showHeader, () => {
  if (isLocked.value && mouseThroughActive.value) {
    updateMouseThroughRegions();
  }
});

// Watch for lyric content changes to update hit regions
watch(renderLyricLines, () => {
  if (isLocked.value && mouseThroughActive.value) {
    nextTick(() => updateMouseThroughRegions());
  }
});

// ── Lifecycle ─────────────────────────────────────────────────────────

let unlistenUnlock: (() => void) | null = null;
let unlistenResized: (() => void) | null = null;

onMounted(async () => {
  // Set initial height from window
  windowHeight.value = window.innerHeight;

  // Anchor the interpolation clock to the current bridge time before the RAF loop
  // starts, so early frames extrapolate from a real perf origin instead of 0 (which
  // used to fling the current line on open). tick() then advances whenever playing.
  syncBridgeTime(bridge.currentTime.value, true);

  // Start RAF loop for time interpolation
  rafId = requestAnimationFrame(tick);

  const tauri = window.__TAURI__;
  if (!tauri) return;
  await windowManager.setIgnoreCursorEvents("desktop-lyrics", false);

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
  if (unlistenMouseThrough) {
    unlistenMouseThrough();
    unlistenMouseThrough = null;
  }
  stopMouseThrough();
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
.locked-info {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
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
  justify-content: center;
  width: 100%;
  position: relative;

  &.pos-left {
    align-items: flex-start;

    .lyric-line {
      text-align: left;
    }

    .lyric-tran {
      text-align: left;
    }
  }

  &.pos-center {
    align-items: center;

    .lyric-line {
      text-align: center;
    }
  }

  &.pos-right {
    align-items: flex-end;

    .lyric-line {
      text-align: right;
    }

    .lyric-tran {
      text-align: right;
    }
  }
}

// ── Lyric line ────────────────────────────────────────────────────────
.lyric-line {
  pointer-events: auto;
  white-space: nowrap;
  overflow: hidden;
  width: 100%;
  max-width: 100%;
  cursor: default;
  padding: 2px 0;
  transition: opacity 0.3s ease;

  &.current {
    opacity: 1;
  }

  &.title {
    cursor: default;
    pointer-events: none;
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
  // Keep the main shadow on glyph-bearing spans. When it is inherited from the
  // full-width line container, clipped scrolling lines can look like a box shadow.
  --lyric-text-shadow:
    -1px -1px 0 rgba(0, 0, 0, 0.8), 1px -1px 0 rgba(0, 0, 0, 0.8), -1px 1px 0 rgba(0, 0, 0, 0.8),
    1px 1px 0 rgba(0, 0, 0, 0.8), 0 2px 8px rgba(0, 0, 0, 0.6), 0 0 20px rgba(0, 0, 0, 0.4);
}

// ── YRC Word dual-layer ──────────────────────────────────────────────
// Inactive layer (.word-bg) shows full text in dim color with inherited text-shadow outline.
// Active layer (.word-fill) overlays bright text, clipped by playback progress.
.lyric-word {
  position: relative;
  display: inline-block;
  vertical-align: baseline;
  // Prevent HTML from collapsing trailing spaces inside each word.
  // TTML lyrics rely on trailing spaces to separate words.
  white-space: pre;
}

.word-bg {
  color: var(--inactive-color, rgba(255, 255, 255, 0.35));
  text-shadow: var(--lyric-text-shadow);
}

.word-fill {
  position: absolute;
  left: 0;
  top: 0;
  color: var(--active-color, rgb(255, 255, 255));
  text-shadow: none;
  will-change: clip-path;
  pointer-events: none;
}

// Plain text (non-YRC or next line)
.lyric-text {
  color: var(--active-color, rgb(255, 255, 255));
  text-shadow: var(--lyric-text-shadow);

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

// Romanization line
.lyric-roma {
  font-style: italic;
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
  width: 100%;
}

// ── No animation mode ────────────────────────────────────────────────
.no-animation {
  .lyric-line,
  .lyric-inner,
  .lyric-word,
  .word-fill,
  .lyric-text,
  .lyric-tran {
    transition: none !important;
  }
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

// SPlayer-style desktop lyric shell: transparent by default, controls appear on hover.
.desktop-lyric {
  display: flex;
  flex-direction: column;
  padding: 12px;
  border-radius: 12px;
  color: #fff;
  cursor: default;
  transition: background-color 0.3s ease;

  &.hovered:not(.locked) {
    background-color: rgba(0, 0, 0, 0.6);
  }

  .header {
    position: relative;
    inset: auto;
    z-index: 10;
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: 12px;
    min-height: 36px;
    margin-bottom: 12px;
    padding: 0;
    border: 0;
    background: transparent;
    backdrop-filter: none;
    -webkit-backdrop-filter: none;
    pointer-events: none;

    > * {
      min-width: 0;
    }
  }

  .header-left,
  .header-center,
  .header-right {
    display: flex;
    align-items: center;
    min-width: 0;
    margin: 0;
    gap: 6px;
  }

  .header-left {
    justify-content: flex-start;
  }

  .header-center {
    justify-content: center;
  }

  .header-right {
    justify-content: flex-end;
  }

  .song-name {
    flex: 1 1 auto;
    min-width: 0;
    padding: 0 8px;
    font-size: 13px;
    line-height: 36px;
    text-align: left;
    opacity: 0;
    transition: opacity 0.3s ease;
  }

  .ctrl-btn {
    flex: 0 0 auto;
    min-width: 0;
    padding: 6px;
    border-radius: 8px;
    opacity: 0;
    background: transparent;
    color: rgba(255, 255, 255, 0.9);
    pointer-events: auto;
    transition:
      opacity 0.3s ease,
      background-color 0.3s ease,
      transform 0.3s ease;

    &:hover {
      background-color: rgba(255, 255, 255, 0.3);
    }

    &:active {
      transform: scale(0.96);
    }
  }

  &.hovered:not(.locked) {
    .song-name,
    .ctrl-btn {
      opacity: 1;
    }
  }

  &.locked {
    .header {
      pointer-events: none;
    }

    .song-name,
    .ctrl-btn,
    .lyric-area {
      pointer-events: none;
    }

    &.hovered,
    &.temp-unlocked {
      .header {
        pointer-events: auto;
      }

      .ctrl-btn,
      .locked-info {
        opacity: 1;
      }

      .unlock-btn {
        pointer-events: auto;
      }
    }
  }

  .locked-info {
    opacity: 0;
    transition: opacity 0.3s ease;
  }

  .unlock-btn {
    gap: 4px;
    padding: 6px 10px;
    border-radius: 12px;
    background-color: rgba(255, 255, 255, 0.12);
  }

  .lyric-area {
    flex: 1 1 auto;
    display: block;
    min-height: 0;
    padding: 0 8px;
    pointer-events: none;
  }

  .lyric-container {
    position: relative;
    display: block;
    width: 100%;
    height: 100%;
    padding: 0;
    cursor: move;
  }

  .lyric-line {
    position: absolute;
    left: 0;
    width: 100%;
    max-width: 100%;
    box-sizing: border-box;
    padding: 4px max(10px, 0.2em);
    line-height: normal;
    white-space: nowrap;
    pointer-events: auto;
    transition:
      top 0.6s cubic-bezier(0.55, 0, 0.1, 1),
      font-size 0.6s cubic-bezier(0.55, 0, 0.1, 1),
      color 0.6s cubic-bezier(0.55, 0, 0.1, 1),
      opacity 0.6s cubic-bezier(0.55, 0, 0.1, 1),
      transform 0.6s cubic-bezier(0.55, 0, 0.1, 1);
    will-change: top, font-size, transform;
    transform-origin: left center;

    &:not(.current) {
      opacity: 0.72;
      filter: none;
    }
  }

  .lyric-inner {
    display: block;
    width: 100%;
  }

  .lyric-scroll-line {
    // Short lines (the common case) aren't clipped by LyricScroll, so their soft
    // drop-shadow renders as a halo hugging the glyphs. Long lines do clip while
    // scrolling. Give the viewport room for the shadow so edge glyphs are not
    // cropped into a rectangular shape.
    &[data-scroll="true"] {
      box-sizing: border-box;
      padding: 0.7em max(10px, 0.2em);
      margin-block: -0.7em;
    }
  }

  .pos-center .lyric-line {
    text-align: center;
    transform-origin: center center;
  }

  .pos-right .lyric-line {
    text-align: right;
    transform-origin: right center;
  }

  &.no-animation {
    .lyric-line,
    .lyric-slide-move,
    .lyric-slide-enter-active,
    .lyric-slide-leave-active {
      transition: none !important;
    }
  }
}

.lyric-slide-move,
.lyric-slide-enter-active,
.lyric-slide-leave-active {
  transition:
    transform 0.6s cubic-bezier(0.55, 0, 0.1, 1),
    opacity 0.6s cubic-bezier(0.55, 0, 0.1, 1);
  will-change: transform, opacity;
}

.lyric-slide-enter-from {
  opacity: 0;
  transform: translateY(100%);
}

.lyric-slide-leave-to {
  opacity: 0;
  transform: translateY(-100%);
}

.lyric-slide-leave-active {
  position: absolute;
}
</style>
