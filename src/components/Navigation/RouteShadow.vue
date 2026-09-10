<template>
  <n-image
    class="route-shadow"
    :class="{ 'is-ready': ready }"
    :style="{
      '--shadow-intro-duration': `${layerMotion.content}s`,
      '--shadow-intro-ease': `cubic-bezier(${layerMotion.ease})`,
    }"
    :src="src"
    object-fit="cover"
    preview-disabled
    fallback-src="/images/pic/default.png"
    @load="loaded"
  />
</template>

<script setup lang="ts">
import { ref, watch } from "vue";
import { layerMotion } from "@/utils/navigation/motion";

const props = defineProps<{ src: string }>();
const ready = ref(false);
watch(
  () => props.src,
  () => {
    ready.value = false;
  },
);

const loaded = async (event: Event) => {
  const image = event.target as HTMLImageElement;
  const src = image.currentSrc;
  try {
    await image.decode();
    if (image.currentSrc === src) ready.value = true;
  } catch {
    // A replacement image owns the next load event.
  }
};
</script>

<style scoped>
.route-shadow {
  opacity: 0;
  transition: opacity var(--shadow-intro-duration) var(--shadow-intro-ease);
}
.route-shadow.is-ready {
  opacity: 1;
}
@media (prefers-reduced-motion: reduce) {
  .route-shadow {
    transition: none;
  }
}
</style>
