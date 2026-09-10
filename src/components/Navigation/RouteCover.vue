<template>
  <div
    class="route-cover"
    :style="{
      '--artwork-fade-duration': `${layerMotion.artwork}s`,
      '--artwork-fade-ease': `cubic-bezier(${layerMotion.ease})`,
    }"
  >
    <img
      v-if="preview?.image"
      class="preview"
      data-navigation-artwork
      data-navigation-shared-image
      :src="preview.image"
      :style="{ opacity: revealed ? 0 : 1 }"
      alt=""
    />
    <n-image
      class="image"
      object-fit="cover"
      show-toolbar-tooltip
      :src="src"
      :preview-src="previewSrc"
      :preview-disabled="!revealed"
      :previewed-img-props="{ style: { borderRadius: 'var(--radius-md)' } }"
      fallback-src="/images/pic/default.png"
      :img-props="imageProps"
      @load="loaded"
      @error="failed = true"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, inject, ref, watch } from "vue";
import { useLayerNavigation } from "@/utils/navigation";
import { pageSourceKey } from "@/utils/navigation/sources";
import { layerMotion } from "@/utils/navigation/motion";

const props = defineProps<{ src: string; previewSrc: string }>();
const preview = inject(pageSourceKey, undefined);
const navigation = useLayerNavigation();
const decoded = ref(false);
const failed = ref(false);
const revealed = ref(false);
const imageProps = computed(() => ({
  "data-navigation-artwork": "",
  "data-navigation-shared-image": "",
  style: { opacity: revealed.value ? 1 : 0 },
}));

watch(
  () => props.src,
  () => {
    decoded.value = false;
    failed.value = false;
    revealed.value = false;
  },
);

const loaded = async (event: Event) => {
  const image = event.target as HTMLImageElement;
  const src = image.currentSrc;
  try {
    await image.decode();
    if (image.currentSrc === src) decoded.value = true;
  } catch {
    // A replaced request can finish loading after its image has been detached.
  }
};

watch(
  [decoded, navigation.isTransitioning, failed],
  () => {
    // Keep the same pixels under the moving proxy until its handoff has finished.
    if (decoded.value && !navigation.isTransitioning.value && !(failed.value && preview?.image)) {
      revealed.value = true;
    }
  },
  { flush: "post" },
);
</script>

<style scoped lang="scss">
.route-cover {
  position: absolute;
  inset: 0;
  isolation: isolate;
  .preview,
  .image {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .preview,
  :deep(.image img) {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: var(--radius-md);
    mix-blend-mode: plus-lighter;
    transition: opacity var(--artwork-fade-duration) var(--artwork-fade-ease);
  }
  @media (prefers-reduced-motion: reduce) {
    .preview,
    :deep(.image img) {
      transition: none;
    }
  }
}
</style>
