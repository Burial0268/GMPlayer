<template>
  <main
    class="taskbar-lyric"
    :data-align="align"
    :data-orientation="orientation"
    :data-visible="isVisible ? 'true' : 'false'"
    @contextmenu.prevent
  >
    <section
      class="taskbar-lyric__container"
      :data-align="align"
      :data-orientation="orientation"
      :data-theme="theme"
      :data-single-line="singleLineMode ? 'true' : 'false'"
      @mouseenter="handleMouseEnter"
      @mousemove="handleMouseMove"
      @mouseleave="handleMouseLeave"
    >
      <div class="cover-wrapper">
        <Transition name="cover-fade" mode="out-in">
          <img
            v-if="bridge.state.coverUrl"
            :key="bridge.state.coverUrl"
            class="cover"
            :src="bridge.state.coverUrl"
            alt=""
            draggable="false"
          />
          <div v-else key="placeholder" class="cover-placeholder" />
        </Transition>
      </div>

      <Transition name="controls">
        <div v-if="isHovered" class="controls-wrapper">
          <div class="controls-panel">
            <button class="control-btn" type="button" title="Previous" @click.stop="handlePrev">
              <IconRewind :key="rewindIconKey" class="control-icon" />
            </button>
            <button
              class="control-btn control-btn--play"
              type="button"
              title="Play/Pause"
              @click.stop="bridge.playPause()"
            >
              <IconPause v-if="bridge.state.isPlaying" class="play-icon" />
              <IconPlay v-else class="play-icon" />
            </button>
            <button class="control-btn" type="button" title="Next" @click.stop="handleNext">
              <IconForward :key="forwardIconKey" class="control-icon" />
            </button>
          </div>
        </div>
      </Transition>

      <div class="text-panel">
        <TransitionGroup name="line-switch" tag="div" class="line-stack">
          <div
            v-for="item in displayItems"
            :key="item.key"
            class="line-row"
            :data-role="item.role"
            :data-active="item.active ? 'true' : 'false'"
          >
            <div
              :ref="(el) => setViewportRef(item.key, el)"
              class="scroll-viewport"
              :data-align="align"
              :data-orientation="orientation"
              :data-scroll="canScroll(item.key) ? 'true' : 'false'"
            >
              <span
                :ref="(el) => setContentRef(item.key, el)"
                class="scroll-content"
                :style="scrollContentStyle(item)"
              >
                {{ item.text }}
              </span>
            </div>
          </div>
        </TransitionGroup>
      </div>
    </section>
  </main>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import type { ComponentPublicInstance, CSSProperties } from "vue";
import IconForward from "@/components/Player/icons/IconForward.vue";
import IconPause from "@/components/Player/icons/IconPause.vue";
import IconPlay from "@/components/Player/icons/IconPlay.vue";
import IconRewind from "@/components/Player/icons/IconRewind.vue";
import "@/components/Player/icons/icon-animations.css";
import { usePlayerBridge } from "@/utils/tauri/playerBridge";
import type { AMLLLine } from "@/utils/LyricsProcessor";

type Orientation = "horizontal" | "vertical";
type Align = "left" | "right";
type Theme = "light" | "dark";
type LineRole = "primary" | "secondary";
type LineMode = "single" | "double";
type VueRefElement = Element | ComponentPublicInstance | null;

interface DisplayLine {
  key: string;
  text: string;
  role: LineRole;
  active: boolean;
  startTime?: number;
  endTime?: number;
}

const LYRIC_LOOKAHEAD_MS = 300;
const MIN_LINE_DURATION_MS = 800;
const SCROLL_END_PADDING = 10;

const bridge = usePlayerBridge();

const isHovered = ref(false);
const isVisible = ref(true);
const orientation = ref<Orientation>("horizontal");
const align = ref<Align>("left");
const theme = ref<Theme>("dark");
const lineMode = ref<LineMode>("double");
const displayTimeMs = ref(0);
const overflowByKey = ref<Record<string, number>>({});
const rewindIconKey = ref(0);
const forwardIconKey = ref(0);

const viewportRefs = new Map<string, HTMLElement>();
const contentRefs = new Map<string, HTMLElement>();
const unlisteners: (() => void)[] = [];

let resizeObserver: ResizeObserver | null = null;
let rafId = 0;
let measureFrame = 0;
let timeAnchorMs = 0;
let perfAnchor = 0;

const singleLineMode = computed(() => lineMode.value === "single");
const lyricLines = computed<AMLLLine[]>(() => bridge.lyricData.value?.amllLines ?? []);
const lrcLines = computed(() => bridge.lyricData.value?.lrc ?? []);
const lyricTimeMs = computed(() => displayTimeMs.value + LYRIC_LOOKAHEAD_MS);
const titleText = computed(() => bridge.state.title?.trim() || "GMPlayer");
const artistText = computed(() => bridge.state.artist?.trim() || "");

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function lineText(line: AMLLLine | null | undefined): string {
  return (
    line?.words
      ?.map((word) => word.word)
      .join("")
      .trim() ?? ""
  );
}

function safeAmllEnd(index: number): number | undefined {
  const line = lyricLines.value[index];
  if (!line) return undefined;
  if (line.endTime > line.startTime) return line.endTime;
  return lyricLines.value[index + 1]?.startTime ?? line.startTime + 5000;
}

function findAmllIndexByTime(timeMs: number): number {
  const lines = lyricLines.value;
  if (!lines.length) return -1;

  let lo = 0;
  let hi = lines.length - 1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (lines[mid].startTime <= timeMs) lo = mid + 1;
    else hi = mid - 1;
  }
  return hi;
}

function findLrcIndexByTime(timeMs: number): number {
  const lines = lrcLines.value;
  if (!lines.length) return -1;

  let lo = 0;
  let hi = lines.length - 1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (lines[mid].time * 1000 <= timeMs) lo = mid + 1;
    else hi = mid - 1;
  }
  return hi;
}

const currentAmllIndex = computed(() => {
  const byTime = findAmllIndexByTime(lyricTimeMs.value);
  if (byTime >= 0) return byTime;

  const synced = bridge.lyricIndex.value;
  return synced >= 0 && synced < lyricLines.value.length ? synced : -1;
});

const currentLrcIndex = computed(() => {
  const byTime = findLrcIndexByTime(lyricTimeMs.value);
  if (byTime >= 0) return byTime;

  const synced = bridge.lyricIndex.value;
  return synced >= 0 && synced < lrcLines.value.length ? synced : -1;
});

const lyricItems = computed<DisplayLine[]>(() => {
  const items: DisplayLine[] = [];
  const amllIndex = currentAmllIndex.value;
  const amllLine = amllIndex >= 0 ? lyricLines.value[amllIndex] : null;

  if (amllLine) {
    const primaryText = lineText(amllLine);
    if (primaryText) {
      const nextLine = lyricLines.value[amllIndex + 1];
      const startTime = amllLine.startTime;
      const endTime = safeAmllEnd(amllIndex);

      items.push({
        key: `amll-primary-${amllIndex}-${startTime}`,
        text: primaryText,
        role: "primary",
        active: true,
        startTime,
        endTime,
      });

      if (!singleLineMode.value) {
        const secondaryText =
          (bridge.settings.showTransl && amllLine.translatedLyric?.trim()) ||
          (bridge.settings.showRoma && amllLine.romanLyric?.trim()) ||
          lineText(nextLine);

        if (secondaryText) {
          items.push({
            key: `amll-secondary-${amllIndex}-${secondaryText}`,
            text: secondaryText,
            role: "secondary",
            active: Boolean(amllLine.translatedLyric || amllLine.romanLyric),
            startTime,
            endTime,
          });
        }
      }
    }
  }

  if (items.length) return items;

  const lrcIndex = currentLrcIndex.value;
  const lrcLine = lrcIndex >= 0 ? lrcLines.value[lrcIndex] : null;
  const primaryText = lrcLine?.content?.trim();
  if (!primaryText) return [];

  const nextLine = lrcLines.value[lrcIndex + 1];
  const startTime = lrcLine.time * 1000;
  const endTime = nextLine ? nextLine.time * 1000 : startTime + 5000;
  items.push({
    key: `lrc-primary-${lrcIndex}-${startTime}`,
    text: primaryText,
    role: "primary",
    active: true,
    startTime,
    endTime,
  });

  if (!singleLineMode.value && nextLine?.content?.trim()) {
    items.push({
      key: `lrc-secondary-${lrcIndex + 1}-${nextLine.time}`,
      text: nextLine.content.trim(),
      role: "secondary",
      active: false,
    });
  }

  return items;
});

const metadataItems = computed<DisplayLine[]>(() => {
  const items: DisplayLine[] = [
    {
      key: `meta-title-${titleText.value}`,
      text: titleText.value,
      role: "primary",
      active: true,
    },
  ];

  if (!singleLineMode.value && artistText.value) {
    items.push({
      key: `meta-artist-${artistText.value}`,
      text: artistText.value,
      role: "secondary",
      active: true,
    });
  }

  return items;
});

const displayItems = computed(() => {
  if (isHovered.value || !lyricItems.value.length) return metadataItems.value;
  return lyricItems.value;
});

function toHTMLElement(el: VueRefElement): HTMLElement | null {
  if (el instanceof HTMLElement) return el;
  const instanceEl = (el as ComponentPublicInstance | null)?.$el;
  return instanceEl instanceof HTMLElement ? instanceEl : null;
}

function setViewportRef(key: string, el: VueRefElement) {
  const element = toHTMLElement(el);
  if (element) viewportRefs.set(key, element);
  else viewportRefs.delete(key);
  scheduleMeasure();
}

function setContentRef(key: string, el: VueRefElement) {
  const element = toHTMLElement(el);
  if (element) contentRefs.set(key, element);
  else contentRefs.delete(key);
  scheduleMeasure();
}

function syncResizeObserver() {
  resizeObserver?.disconnect();
  viewportRefs.forEach((element) => resizeObserver?.observe(element));
  contentRefs.forEach((element) => resizeObserver?.observe(element));
}

function updateOverflow() {
  const activeKeys = new Set(displayItems.value.map((item) => item.key));
  const next: Record<string, number> = {};

  viewportRefs.forEach((_, key) => {
    if (!activeKeys.has(key)) viewportRefs.delete(key);
  });
  contentRefs.forEach((_, key) => {
    if (!activeKeys.has(key)) contentRefs.delete(key);
  });

  for (const item of displayItems.value) {
    const viewport = viewportRefs.get(item.key);
    const content = contentRefs.get(item.key);
    if (!viewport || !content) {
      next[item.key] = 0;
      continue;
    }

    const overflow =
      orientation.value === "vertical"
        ? content.scrollHeight - viewport.clientHeight
        : content.scrollWidth - viewport.clientWidth;
    next[item.key] = Math.max(0, overflow);
  }

  overflowByKey.value = next;
}

function scheduleMeasure() {
  nextTick(() => {
    if (measureFrame) cancelAnimationFrame(measureFrame);
    measureFrame = requestAnimationFrame(() => {
      syncResizeObserver();
      updateOverflow();
    });
  });
}

function canScroll(key: string) {
  return (overflowByKey.value[key] ?? 0) > 1;
}

function lineProgress(item: DisplayLine) {
  if (!item.active || item.startTime === undefined || item.endTime === undefined) return 0;
  const duration = Math.max(MIN_LINE_DURATION_MS, item.endTime - item.startTime);
  return clamp((displayTimeMs.value - item.startTime) / duration, 0, 1);
}

function scrollContentStyle(item: DisplayLine): CSSProperties {
  const overflow = overflowByKey.value[item.key] ?? 0;
  if (overflow <= 1) return {};

  const progress = lineProgress(item);
  const distance = overflow + SCROLL_END_PADDING;
  const direction = orientation.value === "horizontal" && align.value === "right" ? 1 : -1;
  const offset = progress * distance * direction;

  return {
    transform:
      orientation.value === "vertical"
        ? `translateY(${-progress * distance}px)`
        : `translateX(${offset}px)`,
  };
}

function updateTimeAnchor(sec: number) {
  const nextTimeMs = sec * 1000 + (bridge.settings.lyricTimeOffset ?? 0);
  const now = performance.now();
  const predictedTimeMs = timeAnchorMs + (now - perfAnchor);

  if (!bridge.state.isPlaying || Math.abs(nextTimeMs - predictedTimeMs) > 250) {
    timeAnchorMs = nextTimeMs;
    perfAnchor = now;
    displayTimeMs.value = nextTimeMs;
  }
}

function tickTime() {
  if (bridge.state.isPlaying) {
    displayTimeMs.value = timeAnchorMs + (performance.now() - perfAnchor);
  }
  rafId = requestAnimationFrame(tickTime);
}

function updateLayoutMode() {
  orientation.value = window.innerHeight > window.innerWidth ? "vertical" : "horizontal";
  const thickness = orientation.value === "vertical" ? window.innerWidth : window.innerHeight;
  lineMode.value = thickness < 45 ? "single" : "double";
  scheduleMeasure();
}

function updateTheme() {
  theme.value = window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

function setClickInterception(intercept: boolean) {
  window.__TAURI__?.core
    .invoke("plugin:taskbar-lyric|set_click_interception", { intercept })
    .catch(() => {});
}

function setHoverState(hovered: boolean) {
  if (isHovered.value === hovered) return;
  isHovered.value = hovered;
  setClickInterception(hovered);
}

function handleMouseEnter() {
  setHoverState(true);
}

function handleMouseMove() {
  setHoverState(true);
}

function handleMouseLeave() {
  setHoverState(false);
}

interface NativePointerPayload {
  inside?: boolean;
  x?: number;
  y?: number;
  width?: number;
  height?: number;
}

function handleNativePointer(payload: NativePointerPayload) {
  if (!payload.inside) {
    setHoverState(false);
    return;
  }

  const physicalWidth = payload.width && payload.width > 0 ? payload.width : window.innerWidth;
  const physicalHeight = payload.height && payload.height > 0 ? payload.height : window.innerHeight;
  const x = ((payload.x ?? -1) / physicalWidth) * window.innerWidth;
  const y = ((payload.y ?? -1) / physicalHeight) * window.innerHeight;
  const hit = document.elementFromPoint(x, y);
  setHoverState(Boolean(hit?.closest?.(".taskbar-lyric__container")));
}

function handlePrev() {
  rewindIconKey.value++;
  bridge.prevTrack();
}

function handleNext() {
  forwardIconKey.value++;
  bridge.nextTrack();
}

watch(displayItems, scheduleMeasure, { immediate: true });
watch([orientation, singleLineMode], scheduleMeasure);
watch(
  () => bridge.currentTime.value,
  (sec) => updateTimeAnchor(sec),
  { immediate: true },
);
watch(
  () => bridge.settings.lyricTimeOffset,
  () => updateTimeAnchor(bridge.currentTime.value),
);
watch(
  () => bridge.state.isPlaying,
  () => updateTimeAnchor(bridge.currentTime.value),
);

onMounted(async () => {
  perfAnchor = performance.now();
  timeAnchorMs = bridge.currentTime.value * 1000 + (bridge.settings.lyricTimeOffset ?? 0);
  displayTimeMs.value = timeAnchorMs;

  updateLayoutMode();
  updateTheme();

  resizeObserver = new ResizeObserver(updateOverflow);
  tickTime();

  window.addEventListener("resize", updateLayoutMode);
  unlisteners.push(() => window.removeEventListener("resize", updateLayoutMode));

  const themeMedia = window.matchMedia("(prefers-color-scheme: light)");
  themeMedia.addEventListener("change", updateTheme);
  unlisteners.push(() => themeMedia.removeEventListener("change", updateTheme));

  const tauri = window.__TAURI__;
  if (tauri) {
    unlisteners.push(
      await tauri.event.listen("taskbar-layout-extra", (event) => {
        const payload = event.payload as { isCentered?: boolean };
        align.value = payload.isCentered ? "left" : "right";
      }),
    );
    unlisteners.push(
      await tauri.event.listen("taskbar-lyric:fade-out", () => {
        isVisible.value = false;
      }),
    );
    unlisteners.push(
      await tauri.event.listen("taskbar-lyric:fade-in", () => {
        isVisible.value = true;
        scheduleMeasure();
      }),
    );
    unlisteners.push(
      await tauri.event.listen("taskbar-lyric:pointer", (event) => {
        handleNativePointer(event.payload as NativePointerPayload);
      }),
    );
  }

  setHoverState(false);
  scheduleMeasure();
});

onUnmounted(() => {
  setHoverState(false);
  cancelAnimationFrame(rafId);
  if (measureFrame) cancelAnimationFrame(measureFrame);
  resizeObserver?.disconnect();
  unlisteners.splice(0).forEach((unlisten) => unlisten());
});
</script>

<style lang="scss" scoped>
.taskbar-lyric {
  width: 100vw;
  height: 100vh;
  box-sizing: border-box;
  display: flex;
  align-items: center;
  justify-content: flex-start;
  padding: 3px;
  pointer-events: none;
  overflow: hidden;
  opacity: 1;
  filter: blur(0);
  transition:
    opacity 0.35s ease,
    filter 0.35s ease;
}

.taskbar-lyric[data-visible="false"] {
  opacity: 0;
  filter: blur(8px);
}

.taskbar-lyric[data-align="right"] {
  justify-content: flex-end;
}

.taskbar-lyric[data-orientation="vertical"] {
  flex-direction: column;
  justify-content: flex-start;
  align-items: center;
}

.taskbar-lyric[data-orientation="vertical"][data-align="right"] {
  justify-content: flex-end;
}

.taskbar-lyric__container {
  --text-primary: #fff;
  --text-secondary: rgba(255, 255, 255, 0.68);
  --container-bg: rgba(146, 146, 146, 0.45);
  --cover-bg: rgba(255, 255, 255, 0.12);
  --placeholder-bg: linear-gradient(135deg, #444, #202020);
  --button-outline: rgba(255, 255, 255, 0.24);

  pointer-events: auto;
  width: max-content;
  max-width: 100%;
  height: 100%;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 4px 8px;
  box-sizing: border-box;
  border-radius: 8px;
  overflow: hidden;
  color: var(--text-primary);
  background: transparent;
  font-family:
    -apple-system, BlinkMacSystemFont, "SF Pro Display", "PingFang SC", system-ui, "Segoe UI",
    sans-serif;
  font-size: 14px;
  font-weight: 650;
  line-height: 1.2;
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.28);
  transition:
    background 0.18s ease,
    opacity 0.15s ease;
}

.taskbar-lyric__container:hover {
  background: var(--container-bg);
}

.taskbar-lyric__container:active {
  opacity: 0.82;
}

.taskbar-lyric__container[data-theme="light"] {
  --text-primary: #151515;
  --text-secondary: rgba(0, 0, 0, 0.58);
  --container-bg: rgba(255, 255, 255, 0.42);
  --cover-bg: rgba(0, 0, 0, 0.06);
  --placeholder-bg: linear-gradient(135deg, #e2e2e2, #bfbfbf);
  --button-outline: rgba(0, 0, 0, 0.18);

  text-shadow: 0 1px 2px rgba(255, 255, 255, 0.3);
}

.taskbar-lyric__container[data-align="right"] {
  flex-direction: row-reverse;
}

.taskbar-lyric__container[data-orientation="vertical"] {
  width: 100%;
  height: max-content;
  max-height: 100%;
  flex-direction: column;
  justify-content: flex-start;
  padding: 8px 4px;
}

.taskbar-lyric__container[data-orientation="vertical"][data-align="right"] {
  flex-direction: column;
}

.cover-wrapper {
  position: relative;
  flex: 0 0 auto;
  height: 100%;
  aspect-ratio: 1 / 1;
  border-radius: 6px;
  overflow: hidden;
  background: var(--cover-bg);
}

.taskbar-lyric__container[data-orientation="vertical"] .cover-wrapper {
  width: 100%;
  max-width: 64px;
  height: auto;
}

.cover,
.cover-placeholder {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
}

.cover {
  object-fit: cover;
}

.cover-placeholder {
  background: var(--placeholder-bg);
}

.controls-wrapper {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}

.controls-panel {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 2px 0;
}

.taskbar-lyric__container[data-orientation="vertical"] .controls-panel {
  flex-direction: column;
  padding: 0;
}

.control-btn {
  appearance: none;
  width: 28px;
  height: 28px;
  padding: 0;
  border: 0;
  border-radius: 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-primary);
  background: transparent;
  box-shadow: 0 0 0 1px var(--button-outline);
}

.control-btn:hover {
  background: color-mix(in srgb, var(--text-primary) 12%, transparent);
}

.control-icon {
  width: 24px;
  height: 24px;
}

.play-icon {
  width: 12px;
  height: 12px;
}

.text-panel {
  position: relative;
  flex: 1 1 auto;
  width: min(560px, max(64px, calc(100vw - 86px)));
  max-width: min(560px, max(64px, calc(100vw - 86px)));
  min-width: 0;
  overflow: hidden;
  -webkit-mask-image: linear-gradient(
    to right,
    transparent 0,
    #000 5px,
    #000 calc(100% - 5px),
    transparent 100%
  );
  mask-image: linear-gradient(
    to right,
    transparent 0,
    #000 5px,
    #000 calc(100% - 5px),
    transparent 100%
  );
}

.taskbar-lyric__container[data-single-line="true"] .text-panel {
  width: min(560px, max(48px, calc(100vw - 58px)));
  max-width: min(560px, max(48px, calc(100vw - 58px)));
}

.taskbar-lyric__container[data-orientation="vertical"] .text-panel {
  width: 100%;
  max-width: 100%;
  height: min(360px, max(64px, calc(100vh - 96px)));
  max-height: min(360px, max(64px, calc(100vh - 96px)));
  -webkit-mask-image: linear-gradient(
    to bottom,
    transparent 0,
    #000 5px,
    #000 calc(100% - 5px),
    transparent 100%
  );
  mask-image: linear-gradient(
    to bottom,
    transparent 0,
    #000 5px,
    #000 calc(100% - 5px),
    transparent 100%
  );
}

.line-stack {
  position: relative;
  min-width: 0;
  min-height: 1.2em;
  display: flex;
  flex-direction: column;
  justify-content: center;
}

.taskbar-lyric__container[data-orientation="vertical"] .line-stack {
  height: 100%;
  min-height: 0;
  flex-direction: row-reverse;
  align-items: center;
  justify-content: center;
}

.line-row {
  width: 100%;
  min-width: 0;
  height: 1.2em;
  display: flex;
  align-items: center;
  overflow: hidden;
  color: var(--text-primary);
  transition:
    color 0.22s ease,
    opacity 0.22s ease,
    transform 0.24s ease;
}

.line-row[data-role="secondary"] {
  color: var(--text-secondary);
  font-size: 0.82em;
}

.line-row[data-active="false"] {
  opacity: 0.88;
}

.taskbar-lyric__container[data-align="right"] .line-row {
  justify-content: flex-end;
  text-align: right;
}

.taskbar-lyric__container[data-orientation="vertical"] .line-row {
  width: 1.2em;
  height: 100%;
  align-items: flex-start;
  justify-content: center;
}

.scroll-viewport {
  width: 100%;
  min-width: 0;
  height: 1.2em;
  display: flex;
  align-items: center;
  overflow: hidden;
}

.scroll-viewport[data-align="right"] {
  justify-content: flex-end;
}

.scroll-viewport[data-orientation="vertical"] {
  width: 1.2em;
  height: 100%;
  align-items: flex-start;
}

.scroll-content {
  flex: 0 0 auto;
  max-width: none;
  white-space: nowrap;
  will-change: transform;
  transition: transform 0.08s linear;
}

.scroll-viewport[data-scroll="false"] .scroll-content {
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
}

.scroll-viewport[data-orientation="vertical"] .scroll-content {
  writing-mode: vertical-rl;
}

.cover-fade-enter-active,
.cover-fade-leave-active {
  transition: opacity 0.18s ease;
}

.cover-fade-enter-from,
.cover-fade-leave-to,
.controls-enter-from,
.controls-leave-to,
.line-switch-enter-from,
.line-switch-leave-to {
  opacity: 0;
}

.controls-enter-active,
.controls-leave-active {
  transition:
    opacity 0.16s ease,
    transform 0.18s ease;
}

.controls-enter-from,
.controls-leave-to {
  transform: translateX(-8px);
}

.taskbar-lyric__container[data-align="right"] .controls-enter-from,
.taskbar-lyric__container[data-align="right"] .controls-leave-to {
  transform: translateX(8px);
}

.taskbar-lyric__container[data-orientation="vertical"] .controls-enter-from,
.taskbar-lyric__container[data-orientation="vertical"] .controls-leave-to {
  transform: translateY(-8px);
}

.line-switch-enter-active,
.line-switch-leave-active {
  transition:
    opacity 0.18s ease,
    transform 0.22s ease,
    filter 0.22s ease;
}

.line-switch-enter-from {
  transform: translateY(0.8em);
  filter: blur(3px);
}

.line-switch-leave-to {
  transform: translateY(-0.5em);
  filter: blur(3px);
}

.taskbar-lyric__container[data-orientation="vertical"] .line-switch-enter-from {
  transform: translateX(-0.8em);
}

.taskbar-lyric__container[data-orientation="vertical"] .line-switch-leave-to {
  transform: translateX(0.5em);
}
</style>
