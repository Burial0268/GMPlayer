<template>
  <div ref="rootRef" :class="['overflow-marquee', { running: overflow }]" :style="marqueeStyle">
    <div ref="contentRef" class="overflow-marquee__group">
      <slot />
    </div>
    <div v-if="overflow" class="overflow-marquee__group" aria-hidden="true">
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
 */
import { computed, onMounted, onUnmounted, ref } from "vue";

const props = withDefaults(
  defineProps<{
    /** px per second */
    speed?: number;
    gap?: number;
  }>(),
  {
    speed: 34,
    gap: 32,
  },
);

const rootRef = ref<HTMLElement | null>(null);
const contentRef = ref<HTMLElement | null>(null);
const contentWidth = ref(0);
const overflow = ref(false);
let resizeObserver: ResizeObserver | null = null;

const marqueeStyle = computed(() => ({
  "--overflow-marquee-duration": `${Math.max(0.8, (contentWidth.value + props.gap) / Math.max(1, props.speed))}s`,
  "--overflow-marquee-gap": `${props.gap}px`,
}));

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

.overflow-marquee.running .overflow-marquee__group {
  animation: overflow-marquee var(--overflow-marquee-duration) linear infinite;
}

@keyframes overflow-marquee {
  from {
    transform: translateX(0);
  }

  to {
    transform: translateX(calc(-100% - var(--overflow-marquee-gap)));
  }
}

@media (prefers-reduced-motion: reduce) {
  .overflow-marquee.running .overflow-marquee__group {
    animation: none;
  }
}
</style>
