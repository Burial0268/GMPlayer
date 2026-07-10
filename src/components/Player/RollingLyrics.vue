<template>
  <div :class="lyricClasses">
    <LyricPlayer class="am-lyric" @lrcTextClick="handleLrcTextClick" />
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { musicStore, settingStore } from "../../store";
import LyricPlayer from "../../libs/apple-music-like/LyricPlayer.vue";

const emit = defineEmits<{
  lrcTextClick: [time: number];
}>();

const music = musicStore();
const setting = settingStore();

// 直接处理从 LyricPlayer 传递的 lrcTextClick 事件
const handleLrcTextClick = (time: number) => {
  emit("lrcTextClick", time);
};

// 计算歌词容器的类名
const lyricClasses = computed(() => ({
  "lyric-am": true,
  "lyric-left": setting.lyricsPosition === "left",
  "lyric-center": setting.lyricsPosition === "center",
  loading: music.isLoadingSong,
}));
</script>

<style lang="scss" scoped>
.lyric-am {
  position: relative;
  width: 100%;
  height: 100%;
  overflow: hidden;
  filter: drop-shadow(0px 4px 6px rgba(0, 0, 0, 0.2));
  mask: linear-gradient(
    180deg,
    hsla(0, 0%, 100%, 0) 0,
    hsla(0, 0%, 100%, 0.6) 5%,
    #fff 10%,
    #fff 75%,
    hsla(0, 0%, 100%, 0.6) 85%,
    hsla(0, 0%, 100%, 0)
  );
  opacity: 1;
  transform: translateZ(0) scale(1);
  will-change: transform, opacity;
  transition:
    transform 0.5s cubic-bezier(0.34, 1.56, 0.64, 1),
    opacity 0.5s cubic-bezier(0.34, 1.56, 0.64, 1);

  @media (max-width: 768px) {
    height: 100%;
    min-height: 0;
    mask: none;
    -webkit-mask: none;
  }

  &.loading {
    opacity: 0;
    transform: scale(0.8);
  }

  &.lyric-left {
    :deep(.amll-lyric-player) {
      text-align: left;
    }

    :deep([class*="_lyricLine"]) {
      transform-origin: left center;
    }
  }

  &.lyric-center {
    :deep(.amll-lyric-player) {
      text-align: center;
    }

    :deep([class*="_lyricLine"]) {
      transform-origin: center;
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
  }
}
</style>
