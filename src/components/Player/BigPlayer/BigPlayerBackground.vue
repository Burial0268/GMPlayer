<template>
  <Transition name="fade" mode="out-in">
    <div :key="`bg--${songPicGradient}`" :class="['overlay', backgroundImageShow]">
      <template v-if="backgroundImageShow === 'blur'">
        <BlurBackgroundRender v-if="hasPlayData" :fps="isPlaying ? (fps || 30) : 0"
          :playing="actualPlaying" :album="coverImageUrl" :blurLevel="blurAmount || 30"
          :saturation="contrastAmount || 1.2" :renderScale="renderScale || 0.5"
          class="blur-webgl" />
      </template>
    </div>
  </Transition>

  <template v-if="backgroundImageShow === 'eplor'">
    <BackgroundRender :fps="isPlaying ? fps : 0" :playing="actualPlaying"
      :flowSpeed="isPlaying ? flowSpeed : 0"
      :album="albumImageUrl === 'none' ? coverImageUrl : albumImageUrl"
      :renderScale="renderScale" :lowFreqVolume="lowFreqVolume" :staticMode="staticMode"
      class="overlay" />
  </template>

  <div :class="grayClasses" />
</template>

<script setup lang="ts">
import { computed } from "vue";
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
  lowFreqVolume: number;
  staticMode: boolean;
}>();

const isEplorOrBlurMode = computed(() =>
  props.backgroundImageShow === 'eplor' || props.backgroundImageShow === 'blur'
);

const grayClasses = computed(() => {
  const classes: string[] = ['gray'];
  if (props.backgroundImageShow === 'blur') classes.push('blur');
  if (isEplorOrBlurMode.value) classes.push('no-backdrop');
  return classes;
});
</script>

<style lang="scss" scoped>
.overlay {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  overflow: hidden;
  z-index: -2;
  transition: filter 0.5s ease;
  will-change: filter, opacity;

  &.solid {
    background: var(--cover-bg);
    transition: background 0.8s ease;
  }

  &::after {
    content: "";
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
  }

  &.blur {
    display: flex;
    align-items: center;
    justify-content: center;

    .overlay-img {
      width: 150%;
      height: 150%;
      filter: blur(80px) contrast(1.2);
      transition: filter 0.8s ease;
      will-change: filter, transform;
      animation: none !important;
    }

    .blur-webgl {
      position: absolute;
      width: 100%;
      height: 100%;
      top: 0;
      left: 0;
      overflow: hidden;
    }
  }

  // eplor/blur mode: transparent overlay::after
  &.eplor::after,
  &.blur::after {
    background-color: transparent !important;
  }

  &.none {
    &::after {
      display: none;
    }
  }
}

.gray {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  background-color: #00000030;
  -webkit-backdrop-filter: blur(80px);
  backdrop-filter: blur(80px);
  z-index: -1;
  transition: backdrop-filter 0.5s ease, background-color 0.5s ease;

  &.blur {
    background-color: #00000060;
  }

  // eplor/blur mode: disable backdrop
  &.no-backdrop {
    backdrop-filter: none !important;
    -webkit-backdrop-filter: none !important;
    transition: none !important;
    background-color: transparent !important;
  }
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
