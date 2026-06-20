<!--
  LyricScroll — progress-driven single-line scroller for lyric text.

  When the text overflows its container it is translated in sync with playback
  progress (0→1) so the hidden part is revealed exactly as the line plays, rather
  than relying on a fixed-duration CSS loop. Overflow is measured internally via a
  ResizeObserver, so callers only need to pass `text` + `progress`.

  Shared by TaskbarLyrics and DesktopLyrics.
-->
<template>
  <div
    ref="viewportRef"
    class="lyric-scroll"
    :data-orientation="orientation"
    :data-align="align"
    :data-scroll="canScroll ? 'true' : 'false'"
    :style="viewportStyle"
  >
    <span ref="contentRef" class="lyric-scroll__content" :style="contentStyle">
      <slot>{{ text }}</slot>
    </span>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import type { CSSProperties } from "vue";

const props = withDefaults(
  defineProps<{
    /** Text to display (ignored when a default slot is provided). */
    text?: string;
    /** Playback progress through the line, 0→1, drives the scroll offset. */
    progress?: number;
    orientation?: "horizontal" | "vertical";
    align?: "left" | "center" | "right";
    /** Extra pixels scrolled past the overflow so the tail isn't cut tight. */
    endPadding?: number;
  }>(),
  {
    text: "",
    progress: 0,
    orientation: "horizontal",
    align: "left",
    endPadding: 10,
  },
);

const viewportRef = ref<HTMLElement | null>(null);
const contentRef = ref<HTMLElement | null>(null);
const overflow = ref(0);

let resizeObserver: ResizeObserver | null = null;
let measureFrame = 0;

const canScroll = computed(() => overflow.value > 1);

const viewportStyle = computed<CSSProperties>(() => {
  // While scrolling, pin to the leading edge (trailing edge for right-align) so the
  // transform reveals the hidden tail. When it fits, honour the requested alignment.
  let justify: CSSProperties["justifyContent"];
  if (canScroll.value) {
    justify = props.align === "right" ? "flex-end" : "flex-start";
  } else if (props.align === "right") {
    justify = "flex-end";
  } else if (props.align === "center") {
    justify = "center";
  } else {
    justify = "flex-start";
  }
  return { justifyContent: justify };
});

const contentStyle = computed<CSSProperties>(() => {
  if (!canScroll.value) return {};
  const distance = overflow.value + props.endPadding;
  const clamped = Math.min(1, Math.max(0, props.progress));
  if (props.orientation === "vertical") {
    return { transform: `translateY(${-clamped * distance}px)` };
  }
  const direction = props.align === "right" ? 1 : -1;
  return { transform: `translateX(${clamped * distance * direction}px)` };
});

function measure() {
  const viewport = viewportRef.value;
  const content = contentRef.value;
  if (!viewport || !content) {
    overflow.value = 0;
    return;
  }
  const viewportStyle = getComputedStyle(viewport);
  const value =
    props.orientation === "vertical"
      ? content.scrollHeight -
        (viewport.clientHeight -
          parseFloat(viewportStyle.paddingTop) -
          parseFloat(viewportStyle.paddingBottom))
      : content.scrollWidth -
        (viewport.clientWidth -
          parseFloat(viewportStyle.paddingLeft) -
          parseFloat(viewportStyle.paddingRight));
  overflow.value = Math.max(0, value);
}

function scheduleMeasure() {
  nextTick(() => {
    if (measureFrame) cancelAnimationFrame(measureFrame);
    measureFrame = requestAnimationFrame(measure);
  });
}

watch(() => [props.text, props.orientation, props.align], scheduleMeasure);

onMounted(() => {
  resizeObserver = new ResizeObserver(() => measure());
  if (viewportRef.value) resizeObserver.observe(viewportRef.value);
  if (contentRef.value) resizeObserver.observe(contentRef.value);
  scheduleMeasure();
});

onUnmounted(() => {
  if (measureFrame) cancelAnimationFrame(measureFrame);
  resizeObserver?.disconnect();
  resizeObserver = null;
});
</script>

<style lang="scss" scoped>
.lyric-scroll {
  display: flex;
  align-items: center;
  width: 100%;
  min-width: 0;

  // Only clip while actually scrolling. A line that fits is left unclipped so its
  // text-shadow renders fully instead of being cropped into a box by the viewport.
  &[data-scroll="true"] {
    overflow: hidden;
  }

  &[data-orientation="vertical"] {
    width: 1.2em;
    height: 100%;
    align-items: flex-start;
  }
}

.lyric-scroll__content {
  flex: 0 0 auto;
  max-width: none;
  white-space: nowrap;
  will-change: transform;
  transition: transform 0.08s linear;
}

.lyric-scroll[data-orientation="vertical"] .lyric-scroll__content {
  writing-mode: vertical-rl;
}
</style>
