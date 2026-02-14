<!-- 播放器 - 音乐频谱 -->
<template>
  <div :style="{ opacity: show ? '0.6' : '0.1' }" class="spectrum">
    <canvas ref="canvasRef" :style="{ height: height + 'px' }" class="spectrum-line" />
  </div>
</template>

<script setup>
import { storeToRefs } from "pinia";
import { musicStore } from "@/store";

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

const { spectrumsData } = storeToRefs(musicStore());

// canvas
const canvasRef = ref(null);

// Cached canvas dimensions — only update on resize, not every frame
let cachedWidth = 0;
let cachedHeight = 0;

const updateCanvasSize = () => {
  const canvas = canvasRef.value;
  if (!canvas) return;
  const w = Math.min(1600, document.body.clientWidth);
  const h = props.height;
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
  if (!data || !data.length) return;

  const canvas = canvasRef.value;
  if (!canvas) return;
  const ctx = canvas.getContext("2d");

  const canvasWidth = cachedWidth;
  const canvasHeight = cachedHeight;
  const numBars = Math.floor(data.length / 10);
  const barWidth = canvasWidth / numBars / 2;
  const cornerRadius = props.radius;

  ctx.clearRect(0, 0, canvasWidth, canvasHeight);
  ctx.fillStyle = "#efefef";

  // Batch all bars into a single path to reduce draw calls
  ctx.beginPath();
  for (let i = 0; i < numBars; i++) {
    const barHeight = (data[i + 10] / 255) * canvasHeight;
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

// Managed RAF loop — only runs when BigPlayer is visible (show === true)
let rafId = null;

const startDraw = () => {
  if (rafId) return;
  const loop = () => {
    drawSpectrum(spectrumsData.value);
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

// ResizeObserver to track canvas size changes
let resizeObserver = null;

// Control RAF loop based on visibility
watch(() => props.show, (visible) => {
  if (visible) {
    updateCanvasSize();
    startDraw();
  } else {
    stopDraw();
  }
});

onMounted(() => {
  updateCanvasSize();

  // Watch for container/window resize
  if (typeof ResizeObserver !== 'undefined' && canvasRef.value?.parentElement) {
    resizeObserver = new ResizeObserver(updateCanvasSize);
    resizeObserver.observe(canvasRef.value.parentElement);
  }

  // Only start drawing if already visible
  if (props.show) {
    startDraw();
  }
});

onBeforeUnmount(() => {
  stopDraw();
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
  transition: opacity 0.3s;
  mask: linear-gradient(90deg,
      hsla(0, 0%, 100%, 0) 0,
      hsla(0, 0%, 100%, 0.6) 10%,
      #fff 15%,
      #fff 85%,
      hsla(0, 0%, 100%, 0.6) 90%,
      hsla(0, 0%, 100%, 0));
  -webkit-mask: linear-gradient(90deg,
      hsla(0, 0%, 100%, 0) 0,
      hsla(0, 0%, 100%, 0.6) 10%,
      #fff 15%,
      #fff 85%,
      hsla(0, 0%, 100%, 0.6) 90%,
      hsla(0, 0%, 100%, 0));

  .spectrum-line {
    mask: linear-gradient(90deg,
        hsla(0, 0%, 100%, 0) 0,
        hsla(0, 0%, 100%, 0.6) 5%,
        #fff 10%,
        #fff 90%,
        hsla(0, 0%, 100%, 0.6) 95%,
        hsla(0, 0%, 100%, 0));
    -webkit-mask: linear-gradient(90deg,
        hsla(0, 0%, 100%, 0) 0,
        hsla(0, 0%, 100%, 0.6) 5%,
        #fff 10%,
        #fff 90%,
        hsla(0, 0%, 100%, 0.6) 95%,
        hsla(0, 0%, 100%, 0));
  }
}
</style>
