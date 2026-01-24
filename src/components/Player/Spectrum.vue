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
const isKeepDrawing = ref(true);

/**
 * 绘制音乐频谱图
 * @param {Array} data - 包含音频频谱数据的数组
 */
 const drawSpectrum = (data) => {
  // 提前退出条件：如果未提供数据或不需要继续绘制，则直接返回
  if (!data || !isKeepDrawing.value) return;

  // 获取画布和其2D上下文
  const canvas = canvasRef.value;
  const ctx = canvas.getContext("2d");
  
  // 计算画布的宽度，最大为1600或客户端视口宽度的最小值
  const canvasWidth = Math.min(1600, document.body.clientWidth);
  // 使用 props 中的高度设置画布高度
  const canvasHeight = props.height;
  // 计算需要绘制的柱状图数量 (2048 bins, 取一半显示)
  const numBars = Math.floor(spectrumsData.value.length / 10);
  // 计算每个柱状图的宽度
  const barWidth = canvasWidth / numBars / 2;
  // 获取圆角半径，从 props 中获取
  const cornerRadius = props.radius;

  // 设置画布的宽度和高度
  canvas.width = canvasWidth;
  canvas.height = canvasHeight;

  // 在绘制前清空画布
  ctx.clearRect(0, 0, canvasWidth, canvasHeight);

  // 设置柱状图的填充颜色
  ctx.fillStyle = "#efefef";

  // 遍历数据绘制柱状图
  for (let i = 0; i < numBars; i++) {
    // 计算柱状图的高度，从索引10开始跳过低频数据 (1024 FFT)
    const barHeight = (data[i + 10] / 255) * canvasHeight;
    
    // 计算柱状图的 x 和 y 坐标
    const x1 = i * barWidth + canvasWidth / 2; // 右侧柱状图的 x 坐标
    const x2 = canvasWidth / 2 - (i + 1) * barWidth; // 左侧柱状图的 x 坐标
    const y = canvasHeight - barHeight; // 柱状图的 y 坐标
    
    // 检查柱状图的高度是否大于0，避免绘制不可见的柱状图
    if (barHeight > 0) {
      // 绘制圆角矩形
      roundRect(ctx, x1, y, barWidth - 3, barHeight, cornerRadius);
      roundRect(ctx, x2, y, barWidth - 3, barHeight, cornerRadius);
    }
  }

  // 请求下一帧动画，继续绘制
  requestAnimationFrame(() => drawSpectrum(spectrumsData.value));
};

/**
 * 绘制圆角矩形
 * @param {CanvasRenderingContext2D} ctx - 2D上下文
 * @param {number} x - 矩形左上角 x 坐标
 * @param {number} y - 矩形左上角 y 坐标
 * @param {number} width - 矩形宽度
 * @param {number} height - 矩形高度
 * @param {number} radius - 圆角半径
 */
const roundRect = (ctx, x, y, width, height, radius) => {
  ctx.beginPath();
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
  ctx.fill();
};

onMounted(() => {
  drawSpectrum(spectrumsData.value);
});


onBeforeUnmount(() => {
  isKeepDrawing.value = false;
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
