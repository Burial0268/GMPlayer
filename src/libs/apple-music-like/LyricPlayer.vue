<template>
  <LyricPlayer 
    ref="lyricPlayerRef" 
    :lyricLines="currentLyrics" 
    :currentTime="currentTime"
    :playing="music.playState"
    :alignAnchor="alignAnchor"
    :alignPosition="alignPosition" 
    :enableSpring="setting.showYrcAnimation"
    :enableScale="setting.showYrcAnimation" 
    :enableBlur="setting.lyricsBlur"
    :enableInterludeDots="true"
    :wordFadeWidth="0.5" 
    :linePosXSpringParams="setting.springParams.posX"
    :linePosYSpringParams="setting.springParams.posY" 
    :lineScaleSpringParams="setting.springParams.scale" 
    :style="lyricStyles"
    @line-click="handleLineClick" 
  />
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { musicStore, settingStore, siteStore } from "../../store";
import { LyricPlayer, type LyricPlayerRef } from "@applemusic-like-lyrics/vue";
import { createLyricsProcessor, type LyricLine } from "./processLyrics";

const lyricPlayerRef = ref<LyricPlayerRef>();
const site = siteStore();
const music = musicStore();
const setting = settingStore();

const emit = defineEmits<{
  'line-click': [e: { line: { getLine: () => { startTime: number } } }],
  lrcTextClick: [time: number]
}>();

// 计算当前播放时间
const currentTime = computed(() => 
  music.persistData.playSongTime.currentTime * 1000
);

// 计算对齐方式
const alignAnchor = computed(() => 
  setting.lyricsBlock === 'center' ? 'center' : 'top'
);

const alignPosition = computed(() => 
  setting.lyricsBlock === 'center' ? 0.5 : 0.2
);

// 计算歌词样式
const lyricStyles = computed(() => ({
  '--amll-lp-color': mainColor.value,
  '--amll-lyric-player-font-size': `${setting.lyricsFontSize * 11}px`,
  '--amll-lp-height': setting.lyricLineHeight,
  'font-weight': setting.lyricFontWeight,
  'font-family': setting.lyricFont,
  'letter-spacing': setting.lyricLetterSpacing,
  'cursor': 'pointer',
  '--amll-lyric-view-color': mainColor.value,
  'user-select': 'none',
  '-webkit-tap-highlight-color': 'transparent'
}));

// 处理歌词点击
const handleLineClick = (e: { line: { getLine: () => { startTime: number } } }) => {
  const time = e.line.getLine().startTime;
  if (time != null) {
    emit("lrcTextClick", time / 1000);
    emit("line-click", e); // 同时发送原始事件，保持兼容性
  }
};

const mainColor = computed(() => {
  if (!setting.immersivePlayer) return "rgb(239, 239, 239)";
  return `rgb(${site.songPicColor})`;
});

// 获取当前歌词
const currentLyrics = computed<LyricLine[]>(() => {
  const songLyric = music.songLyric || { lrcAMData: [], yrcAMData: [] };
  const lyricData = createLyricsProcessor(songLyric, { 
    showYrc: setting.showYrc,
    showRoma: setting.showRoma,
    showTransl: setting.showTransl
  });
  return lyricData;
});

watch(() => music.playState, (newState) => {
  if (newState) {
    lyricPlayerRef.value?.lyricPlayer?.value?.setCurrentTime(currentTime.value);
  }
});
</script>



