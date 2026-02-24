<template>
  <div ref="wrapperRef" v-bind="$attrs">
    <canvas ref="canvasRef" style="width: 100%; height: 100%" />
  </div>
</template>

<script lang="ts">
import type { ExtractPublicPropTypes, PropType, Ref, ShallowRef } from "vue";
import type { AbstractBaseRenderer, BaseRenderer } from "@applemusic-like-lyrics/core";

export interface BackgroundRenderRef {
  /**
   * 背景渲染实例引用
   */
  bgRender?: Ref<AbstractBaseRenderer | undefined>;
  /**
   * 将背景渲染实例的元素包裹起来的 DIV 元素实例
   */
  wrapperEl: Readonly<ShallowRef<HTMLDivElement | null>>;
}

export const backgroundRenderProps = {
  /**
   * 设置背景专辑资源
   */
  album: {
    type: [String, Object] as PropType<string | HTMLImageElement | HTMLVideoElement>,
    required: false,
  },
  /**
   * 设置专辑资源是否为视频
   */
  albumIsVideo: {
    type: Boolean,
    required: false,
  },
  /**
   * 设置当前背景动画帧率，如果为 `undefined` 则默认为 `30`
   */
  fps: {
    type: Number,
    required: false,
  },
  /**
   * 设置当前播放状态，如果为 `undefined` 则默认为 `true`
   */
  playing: {
    type: Boolean,
    required: false,
  },
  /**
   * 设置当前动画流动速度，如果为 `undefined` 则默认为 `2`
   */
  flowSpeed: {
    type: Number,
    required: false,
  },
  /**
   * 设置背景是否根据"是否有歌词"这个特征调整自身效果，例如有歌词时会变得更加活跃
   *
   * 部分渲染器会根据这个特征调整自身效果
   *
   * 如果不确定是否需要赋值或无法知晓是否包含歌词，请传入 true 或不做任何处理（默认值为 true）
   */
  hasLyric: {
    type: Boolean,
    required: false,
  },
  /**
   * 设置低频的音量大小，范围在 80hz-120hz 之间为宜，取值范围在 [0.0-1.0] 之间
   *
   * 部分渲染器会根据音量大小调整背景效果（例如根据鼓点跳动）
   *
   * 如果无法获取到类似的数据，请传入 undefined 或 1.0 作为默认值，或不做任何处理（默认值即 1.0）
   */
  lowFreqVolume: {
    type: Number,
    required: false,
  },
  /**
   * 设置当前渲染缩放比例，如果为 `undefined` 则默认为 `0.5`
   */
  renderScale: {
    type: Number,
    required: false,
  },
  /**
   * 设置渲染器，如果为 `undefined` 则默认为 `MeshGradientRenderer`
   * 默认渲染器有可能会随着版本更新而更换
   */
  renderer: {
    type: Object as PropType<{
      new (...args: ConstructorParameters<typeof BaseRenderer>): BaseRenderer;
    }>,
    required: false,
  },
  staticMode: {
    type: Boolean,
    required: false,
  },
  blurLevel: {
    type: Number,
    required: false,
  },
  saturation: {
    type: Number,
    required: false,
  },
} as const;

export type BackgroundRenderProps = ExtractPublicPropTypes<typeof backgroundRenderProps>;
</script>

<script setup lang="ts">
import { FbmRenderer } from "../../libs/fbm-renderer/fbm-renderer";
import { onMounted, onUnmounted, ref, watchEffect } from "vue";

const canvasRef = ref<HTMLCanvasElement | null>(null);
const wrapperRef = ref<HTMLDivElement | null>(null);
const bgRenderRef = ref<FbmRenderer>();
const isStarted = ref(false);

const props = defineProps(backgroundRenderProps);

const resizeCanvas = () => {
  if (canvasRef.value && wrapperRef.value) {
    canvasRef.value.width = wrapperRef.value.clientWidth;
    canvasRef.value.height = wrapperRef.value.clientHeight;
  }
};

onMounted(() => {
  if (wrapperRef.value && canvasRef.value) {
    resizeCanvas();
    window.addEventListener("resize", resizeCanvas);
    bgRenderRef.value = new FbmRenderer(canvasRef.value, {
      rotationSpeed: 0.5,
      blurLevel: props.blurLevel ?? 5.8,
      saturation: props.saturation ?? 1.0,
    });
  }
});

onUnmounted(() => {
  window.removeEventListener("resize", resizeCanvas);
  if (bgRenderRef.value) {
    bgRenderRef.value.dispose();
  }
});

watchEffect(async () => {
  if (bgRenderRef.value && props.album) {
    await bgRenderRef.value.setAlbum(props.album, props.albumIsVideo);
    if (!isStarted.value) {
      bgRenderRef.value.start();
      isStarted.value = true;
    }
  }
});

watchEffect(() => {
  if (bgRenderRef.value && props.fps) bgRenderRef.value.setFPS(props.fps);
});

watchEffect(() => {
  if (!bgRenderRef.value) return;
  if (props.playing) {
    bgRenderRef.value.resume();
  } else {
    bgRenderRef.value.pause();
  }
});

watchEffect(() => {
  if (bgRenderRef.value && props.flowSpeed) bgRenderRef.value.setFlowSpeed(props.flowSpeed);
});

watchEffect(() => {
  if (bgRenderRef.value && props.renderScale)
    bgRenderRef.value.setRenderScale(props.renderScale ?? 0.5);
});

watchEffect(() => {
  if (bgRenderRef.value && props.lowFreqVolume)
    bgRenderRef.value.setLowFreqVolume(props.lowFreqVolume ?? 1.0);
});

watchEffect(() => {
  if (bgRenderRef.value) bgRenderRef.value.setHasLyric(props.hasLyric ?? true);
});

watchEffect(() => {
  if (bgRenderRef.value && props.blurLevel !== undefined)
    bgRenderRef.value.setBlur(props.blurLevel);
});

watchEffect(() => {
  if (bgRenderRef.value && props.saturation !== undefined)
    bgRenderRef.value.setSaturation(props.saturation);
});
</script>
