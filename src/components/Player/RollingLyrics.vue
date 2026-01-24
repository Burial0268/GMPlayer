<template>
  <Transition>
    <div :key="currentLyrics?.startTime" 
         :class="lyricClasses">
      <LyricPlayer class="am-lyric" @line-click="handleLineClick" @lrcTextClick="handleLrcTextClick" />
    </div>
  </Transition>
</template>

<script setup lang="ts">
import { computed, watch } from 'vue';
import { musicStore, settingStore } from "../../store";
import LyricPlayer from "../../libs/apple-music-like/LyricPlayer.vue";
import { getProcessedLyrics, preprocessLyrics } from "../../libs/apple-music-like/processLyrics";

const emit = defineEmits<{
  lrcTextClick: [time: number]
}>();

const music = musicStore();
const setting = settingStore();

// 处理歌词点击
const handleLineClick = (e: { line: { getLine: () => { startTime: number } } }) => {
  const time = e.line.getLine().startTime;
  emit("lrcTextClick", time / 1000);
};

// 直接处理从 LyricPlayer 传递的 lrcTextClick 事件
const handleLrcTextClick = (time: number) => {
  emit("lrcTextClick", time);
};

// 计算歌词容器的类名
const lyricClasses = computed(() => ({
  'lyric-am': true,
  'lyric-left': setting.lyricsPosition === 'left',
  'lyric-center': setting.lyricsPosition === 'center',
  'loading': music.isLoadingSong
}));

// 监听设置变化，预处理歌词数据
watch([
  () => setting.showYrc, 
  () => setting.showRoma, 
  () => setting.showTransl
], () => {
  // 设置变化时预处理歌词数据
  if (music.songLyric && Object.keys(music.songLyric).length > 0) {
    console.log('[RollingLyrics] 设置变化，重新预处理歌词');
    preprocessLyrics(music.songLyric, {
      showYrc: setting.showYrc,
      showRoma: setting.showRoma,
      showTransl: setting.showTransl
    });
  }
}, { immediate: true });

// 获取当前行，增强错误处理以提高组件稳定性
const currentLyrics = computed(() => {
  const songLyric = music.songLyric;
  
  // 检查歌词数据是否有效
  if (!songLyric || 
      (!songLyric.lrcAMData?.length && 
       !songLyric.yrcAMData?.length && 
       !songLyric.ttml?.length)) {
    return null;
  }
  
  try {
    // 使用优化后的处理函数，利用缓存提高性能
    const processedLyrics = getProcessedLyrics(songLyric, {
      showYrc: setting.showYrc,
      showRoma: setting.showRoma,
      showTransl: setting.showTransl
    });
    
    return processedLyrics && processedLyrics.length > 0 ? processedLyrics[0] : null;
  } catch (error) {
    console.error('[RollingLyrics] 处理歌词时发生错误:', error);
    return null;
  }
});
</script>

<style lang="scss" scoped>
.lyric-am {
  position: relative;
  width: 100%;
  height: 100%;
  overflow: hidden;
  filter: drop-shadow(0px 4px 6px rgba(0, 0, 0, 0.2));
  mask: linear-gradient(180deg,
      hsla(0, 0%, 100%, 0) 0,
      hsla(0, 0%, 100%, 0.6) 5%,
      #fff 10%,
      #fff 75%,
      hsla(0, 0%, 100%, 0.6) 85%,
      hsla(0, 0%, 100%, 0));
  opacity: 1;
  transform: translateZ(0) scale(1);
  will-change: transform, opacity;
  transition: transform 0.5s cubic-bezier(0.34, 1.56, 0.64, 1),
              opacity 0.5s cubic-bezier(0.34, 1.56, 0.64, 1);

  @media (max-width: 768px) {
    height: 70vh;
  }

  &.loading {
    opacity: 0;
    transform: scale(0.8);
  }

  &.lyric-left {
    :deep(.am-lyric) {
      text-align: left;

      div {
        transform-origin: left center;
      }
    }
  }

  &.lyric-center {
    :deep(.am-lyric) {
      text-align: center;

      div {
        transform-origin: center;
      }
    }
  }

  :deep(.am-lyric) {
    width: 100%;
    height: 100%;
    position: absolute;
    left: 0;
    top: 0;
    font-synthesis: none;
    text-rendering: optimizeLegibility;

    @media (max-width: 768px) {
      position: relative;
      padding: 20px 0;
    }
  }
}

.v-enter-active,
.v-leave-active {
  transition: opacity 0.3s ease, transform 0.5s cubic-bezier(0.34, 1.56, 0.64, 1);
}

.v-enter-from,
.v-leave-to {
  opacity: 0;
  transform: scale(0.8);
}
</style>
