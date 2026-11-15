<template>
  <div ref="rootRef" class="overflow-marquee" :style="marqueeStyle">
    <div ref="contentRef" class="overflow-marquee__group">
      <slot />
    </div>
    <div v-if="overflow" ref="copyRef" class="overflow-marquee__group" aria-hidden="true">
      <slot />
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * Single-line text that scrolls only when it does not fit, and sits still otherwise.
 *
 * It decides that itself: the caller used to measure a hidden copy against some ancestor, which
 * went stale whenever the width changed without a text change, and inside an `out-in` transition
 * measured the leaving line instead of the entering one. Both the box and the content are
 * observed, so a resize, a font load or a new slot text all re-decide.
 *
 * Each loop rests at the start for `pause` seconds before it scrolls again: text that never stops
 * moving cannot be read. The rest's share of the loop depends on the text's width, which a CSS
 * keyframe percentage cannot follow, so motion-v drives it.
 */
import { animate } from "motion-v";
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";

const props = withDefaults(
  defineProps<{
    /** px per second */
    speed?: number;
    gap?: number;
    /** Seconds the text rests at the start of each loop. */
    pause?: number;
  }>(),
  {
    speed: 34,
    gap: 32,
    pause: 3,
  },
);

const rootRef = ref<HTMLElement | null>(null);
const contentRef = ref<HTMLElement | null>(null);
const copyRef = ref<HTMLElement | null>(null);
const contentWidth = ref(0);
const overflow = ref(false);
let resizeObserver: ResizeObserver | null = null;
let scroll: ReturnType<typeof animate> | null = null;
let scrollRun = 0;
const reducedMotion =
  typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

const marqueeStyle = computed(() => ({
  "--overflow-marquee-gap": `${props.gap}px`,
}));

function stopScroll() {
  scroll?.stop();
  scroll = null;
  for (const group of [contentRef.value, copyRef.value]) group?.style.removeProperty("transform");
}

// One loop is a rest at the start, then one group's width plus the gap, which leaves the copy
// exactly where the original began — so wrapping back to the start is invisible.
async function restartScroll() {
  stopScroll();
  const run = ++scrollRun;
  if (!overflow.value || reducedMotion) return;
  await nextTick();
  if (run !== scrollRun) return;
  const groups = [contentRef.value, copyRef.value].filter((el): el is HTMLElement => !!el);
  if (groups.length < 2) return;
  const distance = contentWidth.value + props.gap;
  const travel = Math.max(0.8, distance / Math.max(1, props.speed));
  const total = travel + props.pause;
  scroll = animate(
    groups,
    { x: [0, 0, -distance] },
    { duration: total, times: [0, props.pause / total, 1], ease: "linear", repeat: Infinity },
  );
}

watch([overflow, contentWidth, () => props.speed, () => props.gap, () => props.pause], () => {
  void restartScroll();
});

function measure() {
  const root = rootRef.value;
  const content = contentRef.value;
  if (!root || !content) return;
  // The group is max-content wide and carries no padding, so its width is the text's.
  contentWidth.value = content.getBoundingClientRect().width;
  overflow.value = contentWidth.value > root.clientWidth + 1;
}

onMounted(() => {
  resizeObserver = new ResizeObserver(measure);
  if (rootRef.value) resizeObserver.observe(rootRef.value);
  if (contentRef.value) resizeObserver.observe(contentRef.value);
  measure();
});

onUnmounted(() => {
  resizeObserver?.disconnect();
  stopScroll();
});
</script>

<style scoped>
.overflow-marquee {
  width: 100%;
  min-width: 0;
  overflow: hidden;
  display: flex;
  align-items: center;
  gap: var(--overflow-marquee-gap);
}

.overflow-marquee__group {
  flex: 0 0 auto;
  min-width: max-content;
  display: inline-flex;
  align-items: center;
  white-space: nowrap;
}
</style>
