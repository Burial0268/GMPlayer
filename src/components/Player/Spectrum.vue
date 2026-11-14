<!-- 播放器 - 音乐频谱 -->
<template>
  <div :style="{ opacity: show ? '0.6' : '0.1' }" class="spectrum">
    <canvas ref="canvasRef" :style="{ height: height + 'px' }" class="spectrum-line" />
  </div>
</template>

<script setup>
import { ensureSpectrumUpdate } from "@/utils/AudioContext";
import { getSpectrumFrame, getSpectrumVersion } from "@/utils/AudioContext/analysis/SpectrumFrame";

const props = defineProps({
  show: {
    type: Boolean,
    default: true,
  },
  height: {
    type: Number,
    default: 60,
  },
  barWidth: {
    type: Number,
    default: 4,
  },
  radius: {
    type: Number,
    default: 2.5,
  },
});

// canvas
const canvasRef = ref(null);

// Cached canvas dimensions — only update on resize, not every frame
let cachedWidth = 0;
let cachedHeight = 0;
let displayPeak = 255;
// getContext() is re-entrant but not free; the loop runs every frame, so keep
// the 2D context alongside the size cache instead of re-fetching it per draw.
let ctx = null;

const updateCanvasSize = () => {
  const canvas = canvasRef.value;
  if (!canvas) return;
  const w = Math.min(1600, document.body.clientWidth);
  const h = props.height;
  if (!ctx) ctx = canvas.getContext("2d");
  if (cachedWidth !== w || cachedHeight !== h) {
    cachedWidth = w;
    cachedHeight = h;
    canvas.width = w;
    canvas.height = h;
  }
};

/**
 * 绘制音乐频谱图
 * @param {Array} data - 包含音频频谱数据的数组
 */
const drawSpectrum = (data) => {
  const canvas = canvasRef.value;
  if (!canvas) return;
  if (!ctx) ctx = canvas.getContext("2d");
  if (!ctx) return;
  if (!data || !data.length) {
    // Frame was cleared (pause/track change) — wipe the canvas once instead of
    // leaving stale bars behind.
    ctx.clearRect(0, 0, cachedWidth, cachedHeight);
    return;
  }

  const canvasWidth = cachedWidth;
  const canvasHeight = cachedHeight;
  const rawBinCount = data.length;
  const numBars = Math.max(
    1,
    Math.min(rawBinCount, Math.floor(canvasWidth / (props.barWidth * 2))),
  );
  const barWidth = canvasWidth / numBars / 2;
  const cornerRadius = props.radius;
  let framePeak = 0;
  for (let i = 0; i < rawBinCount; i++) {
    if (data[i] > framePeak) framePeak = data[i];
  }
  if (framePeak <= 0) {
    ctx.clearRect(0, 0, canvasWidth, canvasHeight);
    return;
  }
  if (framePeak > displayPeak) {
    displayPeak = displayPeak * 0.55 + framePeak * 0.45;
  } else {
    displayPeak = Math.max(255, displayPeak * 0.96);
  }

  ctx.clearRect(0, 0, canvasWidth, canvasHeight);
  ctx.fillStyle = "#efefef";

  // Batch all bars into a single path. Each visual bar is a peak-reduced
  // range over the whole frame, so the backend's 512-value display frame keeps its
  // precision instead of only drawing the first ~200 bins.
  ctx.beginPath();
  for (let i = 0; i < numBars; i++) {
    const start = Math.floor((i / numBars) * rawBinCount);
    const end = Math.max(start + 1, Math.floor(((i + 1) / numBars) * rawBinCount));
    let value = 0;
    for (let j = start; j < end; j++) {
      if (data[j] > value) value = data[j];
    }
    const barHeight = Math.min(1, value / displayPeak) * canvasHeight;
    if (barHeight <= 0) continue;

    const x1 = i * barWidth + canvasWidth / 2;
    const x2 = canvasWidth / 2 - (i + 1) * barWidth;
    const y = canvasHeight - barHeight;
    const w = barWidth - 3;

    // Use native roundRect if available, otherwise fallback
    addRoundRect(ctx, x1, y, w, barHeight, cornerRadius);
    addRoundRect(ctx, x2, y, w, barHeight, cornerRadius);
  }
  ctx.fill();
};

/**
 * 添加圆角矩形路径（不立即 fill，批量绘制）
 */
const addRoundRect = (ctx, x, y, width, height, radius) => {
  ctx.moveTo(x + radius, y);
  ctx.lineTo(x + width - radius, y);
  ctx.quadraticCurveTo(x + width, y, x + width, y + radius);
  ctx.lineTo(x + width, y + height - radius);
  ctx.quadraticCurveTo(x + width, y + height, x + width - radius, y + height);
  ctx.lineTo(x + radius, y + height);
  ctx.quadraticCurveTo(x, y + height, x, y + height - radius);
  ctx.lineTo(x, y + radius);
  ctx.quadraticCurveTo(x, y, x + radius, y);
  ctx.closePath();
};

// Managed RAF loop. Keep it alive while mounted so opening BigPlayer can draw
// the latest frame immediately after the user toggles spectrum display.
// Two gates keep the loop from burning CPU when its output cannot be seen:
// - version gate: skip the canvas work when no new spectrum frame arrived
//   (paused / analysis stopped / background tab)
// - show gate: BigPlayer slides off-screen via translateY(100%) when closed,
//   taking this fixed-position canvas with it — after the slide transition
//   settles there is nothing visible to draw, so the loop parks entirely
//   until `show` flips back.
let rafId = null;
let lastDrawnVersion = -1;
let hideParkTimer = null;
const HIDE_PARK_DELAY_MS = 1000; // outlasts the 0.5s close transition

const startDraw = () => {
  if (rafId) return;
  const loop = () => {
    const version = getSpectrumVersion();
    if (version !== lastDrawnVersion) {
      lastDrawnVersion = version;
      drawSpectrum(getSpectrumFrame());
    }
    rafId = requestAnimationFrame(loop);
  };
  loop();
};

const stopDraw = () => {
  if (rafId) {
    cancelAnimationFrame(rafId);
    rafId = null;
  }
};

const clearHideParkTimer = () => {
  if (hideParkTimer) {
    clearTimeout(hideParkTimer);
    hideParkTimer = null;
  }
};

// ResizeObserver to track canvas size changes
let resizeObserver = null;

watch(
  () => props.show,
  (visible) => {
    updateCanvasSize();
    ensureSpectrumUpdate();
    clearHideParkTimer();
    if (visible) {
      lastDrawnVersion = -1; // force an immediate redraw of the latest frame
      startDraw();
    } else {
      hideParkTimer = setTimeout(() => {
        hideParkTimer = null;
        stopDraw();
      }, HIDE_PARK_DELAY_MS);
    }
  },
);

onMounted(() => {
  updateCanvasSize();
  ensureSpectrumUpdate();

  // Watch for container/window resize
  if (typeof ResizeObserver !== "undefined" && canvasRef.value?.parentElement) {
    resizeObserver = new ResizeObserver(updateCanvasSize);
    resizeObserver.observe(canvasRef.value.parentElement);
  }

  // Off-screen at mount (player closed) — the show watcher starts the loop.
  if (props.show) {
    startDraw();
  }
});

onBeforeUnmount(() => {
  clearHideParkTimer();
  stopDraw();
  ctx = null;
  if (resizeObserver) {
    resizeObserver.disconnect();
    resizeObserver = null;
  }
});
</script>

<style lang="scss" scoped>
.spectrum {
  z-index: 10000; // 拉高层级
  position: fixed;
  left: 0;
  bottom: 0;
  width: 100%;
  display: flex;
  flex-direction: row;
  justify-content: center;
  opacity: 0.6;
  pointer-events: none;
  transition: opacity var(--duration-300) var(--ease-out);
  mask: linear-gradient(
    90deg,
    hsla(0, 0%, 100%, 0) 0,
    hsla(0, 0%, 100%, 0.6) 10%,
    #fff 15%,
    #fff 85%,
    hsla(0, 0%, 100%, 0.6) 90%,
    hsla(0, 0%, 100%, 0)
  );
  -webkit-mask: linear-gradient(
    90deg,
    hsla(0, 0%, 100%, 0) 0,
    hsla(0, 0%, 100%, 0.6) 10%,
    #fff 15%,
    #fff 85%,
    hsla(0, 0%, 100%, 0.6) 90%,
    hsla(0, 0%, 100%, 0)
  );

  .spectrum-line {
    mask: linear-gradient(
      90deg,
      hsla(0, 0%, 100%, 0) 0,
      hsla(0, 0%, 100%, 0.6) 5%,
      #fff 10%,
      #fff 90%,
      hsla(0, 0%, 100%, 0.6) 95%,
      hsla(0, 0%, 100%, 0)
    );
    -webkit-mask: linear-gradient(
      90deg,
      hsla(0, 0%, 100%, 0) 0,
      hsla(0, 0%, 100%, 0.6) 5%,
      #fff 10%,
      #fff 90%,
      hsla(0, 0%, 100%, 0.6) 95%,
      hsla(0, 0%, 100%, 0)
    );
  }
}
</style>
