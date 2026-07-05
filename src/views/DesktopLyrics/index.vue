<template>
  <div
    class="desktop-lyric"
    :class="{
      locked: isLocked,
      hovered: isHovering,
      'temp-unlocked': isTempUnlocked,
      'no-animation': !animationsEnabled,
    }"
    @mousemove="onMouseMove"
    @mouseenter="onMouseEnter"
    @mouseleave="onMouseLeave"
    @pointerdown="handleWindowDrag"
    @contextmenu.prevent
  >
    <!-- Header bar -->
    <div
      ref="headerRef"
      class="header"
      @mouseenter="isHeaderHovering = true"
      @mouseleave="onHeaderLeave"
    >
      <template v-if="!isLocked">
        <div class="header-left">
          <span class="song-name" :title="state.title">
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
          class="lyric-line"
          :class="{
            current: line.isTimelineActive,
            primary: line.isCurrent,
            parallel: line.isParallel,
            interlude: line.isInterlude,
            retiring: line.slotIndex < 0,
            title: line.isTitle,
          }"
          :style="getLineStyle(line)"
        >
          <div class="lyric-inner" :style="getLyricTextStyle(line)">
            <LyricScroll
              class="lyric-scroll-line"
              :text="line.text"
              :progress="lineScrollProgress(line)"
              :align="scrollAlign"
              :end-padding="DESKTOP_SCROLL_END_PADDING"
            >
              <template v-if="shouldRenderWordProgress(line)">
                <span
                  v-for="(word, wi) in line.words"
                  :key="wi"
                  class="lyric-word"
                  :class="{ 'interlude-word': line.isInterlude }"
                  :data-word="word.word"
                >
                  <span class="word-bg">{{ word.word }}</span>
                  <span class="word-fill" :style="getWordStyle(word, line)">{{ word.word }}</span>
                </span>
              </template>
              <template v-else>
                <span class="lyric-text" :class="{ current: line.isTimelineActive }">
                  {{ line.text }}
                </span>
              </template>
            </LyricScroll>
          </div>
          <LyricScroll
            v-if="line.isCurrent && line.translatedLyric && bridge.settings.showTransl"
            class="lyric-tran"
            :style="getTranStyle(line)"
            :text="line.translatedLyric"
            :progress="lineScrollProgress(line)"
            :align="scrollAlign"
            :end-padding="DESKTOP_SCROLL_END_PADDING"
          >
            <span class="lyric-sub-text">{{ line.translatedLyric }}</span>
          </LyricScroll>
          <LyricScroll
            v-if="line.isCurrent && line.romanLyric && bridge.settings.showRoma"
            class="lyric-tran lyric-roma"
            :style="getTranStyle(line)"
            :text="line.romanLyric"
            :progress="lineScrollProgress(line)"
            :align="scrollAlign"
            :end-padding="DESKTOP_SCROLL_END_PADDING"
          >
            <span class="lyric-sub-text">{{ line.romanLyric }}</span>
          </LyricScroll>
        </div>
      </TransitionGroup>
      <div v-else-if="displayState === 'noLyrics'" class="no-lyrics" :style="noLyricsTextStyle">
        <span class="song-title">{{ $t("desktopLyrics.pureMusic") }}</span>
      </div>
      <div v-else class="no-lyrics" :style="noLyricsTextStyle">
        <span class="song-title">{{ state.title || $t("desktopLyrics.noLyrics") }}</span>
        <span v-if="state.artist" class="artist-name">{{ state.artist }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, shallowRef, onMounted, onUnmounted, nextTick } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
const DESKTOP_SCROLL_END_PADDING = 14;

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
  interpolatedTimeMs.value = shouldSnap ? bridgeMs : predicted;
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
      syncBridgeTime(bridge.currentTime.value, true);
    }
  },
  { immediate: true },
);

watch(
  () => bridge.currentTime.value,
  (sec) => syncBridgeTime(sec),
);

watch(
  () => state.isPlaying,
  () => syncBridgeTime(bridge.currentTime.value, true),
);

function tick() {
  if (state.isPlaying) {
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
  isTimelineActive: boolean;
  isParallel: boolean;
  isInterlude: boolean;
  slotIndex: number;
  words: AMLLWord[];
  text: string;
  translatedLyric: string;
  romanLyric: string;
  lineStartTime: number;
  lineEndTime: number;
  lineIndex: number;
  isTitle: boolean;
}

interface InterludeLineEnd {
  time: number;
}

interface InterludeWindow {
  anchorLineIndex: number;
  nextIndex: number;
  startTime: number;
  endTime: number;
}

function clamp(v: number, min: number, max: number) {
  return Math.max(min, Math.min(max, v));
}

function buildVisibleLine(
  lines: AMLLLine[],
  index: number,
  gen: number,
  isCurrent: boolean,
  isTimelineActive: boolean,
  isParallel = false,
  slotIndex = 0,
): VisibleLine {
  const line = lines[index];
  return {
    key: `${gen}-${index}`,
    isCurrent,
    isTimelineActive,
    isParallel,
    isInterlude: false,
    slotIndex,
    words: line.words,
    text: line.words.map((w) => w.word).join(""),
    translatedLyric: line.translatedLyric || "",
    romanLyric: line.romanLyric || "",
    lineStartTime: line.startTime,
    lineEndTime: getSafeEndTime(lines, index),
    lineIndex: index,
    isTitle: false,
  };
}

function buildInterludeLine(
  gen: number,
  prevIndex: number,
  nextIndex: number,
  lineStartTime: number,
  lineEndTime: number,
  slotIndex: number,
): VisibleLine {
  return {
    key: `${gen}-interlude-${prevIndex}-${nextIndex}`,
    isCurrent: false,
    isTimelineActive: true,
    isParallel: false,
    isInterlude: true,
    slotIndex,
    words: buildInterludeWords(lineStartTime, lineEndTime),
    text: "...",
    translatedLyric: "",
    romanLyric: "",
    lineStartTime,
    lineEndTime,
    lineIndex: prevIndex,
    isTitle: false,
  };
}

function buildInterludeWords(lineStartTime: number, lineEndTime: number): AMLLWord[] {
  const availableDuration = Math.max(INTERLUDE_DOT_COUNT, lineEndTime - lineStartTime);
  const fillDuration = Math.max(
    INTERLUDE_DOT_COUNT,
    availableDuration - INTERLUDE_DOT_FILL_IDLE_MS,
  );
  const segmentDuration = fillDuration / INTERLUDE_DOT_COUNT;

  return Array.from({ length: INTERLUDE_DOT_COUNT }, (_, dotIndex) => {
    const startTime = lineStartTime + dotIndex * segmentDuration;
    return {
      word: ".",
      startTime,
      endTime:
        dotIndex === INTERLUDE_DOT_COUNT - 1
          ? lineStartTime + fillDuration
          : startTime + segmentDuration,
    };
  });
}

function hasVisibleSubLines(line: VisibleLine | undefined) {
  return Boolean(
    line &&
    ((bridge.settings.showTransl && line.translatedLyric) ||
      (bridge.settings.showRoma && line.romanLyric)),
  );
}

function getLineText(line: AMLLLine) {
  return line.words.map((word) => word.word).join("");
}

function getLastWordEndTime(line: AMLLLine) {
  let endTime = 0;
  for (const word of line.words) {
    if (word.endTime > endTime) endTime = word.endTime;
  }
  return endTime;
}

function getEstimatedLineSingEndTime(line: AMLLLine, nextStartTime: number) {
  const textLength = Array.from(getLineText(line).trim()).length;
  const estimatedDuration = clamp(
    INTERLUDE_ESTIMATED_BASE_DURATION_MS + textLength * INTERLUDE_ESTIMATED_CHAR_DURATION_MS,
    INTERLUDE_ESTIMATED_MIN_DURATION_MS,
    INTERLUDE_ESTIMATED_MAX_DURATION_MS,
  );
  return Math.min(line.startTime + estimatedDuration, nextStartTime - INTERLUDE_LINE_END_GUARD_MS);
}

function hasLineStarted(line: AMLLLine, timelineMs: number) {
  return timelineMs >= line.startTime;
}

function isLineActive(lines: AMLLLine[], index: number, timelineMs: number) {
  const line = lines[index];
  return hasLineStarted(line, timelineMs) && timelineMs < getSafeEndTime(lines, index);
}

function collectActiveLineIndices(
  lines: AMLLLine[],
  latestStartedIndex: number,
  timelineMs: number,
) {
  const activeIndices: number[] = [];
  for (let index = latestStartedIndex; index >= 0; index--) {
    if (isLineActive(lines, index, timelineMs)) {
      activeIndices.push(index);
      if (activeIndices.length >= MAX_DESKTOP_RENDER_LINES) break;
    }
  }
  return activeIndices.reverse();
}

function isLineTimelineActive(
  lines: AMLLLine[],
  index: number,
  timelineMs: number,
  isCurrent = false,
) {
  if (isCurrent) return true;
  const line = lines[index];
  return timelineMs >= line.startTime && timelineMs < getSafeEndTime(lines, index);
}

function getInterludeLineEnd(lines: AMLLLine[], index: number, nextStartTime: number) {
  if (index < 0) {
    return {
      time: 0,
    } satisfies InterludeLineEnd;
  }

  const line = lines[index];

  if (line.endTime > line.startTime && line.endTime < nextStartTime - INTERLUDE_LINE_END_GUARD_MS) {
    return {
      time: line.endTime,
    } satisfies InterludeLineEnd;
  }

  const wordEndTime = getLastWordEndTime(line);
  if (wordEndTime > nextStartTime + INTERLUDE_LINE_END_GUARD_MS) {
    return null;
  }
  if (
    line.words.length > 1 &&
    wordEndTime > line.startTime &&
    wordEndTime < nextStartTime - INTERLUDE_LINE_END_GUARD_MS
  ) {
    return {
      time: wordEndTime,
    } satisfies InterludeLineEnd;
  }

  const estimatedEndTime = getEstimatedLineSingEndTime(line, nextStartTime);
  return {
    time: estimatedEndTime,
  } satisfies InterludeLineEnd;
}

function checkInterludeGap(lines: AMLLLine[], anchorLineIndex: number, currentTime: number) {
  const nextIndex = anchorLineIndex + 1;
  if (nextIndex < 0 || nextIndex >= lines.length) return null;

  const nextStartTime = lines[nextIndex].startTime;
  const lineEnd = getInterludeLineEnd(lines, anchorLineIndex, nextStartTime);
  if (lineEnd === null) return null;

  const startTime = Math.max(0, lineEnd.time);
  const endTime = Math.max(startTime, nextStartTime - INTERLUDE_END_LOOKAHEAD_MS);
  if (endTime - startTime < INTERLUDE_MIN_GAP_MS) return null;
  if (currentTime <= startTime || currentTime >= endTime) return null;

  return {
    anchorLineIndex,
    nextIndex,
    startTime,
    endTime,
  } satisfies InterludeWindow;
}

function getInterludeWindow(lines: AMLLLine[], index: number, currentTime: number) {
  const adjustedTime = currentTime + INTERLUDE_TIME_LOOKAHEAD_MS;
  return (
    checkInterludeGap(lines, index - 1, adjustedTime) ||
    checkInterludeGap(lines, index, adjustedTime) ||
    checkInterludeGap(lines, index + 1, adjustedTime)
  );
}

function getParallelLineSlot(renderIndex: number) {
  return renderIndex;
}

const renderLyricLines = computed<VisibleLine[]>(() => {
  const lines = amllLines.value;
  if (!lines || lines.length === 0) return [];
  const idx = currentLineIndex.value;
  const gen = songGeneration.value;
  const result: VisibleLine[] = [];
  const displayMs = interpolatedTimeMs.value;

  // Before first lyric line: show song title + artist as placeholder
  if (idx < 0) {
    const interlude = getInterludeWindow(lines, idx, displayMs);
    if (interlude) {
      result.push(
        buildInterludeLine(
          gen,
          interlude.anchorLineIndex,
          interlude.nextIndex,
          interlude.startTime,
          interlude.endTime,
          0,
        ),
      );
      return result;
    }

    result.push({
      key: `${gen}-title`,
      isCurrent: true,
      isTimelineActive: true,
      isParallel: false,
      isInterlude: false,
      slotIndex: 0,
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

  if (idx >= lines.length) return result;

  const timelineMs = interpolatedTimeMs.value + LINE_SWITCH_LOOKAHEAD;
  const interlude = getInterludeWindow(lines, idx, displayMs);
  if (interlude) {
    result.push(
      buildInterludeLine(
        gen,
        interlude.anchorLineIndex,
        interlude.nextIndex,
        interlude.startTime,
        interlude.endTime,
        0,
      ),
    );
    return result;
  }

  const activeLineIndices = collectActiveLineIndices(lines, idx, timelineMs);
  const visibleLineIndices = activeLineIndices.length > 0 ? activeLineIndices : [idx];
  const primaryIdx = visibleLineIndices[visibleLineIndices.length - 1];
  const hasParallelRows = visibleLineIndices.length > 1;

  for (let renderIndex = 0; renderIndex < visibleLineIndices.length; renderIndex++) {
    const lineIdx = visibleLineIndices[renderIndex];
    const lineIsPrimary = lineIdx === primaryIdx;
    result.push(
      buildVisibleLine(
        lines,
        lineIdx,
        gen,
        lineIsPrimary,
        isLineTimelineActive(lines, lineIdx, timelineMs, lineIsPrimary),
        hasParallelRows,
        getParallelLineSlot(renderIndex),
      ),
    );
  }

  const primaryLine = result.find((line) => line.lineIndex === primaryIdx);

  // Next line preview stays disabled when the primary line already needs room for
  // translation or romanization; otherwise the bottom auxiliary line can overflow.
  // Parallel lyrics reserve all rows for real active timelines, so a finished middle
  // row can leave while a new active row takes its slot without pushing older lines.
  if (!hasParallelRows && primaryLine && !hasVisibleSubLines(primaryLine)) {
    const nextIdx = idx + 1;
    if (nextIdx < lines.length) {
      result.push(
        buildVisibleLine(
          lines,
          nextIdx,
          gen,
          false,
          isLineTimelineActive(lines, nextIdx, timelineMs),
          false,
          1,
        ),
      );
    }
  }

  return result;
});

// ── YRC Word Gradient Style ───────────────────────────────────────────

function getWordStyle(word: AMLLWord, line: VisibleLine) {
  const duration = word.endTime - word.startTime;
  if (duration <= 0) return { clipPath: "inset(0 0% 0 0)" };
  const lookahead = line.isInterlude ? 0 : LYRIC_LOOKAHEAD;
  const progress = clamp((interpolatedTimeMs.value - word.startTime + lookahead) / duration, 0, 1);
  return {
    clipPath: `inset(0 ${(1 - progress) * 100}% 0 0)`,
  };
}

function shouldRenderWordProgress(line: VisibleLine) {
  if (line.isInterlude) return animationsEnabled.value && line.words.length > 1;
  return (
    !line.isTitle &&
    bridge.settings.showYrc &&
    line.words.length > 1 &&
    (line.isTimelineActive || line.lineIndex < currentLineIndex.value)
  );
}

// ── Font Size ─────────────────────────────────────────────────────────

const MIN_DESKTOP_FONT_SIZE = 16;
const MAX_DESKTOP_FONT_SIZE = 96;
const SECONDARY_LINE_SCALE = 0.8;
const TRANSLATION_FONT_SCALE = 0.45;
const MIN_TRANSLATION_FONT_SIZE = 9;
const MAIN_LINE_HEIGHT = 1.18;
const SUB_LINE_HEIGHT = 1.15;
const PARALLEL_LINE_TOP_SCALE = 1.9;
const MAX_DESKTOP_RENDER_LINES = 3;
const INTERLUDE_DOT_COUNT = 3;
const INTERLUDE_TIME_LOOKAHEAD_MS = 20;
const INTERLUDE_END_LOOKAHEAD_MS = 250;
const INTERLUDE_MIN_GAP_MS = 4000;
const INTERLUDE_LINE_END_GUARD_MS = INTERLUDE_END_LOOKAHEAD_MS;
const INTERLUDE_ESTIMATED_BASE_DURATION_MS = 1100;
const INTERLUDE_ESTIMATED_CHAR_DURATION_MS = 140;
const INTERLUDE_ESTIMATED_MIN_DURATION_MS = 2200;
const INTERLUDE_ESTIMATED_MAX_DURATION_MS = 5200;
const INTERLUDE_DOT_FILL_IDLE_MS = 420;
const LINE_VERTICAL_PADDING = 8;
const SUB_LINE_MARGIN_TOP = 2;
const DESKTOP_SHELL_VERTICAL_PADDING = 24;
const DESKTOP_HEADER_RESERVED_HEIGHT = 48;

const windowHeight = ref(120);
const fontSizeOffset = computed(() => bridge.settings.desktopLyricsFontSizeOffset ?? 0);

function getSubLineCount(line: VisibleLine | undefined) {
  if (!line || !line.isCurrent) return 0;
  let count = 0;
  if (bridge.settings.showTransl && line.translatedLyric) count++;
  if (bridge.settings.showRoma && line.romanLyric) count++;
  return count;
}

function getAvailableLyricHeight() {
  return Math.max(
    44,
    windowHeight.value - DESKTOP_SHELL_VERTICAL_PADDING - DESKTOP_HEADER_RESERVED_HEIGHT,
  );
}

function getDesiredTranslationFontSizeFor(baseFontSize: number) {
  return Math.max(MIN_TRANSLATION_FONT_SIZE, Math.round(baseFontSize * TRANSLATION_FONT_SCALE));
}

function estimateLineHeight(baseFontSize: number, line: VisibleLine, subLineFontSize?: number) {
  const mainFontSize = baseFontSize * getLineScale(line);
  const subLineCount = getSubLineCount(line);
  const resolvedSubLineFontSize = subLineFontSize ?? getDesiredTranslationFontSizeFor(baseFontSize);
  return (
    mainFontSize * MAIN_LINE_HEIGHT +
    subLineCount * (resolvedSubLineFontSize * SUB_LINE_HEIGHT + SUB_LINE_MARGIN_TOP) +
    LINE_VERTICAL_PADDING
  );
}

function estimateLyricGroupHeight(baseFontSize: number, currentSubLineFontSize?: number) {
  const visibleLines = renderLyricLines.value;
  if (visibleLines.length === 0) return baseFontSize * MAIN_LINE_HEIGHT + LINE_VERTICAL_PADDING;

  const lineGap = baseFontSize * PARALLEL_LINE_TOP_SCALE;
  let hasMeasuredLine = false;
  let minTop = 0;
  let maxBottom = 0;

  for (const line of visibleLines) {
    if (line.slotIndex < 0) continue;

    const lineTop = line.slotIndex * lineGap;
    const lineHeight = estimateLineHeight(
      baseFontSize,
      line,
      line.isCurrent ? currentSubLineFontSize : undefined,
    );
    minTop = hasMeasuredLine ? Math.min(minTop, lineTop) : lineTop;
    maxBottom = hasMeasuredLine ? Math.max(maxBottom, lineTop + lineHeight) : lineTop + lineHeight;
    hasMeasuredLine = true;
  }

  return hasMeasuredLine
    ? maxBottom - minTop
    : baseFontSize * MAIN_LINE_HEIGHT + LINE_VERTICAL_PADDING;
}

const localFontSize = computed(() => {
  const base = clamp(20 + (windowHeight.value - 100) * 0.3, 20, 80);
  return clamp(
    Math.round(base + fontSizeOffset.value),
    MIN_DESKTOP_FONT_SIZE,
    MAX_DESKTOP_FONT_SIZE,
  );
});

const currentSubLineFontSize = computed(() => {
  const visibleLines = renderLyricLines.value;
  const currentLine = visibleLines.find((line) => line.isCurrent);
  const subLineCount = getSubLineCount(currentLine);
  const desiredSize = getDesiredTranslationFontSizeFor(localFontSize.value);
  if (!currentLine || subLineCount === 0) return desiredSize;

  const availableHeight = getAvailableLyricHeight();
  const currentTop =
    Math.max(0, currentLine.slotIndex) * localFontSize.value * PARALLEL_LINE_TOP_SCALE;
  const remainingForCurrent = Math.max(0, availableHeight - currentTop - LINE_VERTICAL_PADDING);
  const mainHeight = localFontSize.value * MAIN_LINE_HEIGHT;
  const remainingForSubLines =
    remainingForCurrent - mainHeight - subLineCount * SUB_LINE_MARGIN_TOP;
  const maxSubLineSize = Math.floor(remainingForSubLines / (subLineCount * SUB_LINE_HEIGHT));
  return clamp(maxSubLineSize, MIN_TRANSLATION_FONT_SIZE, desiredSize);
});

const lyricGroupTopOffset = computed(() => {
  const overflow =
    estimateLyricGroupHeight(localFontSize.value, currentSubLineFontSize.value) -
    getAvailableLyricHeight();
  return overflow > 0 ? -Math.ceil(overflow) : 0;
});

function getLineTop(slotIndex: number) {
  return `${lyricGroupTopOffset.value + slotIndex * localFontSize.value * PARALLEL_LINE_TOP_SCALE}px`;
}

function getLineFontSize(line: VisibleLine) {
  return Math.round(localFontSize.value);
}

function getLineScale(line: VisibleLine) {
  return line.isCurrent || line.isTimelineActive || line.isParallel ? 1 : SECONDARY_LINE_SCALE;
}

function getLineStyle(line: VisibleLine) {
  return {
    top: getLineTop(line.slotIndex),
  };
}

function increaseFontSize() {
  bridge.setDesktopLyricsFontSizeOffset(fontSizeOffset.value + 4);
}

function decreaseFontSize() {
  bridge.setDesktopLyricsFontSizeOffset(fontSizeOffset.value - 4);
}

// ── Computed Styles ───────────────────────────────────────────────────

const accentColor = computed(() =>
  state.accentColor ? `rgb(${state.accentColor})` : "rgb(255, 255, 255)",
);

const inactiveColor = computed(() =>
  state.accentColor ? `rgba(${state.accentColor}, 0.35)` : "rgba(255, 255, 255, 0.35)",
);

const secondaryColor = computed(() =>
  state.accentColor ? `rgba(${state.accentColor}, 0.42)` : "rgba(255, 255, 255, 0.42)",
);

const lyricTextStyle = computed(() => ({
  fontWeight: bridge.settings.lyricFontWeight || "bold",
  fontFamily: bridge.settings.lyricFont || "HarmonyOS Sans SC",
  letterSpacing: bridge.settings.lyricLetterSpacing || "normal",
  "--active-color": accentColor.value,
  "--inactive-color": inactiveColor.value,
  "--secondary-color": secondaryColor.value,
}));

const noLyricsTextStyle = computed(() => ({
  ...lyricTextStyle.value,
  fontSize: `${localFontSize.value}px`,
}));

function getLyricTextStyle(line: VisibleLine) {
  return {
    ...lyricTextStyle.value,
    fontSize: `${getLineFontSize(line)}px`,
    transform: `scale(${getLineScale(line)})`,
  };
}

function getTranStyle(line: VisibleLine) {
  return {
    fontSize: `${line.isCurrent ? currentSubLineFontSize.value : getDesiredTranslationFontSizeFor(getLineFontSize(line))}px`,
  };
}

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
  if (hasLyrics.value) return "hasLyrics";
  if (state.isLoading) return "loading";
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

// ── Window Drag ───────────────────────────────────────────────────────

const dragBlockSelector = [
  "button",
  "a",
  "input",
  "textarea",
  "select",
  "[role='button']",
  "[role='slider']",
].join(",");

function handleWindowDrag(event: PointerEvent) {
  if (isLocked.value || event.button !== 0 || event.detail > 1) return;
  if (typeof window === "undefined" || !("__TAURI__" in window)) return;
  const target = event.target;
  if (target instanceof Element && target.closest(dragBlockSelector)) return;

  event.preventDefault();
  void getCurrentWindow()
    .startDragging()
    .catch(() => {});
}

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

    windowManager.setIgnoreCursorEvents("desktop-lyrics", false);
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
  pointer-events: none;
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

  &.current:not(.primary) {
    opacity: 0.78;
  }

  &.current.parallel {
    opacity: 1;
  }

  &.interlude {
    opacity: 1;
    filter: none;
  }

  &.retiring {
    opacity: 0;
    visibility: hidden;
  }

  &.title {
    cursor: default;
    pointer-events: none;
  }

  &:not(.current) {
    opacity: 0.38;
    filter: blur(0.5px);

    &:hover {
      opacity: 0.7;
      filter: blur(0);
    }
  }
}

.lyric-inner {
  display: inline;
  transform-origin: inherit;
  transition: transform 0.6s cubic-bezier(0.55, 0, 0.1, 1);
  will-change: transform;
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
  isolation: isolate;
  vertical-align: baseline;
  // Prevent HTML from collapsing trailing spaces inside each word.
  // TTML lyrics rely on trailing spaces to separate words.
  white-space: pre;

  &::before {
    content: attr(data-word);
    position: absolute;
    left: 0;
    top: 0;
    z-index: 0;
    color: transparent;
    white-space: inherit;
    text-shadow: var(--lyric-text-shadow);
    pointer-events: none;
  }
}

.word-bg {
  position: relative;
  z-index: 1;
  color: var(--inactive-color, rgba(255, 255, 255, 0.35));
  text-shadow: none;
}

.word-fill {
  position: absolute;
  left: 0;
  top: 0;
  z-index: 2;
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
    color: var(--secondary-color, rgba(255, 255, 255, 0.55));
  }
}

.lyric-word.interlude-word {
  font-size: 1.16em;
  letter-spacing: 0.16em;

  .word-bg {
    color: var(--inactive-color, rgba(255, 255, 255, 0.3));
  }

  .word-fill {
    text-shadow: 0 0 0.38em var(--active-color, rgb(255, 255, 255));
  }
}

// Translation line
.lyric-tran {
  width: 100%;
  max-width: 100%;
  min-width: 0;
  text-align: center;
  color: rgba(255, 255, 255, 0.7);
  margin-top: 2px;
  text-shadow: none;
  font-family:
    "HarmonyOS Sans SC",
    "Segoe UI",
    system-ui,
    -apple-system,
    sans-serif;

  &[data-scroll="true"] {
    box-sizing: border-box;
    overflow: clip;
    overflow-clip-margin: var(--lyric-scroll-shadow-bleed);
    padding-block: 0.5em;
    padding-inline: 0;
    margin-block: -0.5em 0;
  }
}

.lyric-sub-text {
  text-shadow:
    -1px -1px 0 rgba(0, 0, 0, 0.6),
    1px -1px 0 rgba(0, 0, 0, 0.6),
    -1px 1px 0 rgba(0, 0, 0, 0.6),
    1px 1px 0 rgba(0, 0, 0, 0.6),
    0 1px 4px rgba(0, 0, 0, 0.4);
}

// Romanization line
.lyric-roma {
  font-style: italic;
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
  pointer-events: none;
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

      .ctrl-btn {
        pointer-events: auto;
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
    pointer-events: none;
    transition:
      top 0.6s cubic-bezier(0.55, 0, 0.1, 1),
      color 0.6s cubic-bezier(0.55, 0, 0.1, 1),
      opacity 0.6s cubic-bezier(0.55, 0, 0.1, 1);
    will-change: top;
    transform-origin: left center;
    --lyric-scroll-shadow-bleed: max(10px, 0.22em);

    &:not(.current) {
      opacity: 0.58;
      filter: none;
    }

    &.current:not(.primary) {
      opacity: 0.78;
    }

    &.current.parallel {
      opacity: 1;
    }

    &.interlude {
      opacity: 1;
      filter: none;
    }

    &.retiring {
      opacity: 0;
    }
  }

  .lyric-inner {
    display: block;
    width: 100%;
    transform-origin: inherit;
  }

  .lyric-scroll-line {
    // Short lines (the common case) aren't clipped by LyricScroll, so their soft
    // drop-shadow renders as a halo hugging the glyphs. Long lines do clip while
    // scrolling. Keep only vertical breathing room here; horizontal padding shifts
    // the measured start edge and makes scrolling lines misalign with normal lines.
    &[data-scroll="true"] {
      box-sizing: border-box;
      overflow: clip;
      overflow-clip-margin: var(--lyric-scroll-shadow-bleed);
      padding-block: 0.7em;
      padding-inline: 0;
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
    .lyric-slide-enter-active,
    .lyric-slide-leave-active {
      transition: none !important;
    }
  }
}

.desktop-lyric .lyric-container .lyric-line.lyric-slide-enter-active,
.desktop-lyric .lyric-container .lyric-line.lyric-slide-leave-active {
  transition:
    transform 0.6s cubic-bezier(0.55, 0, 0.1, 1),
    opacity 0.6s cubic-bezier(0.55, 0, 0.1, 1);
  will-change: transform, opacity;
}

.desktop-lyric .lyric-container .lyric-line.lyric-slide-enter-from {
  opacity: 0;
  transform: translateY(100%);
}

.desktop-lyric .lyric-container .lyric-line.lyric-slide-leave-to {
  opacity: 0;
  transform: translateY(-100%);
}

.desktop-lyric .lyric-container .lyric-line.lyric-slide-leave-active {
  position: absolute;
  width: 100%;
}
</style>
