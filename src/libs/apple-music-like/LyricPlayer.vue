<template>
  <div
    class="lyric-player-wrapper"
    @touchstart.passive="handleTouchStart"
    @touchend.passive="handleTouchEnd"
  >
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
import { ref, computed, watch, watchEffect, toRaw, shallowRef, onMounted, nextTick } from 'vue';
import { musicStore, settingStore, siteStore } from "../../store";
import { LyricPlayer, LyricPlayerRef } from "@applemusic-like-lyrics/vue";
import { preprocessLyrics, getProcessedLyrics, type LyricLine } from "./processLyrics";
import '@applemusic-like-lyrics/core/style.css';

const site = siteStore();
const music = musicStore();
const setting = settingStore();

// 直接复制 AMLL-Editor 的实现模式
const playerKey = ref(Symbol());
const amllLyricLines = shallowRef<LyricLine[]>([]);
const amllPlayerRef = ref<LyricPlayerRef>();

const playState = shallowRef(false);
const currentTime = shallowRef(0);

// 移动端触摸处理 - 用于检测点击歌词行
let touchStartTime = 0;
let touchStartX = 0;
let touchStartY = 0;
const TAP_THRESHOLD_TIME = 300; // 点击最大时长 (ms)
const TAP_THRESHOLD_DISTANCE = 15; // 点击最大移动距离 (px)

const handleTouchStart = (e: TouchEvent) => {
  touchStartTime = Date.now();
  touchStartX = e.touches[0].clientX;
  touchStartY = e.touches[0].clientY;
  console.log('[LyricPlayer] touchstart', touchStartX, touchStartY);
};

const handleTouchEnd = (e: TouchEvent) => {
  const touchEndTime = Date.now();
  const touchEndX = e.changedTouches[0].clientX;
  const touchEndY = e.changedTouches[0].clientY;

  const timeDiff = touchEndTime - touchStartTime;
  const distanceX = Math.abs(touchEndX - touchStartX);
  const distanceY = Math.abs(touchEndY - touchStartY);

  console.log('[LyricPlayer] touchend', { timeDiff, distanceX, distanceY });

  // 判断是否为点击（短时间、小位移）
  if (timeDiff < TAP_THRESHOLD_TIME && distanceX < TAP_THRESHOLD_DISTANCE && distanceY < TAP_THRESHOLD_DISTANCE) {
    console.log('[LyricPlayer] detected tap');

    // 查找点击的歌词行元素
    const target = document.elementFromPoint(touchEndX, touchEndY) as HTMLElement;
    console.log('[LyricPlayer] target element', target);

    if (target) {
      // 向上查找歌词行容器 - AMLL 使用 _lyricMainLine 类名 (CSS modules)
      const lineEl = target.closest('[class*="lyricMainLine"]') ||
                     target.closest('[class*="LyricMainLine"]') ||
                     target.closest('[class*="lyric-main-line"]');

      console.log('[LyricPlayer] found line element', lineEl);

      if (lineEl) {
        const lyricPlayer = amllPlayerRef.value?.lyricPlayer.value;
        if (lyricPlayer) {
          // 获取所有歌词行元素
          const container = lyricPlayer.getElement();
          const lineElements = container.querySelectorAll('[class*="lyricMainLine"], [class*="LyricMainLine"]');
          const lineIndex = Array.from(lineElements).indexOf(lineEl as Element);

          console.log('[LyricPlayer] line index', lineIndex, 'total lines', amllLyricLines.value.length);

          if (lineIndex !== -1 && amllLyricLines.value[lineIndex]) {
            const targetTime = amllLyricLines.value[lineIndex].startTime;
            console.log('[LyricPlayer] seeking to', targetTime);
            lyricPlayer.setCurrentTime(targetTime, true);
            emit("lrcTextClick", targetTime / 1000);
          }
        }
      }
    }
  }
};

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

const mainColor = computed(() => {
  if (!setting.immersivePlayer) return "rgb(239, 239, 239)";
  return `rgb(${site.songPicColor})`;
});

// 设置样式（直接设置，不使用 v-bind 转换）
const lyricStyles = computed(() => ({
  '--amll-lp-color': mainColor.value,
  '--amll-lyric-view-color': mainColor.value,
  'font-weight': setting.lyricFontWeight,
  'font-family': setting.lyricFont,
  'letter-spacing': setting.lyricLetterSpacing,
  'font-size': `${setting.lyricsFontSize * 3}px`,
  'cursor': 'pointer',
  'user-select': 'none',
  '-webkit-tap-highlight-color': 'transparent',
}));

// 处理歌词点击（参考 AMLL-Editor 的 jumpSeek）- 用于桌面端
const handleLineClick = (evt: any) => {
  console.log('[LyricPlayer] line-click event', evt);
  const targetTime = evt.line.getLine().startTime;
  amllPlayerRef.value?.lyricPlayer.value?.setCurrentTime(targetTime, true);
  emit("lrcTextClick", targetTime / 1000);
  emit("line-click", evt);
};

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
.lyric-player-wrapper {
  width: 100%;
  height: 100%;
  touch-action: pan-y;
}

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
