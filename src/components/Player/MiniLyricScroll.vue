<!--
  The player bar's lyric line. It reads the play clock itself so that the 30 Hz clock re-renders
  only this line; read in Player/index.vue it re-rendered the whole bar every tick.
-->
<template>
  <LyricScroll :text="text" :progress="progress" />
</template>

<script setup lang="ts">
import { computed } from "vue";
import { musicStore, settingStore } from "@/store";
import LyricScroll from "@/components/Lyric/LyricScroll.vue";
import { getLyricPresentationTimeSeconds } from "@/utils/tauri/audio/uiDelay";

const props = defineProps<{ text: string; start: number; end: number }>();

const music = musicStore();
const setting = settingStore();

// 和桌面歌词同一套：溢出部分随这一行的播放进度滚出来，而不是固定速度循环
const progress = computed(() => {
  const now = getLyricPresentationTimeSeconds(
    music.getPlaySongTime.playbackCurrentTime ?? music.getPlaySongTime.currentTime,
    setting.lyricTimeOffset ?? 0,
  );
  return (now - props.start) / Math.max(0.8, props.end - props.start);
});
</script>
