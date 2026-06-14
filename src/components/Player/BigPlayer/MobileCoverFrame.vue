<template>
  <Motion
    v-if="visible"
    :class="['mobile-cover-frame', { 'is-interactive': interactive }]"
    :layout="frameLayoutEnabled"
    :layout-id="frameLayoutId"
    :layout-dependency="layoutDependency"
    :transition="layoutTransition"
    :style="mergedFrameStyle"
    @click="$emit('click')"
  >
    <!-- Optional loading treatment. Disabled for player handoff so album art
         never appears to reload after a shared transition. -->
    <Transition name="shimmer-fade">
      <div v-if="loadAnimationEnabled && !imgLoaded" class="shimmer-overlay" />
    </Transition>

    <img
      :src="coverUrl"
      alt="cover"
      :class="{ loaded: imgLoaded }"
      loading="eager"
      decoding="async"
      @load="onImgLoad"
      @error="onImgError"
    />
  </Motion>
</template>

<script lang="ts">
const loadedCoverUrls = new Set<string>();
</script>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { Motion, type MotionValue } from "motion-v";

type MotionStyleRecord = Record<string, string | number | MotionValue | undefined>;

const props = defineProps<{
  visible: boolean;
  motionStyle?: MotionStyleRecord | null;
  coverUrl: string;
  layoutTransition: Record<string, unknown>;
  layoutDependency?: unknown;
  layoutEnabled?: boolean;
  layoutId?: string | null;
  borderRadius?: number;
  interactive?: boolean;
  loadAnimation?: boolean;
}>();

defineEmits<{
  click: [];
}>();

// ── Image-load state ──────────────────────────────────────────────────────────

/**
 * Becomes `true` once the current `coverUrl` image has fired a "load" event.
 * Reset to `false` whenever the URL changes so the shimmer re-appears for the
 * next artwork before it finishes downloading.
 */
const mergedFrameStyle = computed(() => {
  const style: MotionStyleRecord = {
    borderRadius: `${props.borderRadius ?? 12}px`,
  };
  if (props.motionStyle) Object.assign(style, props.motionStyle);
  return style;
});
const frameLayoutEnabled = computed(() => props.layoutEnabled ?? true);
const frameLayoutId = computed(() =>
  props.layoutId === null ? undefined : props.layoutId ?? "splayer-mobile-album",
);
const loadAnimationEnabled = computed(() => props.loadAnimation ?? false);
const imgLoaded = ref(!loadAnimationEnabled.value || loadedCoverUrls.has(props.coverUrl));

watch(
  [() => props.coverUrl, loadAnimationEnabled],
  ([newUrl]) => {
    imgLoaded.value = !loadAnimationEnabled.value || loadedCoverUrls.has(newUrl);
  },
);

function onImgLoad(): void {
  loadedCoverUrls.add(props.coverUrl);
  imgLoaded.value = true;
}

/** Treat a broken image the same as a successful load — hide the shimmer. */
function onImgError(): void {
  loadedCoverUrls.add(props.coverUrl);
  imgLoaded.value = true;
}
</script>

<style lang="scss" scoped>
.mobile-cover-frame {
  position: relative;
  width: 100%;
  height: 100%;
  overflow: hidden;
  cursor: pointer;
  pointer-events: auto;
  z-index: 1;
  box-shadow: 0px 12px 40px rgba(0, 0, 0, 0.35);
  will-change: transform, width, height, left, top, border-radius;

  &.is-interactive {
    position: absolute;
    width: 0px;
    height: 0px;
    z-index: 60;
  }

  &:active {
    opacity: 0.9;
  }

  img {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;

    // Start fully transparent; fade in once loaded so there is never a
    // jarring "snap" where the old artwork is replaced by a blank frame.
    opacity: 0;
    transition: opacity 0.35s ease;

    &.loaded {
      opacity: 1;
    }
  }

  // ── Shimmer overlay ─────────────────────────────────────────────────────────

  .shimmer-overlay {
    position: absolute;
    inset: 0;
    z-index: 1;

    // Dark base so the shimmer is visible regardless of the player background
    background-color: rgba(255, 255, 255, 0.06);

    // Travelling highlight wave
    &::after {
      content: "";
      position: absolute;
      inset: 0;
      background: linear-gradient(
        105deg,
        transparent 30%,
        rgba(255, 255, 255, 0.14) 50%,
        transparent 70%
      );
      background-size: 200% 100%;
      animation: shimmer-sweep 1.5s ease-in-out infinite;
    }
  }

  // ── Shimmer fade-out transition ─────────────────────────────────────────────

  .shimmer-fade-leave-active {
    transition: opacity 0.4s ease;
  }

  .shimmer-fade-leave-to {
    opacity: 0;
  }
}

@keyframes shimmer-sweep {
  0% {
    background-position: 200% 0;
  }
  100% {
    background-position: -200% 0;
  }
}
</style>
