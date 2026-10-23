<template>
  <div
    ref="root"
    class="route-artwork"
    :class="{ 'is-ready': ready }"
    :style="{
      '--artwork-intro-duration': `${layerMotion.content}s`,
      '--artwork-intro-ease': `cubic-bezier(${layerMotion.ease})`,
    }"
    @load.capture="loaded"
  >
    <slot :imageProps="imageProps" />
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref, type ImgHTMLAttributes } from "vue";
import { layerMotion } from "@/utils/navigation/motion";

const root = ref<HTMLElement>();
const ready = ref(false);
const imageProps: ImgHTMLAttributes & { "data-navigation-artwork": string } = {
  "data-navigation-artwork": "",
};

const reveal = async (image: HTMLImageElement) => {
  if (ready.value) return;
  const src = image.currentSrc;
  try {
    await image.decode();
    if (ready.value || image.currentSrc !== src || !root.value?.contains(image)) return;
    // Commit the transparent glow before a cached decode can reveal it in the same frame.
    getComputedStyle(root.value).getPropertyValue("filter");
    ready.value = true;
  } catch {
    // A replacement or fallback image owns the next load event.
  }
};

const loaded = (event: Event) => {
  const image = event.target;
  if (image instanceof HTMLImageElement && image.hasAttribute("data-navigation-artwork")) {
    void reveal(image);
  }
};

onMounted(() => {
  root.value
    ?.querySelectorAll<HTMLImageElement>("img[data-navigation-artwork]")
    .forEach((image) => {
      if (image.complete && image.naturalWidth) void reveal(image);
    });
});
</script>

<style scoped>
.route-artwork {
  --artwork-glow-alpha: 0;
  filter: drop-shadow(
    0 16px 28px rgba(var(--content-panel-accent-rgb, 0, 0, 0), var(--artwork-glow-alpha))
  );
  transition:
    transform var(--duration-300) var(--ease-out),
    filter var(--artwork-intro-duration) var(--artwork-intro-ease);
}
.route-artwork.is-ready {
  --artwork-glow-alpha: 0.22;
}
.route-surface[data-navigation-cover-hidden] .route-artwork {
  --artwork-glow-alpha: 0;
  transition-duration: var(--duration-300), 0s;
}
@media (prefers-reduced-motion: reduce) {
  .route-artwork {
    transition: none;
  }
}
</style>
