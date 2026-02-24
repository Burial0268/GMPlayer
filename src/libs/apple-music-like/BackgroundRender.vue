<template>
  <div ref="wrapperRef" v-bind="$attrs"></div>
</template>

<script setup lang="ts">
// Thanks to Steven-xmh's `applemusic-like-lyrics` package.
import {
  BackgroundRender as CoreBackgroundRender,
  AbstractBaseRenderer,
  BaseRenderer,
  MeshGradientRenderer,
} from "@applemusic-like-lyrics/core";
import { ref, onMounted, onUnmounted, watch } from "vue";

interface BackgroundRenderProps {
  album?: string;
  fps?: number;
  playing?: boolean;
  flowSpeed?: number;
  hasLyric?: boolean;
  lowFreqVolume?: number;
  renderScale?: number;
  staticMode?: boolean;
  renderer?: { new (canvas: HTMLCanvasElement): BaseRenderer };
}

const props = withDefaults(defineProps<BackgroundRenderProps>(), {
  playing: true,
  hasLyric: true,
  lowFreqVolume: 1.0,
  renderScale: 0.5,
  staticMode: false,
});

const coreBGRenderRef = ref<AbstractBaseRenderer>();
const wrapperRef = ref<HTMLDivElement | null>(null);

onMounted(() => {
  coreBGRenderRef.value = CoreBackgroundRender.new(props.renderer ?? MeshGradientRenderer);

  // Set initial values
  if (props.album) coreBGRenderRef.value?.setAlbum(props.album);
  if (props.fps) coreBGRenderRef.value?.setFPS(props.fps);
  if (props.flowSpeed) coreBGRenderRef.value?.setFlowSpeed(props.flowSpeed);
  coreBGRenderRef.value?.setStaticMode(props.staticMode);
  coreBGRenderRef.value?.setRenderScale(props.renderScale);
  coreBGRenderRef.value?.setLowFreqVolume(props.lowFreqVolume);
  coreBGRenderRef.value?.setHasLyric(props.hasLyric);

  if (coreBGRenderRef.value) {
    const el = coreBGRenderRef.value.getElement();
    el.style.width = "100%";
    el.style.height = "100%";
    wrapperRef.value?.appendChild(el);
  }

  // Set initial playing state
  if (props.playing) {
    coreBGRenderRef.value?.resume();
  } else {
    coreBGRenderRef.value?.pause();
  }
});

onUnmounted(() => {
  coreBGRenderRef.value?.dispose();
});

watch(
  () => props.album,
  (newValue) => {
    if (newValue) coreBGRenderRef.value?.setAlbum(newValue);
  },
);

watch(
  () => props.fps,
  (newValue) => {
    if (typeof newValue !== "undefined") coreBGRenderRef.value?.setFPS(newValue);
  },
);

watch(
  () => props.playing,
  (newValue) => {
    if (newValue) {
      coreBGRenderRef.value?.resume();
    } else {
      coreBGRenderRef.value?.pause();
    }
  },
);

watch(
  () => props.flowSpeed,
  (newValue) => {
    if (typeof newValue !== "undefined") coreBGRenderRef.value?.setFlowSpeed(newValue);
  },
);

watch(
  () => props.staticMode,
  (newValue) => {
    coreBGRenderRef.value?.setStaticMode(newValue);
  },
);

watch(
  () => props.renderScale,
  (newValue) => {
    if (newValue) coreBGRenderRef.value?.setRenderScale(newValue);
  },
);

watch(
  () => props.lowFreqVolume,
  (newValue) => {
    if (typeof newValue !== "undefined") coreBGRenderRef.value?.setLowFreqVolume(newValue);
  },
);

watch(
  () => props.hasLyric,
  (newValue) => {
    if (newValue !== undefined) coreBGRenderRef.value?.setHasLyric(newValue);
  },
);

defineExpose({
  wrapperEl: wrapperRef,
  bgRender: coreBGRenderRef,
});
</script>
