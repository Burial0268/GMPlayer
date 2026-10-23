<template>
  <div class="big-player-background">
    <div :class="['overlay', backgroundImageShow]">
      <template v-if="backgroundImageShow === 'blur'">
        <BlurBackgroundRender
          v-if="hasPlayData"
          :fps="fps || 30"
          :playing="backgroundPlaying"
          :album="coverImageUrl"
          :blurLevel="blurAmount || 30"
          :saturation="contrastAmount || 1.2"
          :renderScale="renderScale || 0.5"
          class="blur-webgl"
        />
      </template>
    </div>

    <template v-if="backgroundImageShow === 'eplor'">
      <BackgroundRender
        ref="backgroundRender"
        :fps="fps"
        :playing="backgroundPlaying"
        :flowSpeed="flowSpeed"
        :album="albumImageUrl === 'none' ? coverImageUrl : albumImageUrl"
        :renderScale="renderScale"
        :staticMode="staticMode"
        class="overlay"
      />
    </template>

    <div v-if="!isEplorOrBlurMode" :class="grayClasses" :style="grayStyles" />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, useTemplateRef } from "vue";
import { settingStore } from "@/store";
import { subscribeLowFreqVolume } from "@/utils/AudioContext/lowFreqVolume";
import BlurBackgroundRender from "../BlurBackgroundRender.vue";
import BackgroundRender from "@/libs/apple-music-like/BackgroundRender.vue";

const props = defineProps<{
  songPicGradient: string;
  backgroundImageShow: string;
  hasPlayData: boolean;
  isPlaying: boolean;
  actualPlaying: boolean;
  fps: number;
  blurAmount: number;
  contrastAmount: number;
  renderScale: number;
  coverImageUrl: string;
  albumImageUrl: string;
  flowSpeed: number;
  staticMode: boolean;
}>();

const setting = settingStore();

// Low-frequency volume is deliberately kept OUT of the render graph and out of
// reactive state entirely — see `@/utils/AudioContext/lowFreqVolume` for why.
//
// In short: the producer runs on requestAnimationFrame (so up to 144Hz on a
// high-refresh panel) and the only consumer is amll-core's `setLowFreqVolume`,
// a plain float assignment that the renderer samples from its own rAF loop.
// Binding it as a prop made every analysis frame invalidate the component that
// hosts the WebGL canvas wrapper, queueing vnode work against the renderer's
// own tick — which is what caused the stutter.
//
// Subscribing to the plain signal gives the renderer identical data with zero
// reactivity and zero render-graph work, and propagates a real jump on the very
// next frame with no scheduler hop.
const backgroundRenderRef = useTemplateRef<{
  bgRender?: { setLowFreqVolume?: (v: number) => void };
}>("backgroundRender");

let unsubscribeLowFreq: (() => void) | null = null;

onMounted(() => {
  unsubscribeLowFreq = subscribeLowFreqVolume((value) => {
    backgroundRenderRef.value?.bgRender?.setLowFreqVolume?.(setting.dynamicFlowSpeed ? value : 1.0);
  });
});

onUnmounted(() => {
  unsubscribeLowFreq?.();
  unsubscribeLowFreq = null;
});

const isEplorOrBlurMode = computed(
  () => props.backgroundImageShow === "eplor" || props.backgroundImageShow === "blur",
);
const backgroundPlaying = computed(() => !props.staticMode);

const grayClasses = computed(() => {
  const classes: string[] = ["gray"];
  if (props.backgroundImageShow) classes.push(props.backgroundImageShow);
  return classes;
});

const grayStyles = computed(() => ({
  backgroundColor: "#00000030",
  WebkitBackdropFilter: "blur(80px)",
  backdropFilter: "blur(80px)",
  transition:
    "backdrop-filter var(--duration-500) var(--ease-out), background-color var(--duration-500) var(--ease-out)",
}));
</script>

<style lang="scss" scoped>
.big-player-background {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  overflow: hidden;
  z-index: -2;
  pointer-events: none;
}

// 画布外层不要挂 filter/opacity 的 will-change 或 filter 过渡：
// 那会把 WebGL 画布强制压进离屏合成层，重采样会抹掉渲染器的抖动、放大色带。
.overlay {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  overflow: hidden;
  z-index: 0;

  &.solid {
    background: var(--cover-bg);
    transition: background 0.8s ease;
  }

  &.blur {
    display: flex;
    align-items: center;
    justify-content: center;

    .blur-webgl {
      position: absolute;
      width: 100%;
      height: 100%;
      top: 0;
      left: 0;
      overflow: hidden;
    }
  }
}

.gray {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  z-index: 1;
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity var(--duration-300) cubic-bezier(0.34, 1.56, 0.64, 1);
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
