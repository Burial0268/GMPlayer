<template>
  <div class="lyric-player-wrapper">
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
      ref="amllPlayerRef"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, watchEffect, toRaw, shallowRef, onMounted, nextTick } from "vue";
import { musicStore, settingStore, siteStore } from "../../store";
import { LyricPlayer, LyricPlayerRef } from "@applemusic-like-lyrics/vue";
import { preprocessLyrics, getProcessedLyrics, type AMLLLine } from "@/utils/LyricsProcessor";
import "@applemusic-like-lyrics/core/style.css";

const site = siteStore();
const music = musicStore();
const setting = settingStore();

// 直接复制 AMLL-Editor 的实现模式
const playerKey = ref(Symbol());
const amllLyricLines = shallowRef<AMLLLine[]>([]);
const amllPlayerRef = ref<LyricPlayerRef>();

const playState = shallowRef(false);
const currentTime = shallowRef(0);

onMounted(() => {
  nextTick(() => {
    // 强制触发 playState 更新
    playState.value = music.playState;
  });
});

watchEffect(() => {
  playState.value = music.playState;
});

const copyValue = (value: any) => {
  return setting[value];
};

const emit = defineEmits<{
  "line-click": [e: { line: { getLine: () => { startTime: number } } }];
  lrcTextClick: [time: number];
}>();

// 计算当前播放时间
watchEffect(() => {
  currentTime.value =
    music.persistData.playSongTime.currentTime * 1000 + (setting.lyricTimeOffset ?? 0);
});

// 计算对齐方式
const alignAnchor = computed(() => (setting.lyricsBlock === "center" ? "center" : "top"));

const alignPosition = computed(() => (setting.lyricsBlock === "center" ? 0.5 : 0.2));

const mainColor = computed(() => {
  if (!setting.immersivePlayer) return "rgb(239, 239, 239)";
  return `rgb(${site.songPicColor})`;
});

// 设置样式（直接设置，不使用 v-bind 转换）
const lyricStyles = computed(() => ({
  "--amll-lp-color": mainColor.value,
  "--amll-lyric-view-color": mainColor.value,
  "font-weight": setting.lyricFontWeight,
  "font-family": setting.lyricFont,
  "letter-spacing": setting.lyricLetterSpacing,
  "font-size": `${setting.lyricsFontSize * 3}px`,
  cursor: "pointer",
  "-webkit-tap-highlight-color": "transparent",
}));

// 处理歌词点击（参考 AMLL-Editor 的 jumpSeek）- 用于桌面端
const handleLineClick = (evt: any) => {
  console.log("[LyricPlayer] line-click event", evt);
  const targetTime = evt.line.getLine().startTime;
  amllPlayerRef.value?.lyricPlayer.value?.setCurrentTime(targetTime, true);
  amllPlayerRef.value?.lyricPlayer.value?.update();
  emit("lrcTextClick", targetTime / 1000);
  emit("line-click", evt);
};

// 播放/暂停时更新歌词状态
watch(
  () => music.playState,
  (newVal) => {
    newVal
      ? amllPlayerRef.value?.lyricPlayer.value?.resume()
      : amllPlayerRef.value?.lyricPlayer.value?.pause();
    amllPlayerRef.value?.lyricPlayer.value?.update();
  },
  { immediate: true },
);

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
        showTransl: setting.showTransl,
      });
    } catch (error) {
      console.error("[LyricPlayer] 预处理歌词失败", error);
    }

    // 使用优化后的函数获取歌词，优先使用缓存数据
    const processed = getProcessedLyrics(rawSongLyric, {
      showYrc: setting.showYrc,
      showRoma: setting.showRoma,
      showTransl: setting.showTransl,
    });

    // Ensure translation/roma are stripped when settings disable them.
    // processLyrics should already handle this, but we sanitize here as a
    // final guarantee so the AMLL renderer never receives stale sub-line data.
    if (!setting.showTransl || !setting.showRoma) {
      for (let i = 0; i < processed.length; i++) {
        const line = processed[i];
        if (!setting.showTransl) line.translatedLyric = "";
        if (!setting.showRoma) {
          line.romanLyric = "";
          const words = line.words;
          for (let j = 0; j < words.length; j++) {
            words[j].romanWord = "";
          }
        }
      }
    }

    amllLyricLines.value = processed;
    playerKey.value = Symbol(); // 强制重新渲染组件
  },
  { immediate: true, deep: true },
);
</script>

<style lang="scss" scoped>
.lyric-player-wrapper {
  width: 100%;
  height: 100%;
  touch-action: pan-y;
}

.amll-lyric-player {
  // Hover feedback color for lyric lines (core module CSS reads this variable)
  --amll-lp-hover-bg-color: rgba(255, 255, 255, 0.067);

  // Fix: emphasize span padding — letters like 'j' clipped
  // Must use :deep() to penetrate scoped boundary into child component DOM
  &.dom:deep(span[class^="_emphasizeWrapper"] span) {
    padding: 1em;
    margin: -1em;
  }
}
</style>
