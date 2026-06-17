<template>
  <div ref="rootRef" class="overflow-marquee" :style="marqueeStyle">
    <div ref="contentRef" class="overflow-marquee__group">
      <slot />
    </div>
    <div class="overflow-marquee__group" aria-hidden="true">
      <slot />
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";

const props = withDefaults(
  defineProps<{
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
let resizeObserver: ResizeObserver | null = null;

const duration = computed(() => {
  const distance = Math.max(1, contentWidth.value);
  return Math.max(0.8, distance / Math.max(1, props.speed));
});

const marqueeStyle = computed(() => ({
  "--overflow-marquee-duration": `${duration.value}s`,
  "--overflow-marquee-gap": `${props.gap}px`,
}));

function measure() {
  contentWidth.value = contentRef.value?.scrollWidth ?? 0;
}

function scheduleMeasure() {
  nextTick(measure);
}

onMounted(() => {
  resizeObserver = new ResizeObserver(measure);
  if (rootRef.value) resizeObserver.observe(rootRef.value);
  if (contentRef.value) resizeObserver.observe(contentRef.value);
  scheduleMeasure();
});

onUnmounted(() => {
  resizeObserver?.disconnect();
});

watch(() => [props.speed, props.gap], scheduleMeasure);
</script>

<style scoped>
.overflow-marquee {
  width: 100%;
  min-width: 0;
  overflow: hidden;
  display: flex;
  align-items: center;
}

.overflow-marquee__group {
  flex: 0 0 auto;
  min-width: max-content;
  display: inline-flex;
  align-items: center;
  white-space: nowrap;
  padding-right: var(--overflow-marquee-gap);
  animation: overflow-marquee var(--overflow-marquee-duration) linear infinite;
}

@keyframes overflow-marquee {
  from {
    transform: translateX(0);
  }

  to {
    transform: translateX(-100%);
  }
}
</style>
