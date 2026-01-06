<template>
  <LyricPlayer
    class="amll-lyric-player"
    :lyric-lines="amllLyricLines"
    :current-time="currentTime"
    :playing="playState"
    :enable-blur="copyValue('lyricsBlur')"
    :enable-spring="copyValue('showYrcAnimation')"
    :enable-scale="copyValue('showYrcAnimation')"
    :word-fade-width="0.5"
    :align-anchor="alignAnchor"
    :align-position="alignPosition"
    :line-pos-x-spring-params="copyValue('springParams.posX')"
    :line-pos-y-spring-params="copyValue('springParams.posY')"
    :line-scale-spring-params="copyValue('springParams.scale')"
    :enable-interlude-dots="true"
    :style="lyricStyles"
    @line-click="handleLineClick"
    :key="playerKey"
  />
</template>

<script setup lang="ts">
import { ref, computed, watch, watchEffect, toRaw, shallowRef } from 'vue';
import { musicStore, settingStore, siteStore } from "../../store";
import { LyricPlayer } from "@applemusic-like-lyrics/vue";
import { preprocessLyrics, getProcessedLyrics, type LyricLine } from "./processLyrics";
import '@applemusic-like-lyrics/core/style.css';

const site = siteStore();
const music = musicStore();
const setting = settingStore();
const fontSize = ref(setting.lyricsFontSize * 3);

// 直接复制 AMLL-Editor 的实现模式
const playerKey = ref(Symbol());
const amllLyricLines = shallowRef<LyricLine[]>([]);

const playState = shallowRef(false);
const currentTime = shallowRef(0);

watchEffect(() => {
  playState.value = music.playState;
});

const copyValue = (value: any) => {
  return setting[value];
};

const emit = defineEmits<{
  'line-click': [e: { line: { getLine: () => { startTime: number } } }],
  lrcTextClick: [time: number]
}>();

// 计算当前播放时间
watchEffect(() => {
  // 提前 150ms 来解决异步更新延迟问题
  currentTime.value = (music.persistData.playSongTime.currentTime * 1000) + 150;
});

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
  '--amll-lp-font-size': `${fontSize.value}px`,
  '--amll-lp-height': setting.lyricLineHeight,
  '--amll-lp-word-spacing': '0em',
  'font-weight': setting.lyricFontWeight,
  'font-family': setting.lyricFont,
  'letter-spacing': setting.lyricLetterSpacing,
  'cursor': 'pointer',
  '--amll-lyric-view-color': mainColor.value,
  'user-select': 'none',
  '-webkit-tap-highlight-color': 'transparent'
}));

// 处理歌词点击（参考 AMLL-Editor 的 jumpSeek）
const handleLineClick = (line: any) => {
  if (!line?.line?.lyricLine?.startTime) return;
  const time = line.line.lyricLine.startTime;
  emit("lrcTextClick", time / 1000);
  emit("line-click", line);
};

const mainColor = computed(() => {
  if (!setting.immersivePlayer) return "rgb(239, 239, 239)";
  return `rgb(${site.songPicColor})`;
});

// 更新歌词数据（直接复制 AMLL-Editor 的 watch 模式）
watch(
  () => [music.songLyric, setting.showYrc, setting.showRoma, setting.showTransl],
  () => {
    const rawSongLyric = toRaw(music.songLyric);
    
    if (!rawSongLyric) {
      amllLyricLines.value = [];
      return;
    }
    
    // 预处理歌词（如果尚未处理）
    try {
      preprocessLyrics(rawSongLyric, { 
        showYrc: setting.showYrc,
        showRoma: setting.showRoma,
        showTransl: setting.showTransl
      });
    } catch (error) {
      console.error("[LyricPlayer] 预处理歌词失败", error);
    }
    
    // 使用优化后的函数获取歌词，优先使用缓存数据
    const processed = getProcessedLyrics(rawSongLyric, { 
      showYrc: setting.showYrc,
      showRoma: setting.showRoma,
      showTransl: setting.showTransl
    });
    
    amllLyricLines.value = processed;
    playerKey.value = Symbol(); // 强制重新渲染组件
  },
  { immediate: true, deep: true },
);
</script>

<style lang="scss" scoped>
.amll-lyric-player.dom {
  line-height: 1.5;
  --bright-mask-alpha: 1;
  --dark-mask-alpha: 0.4;

  // Fix padding issue: letters like 'j' get cut off
  span[class^='_emphasizeWrapper'] span {
    padding: 1em;
    margin: -1em;
  }
}
</style>
