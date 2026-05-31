<template>
  <div
    class="lyric-player-wrapper"
    @wheel.self="passToPlayer"
    @touchstart.self="passToPlayer"
    @touchmove.self="passToPlayer"
    @touchend.self="passToPlayer"
  >
    <LyricPlayer
      class="amll-lyric-player"
      :lyric-lines="amllLyricLines"
      :current-time="currentTime"
      :playing="playState"
      :enable-blur="setting.lyricsBlur"
      :enable-spring="setting.showYrcAnimation"
      :enable-scale="setting.showYrcAnimation"
      :word-fade-width="0.5"
      :align-anchor="alignAnchor"
      :align-position="alignPosition"
      :line-pos-x-spring-params="setting.springParams.posX"
      :line-pos-y-spring-params="setting.springParams.posY"
      :line-scale-spring-params="setting.springParams.scale"
      :style="lyricStyles"
      @line-click="jumpSeek"
      :key="playerKey"
      ref="playerRef"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, watch, toRaw, shallowRef, useTemplateRef } from "vue";
import { musicStore, settingStore, siteStore } from "../../store";
import { LyricPlayer, type LyricPlayerRef } from "@applemusic-like-lyrics/vue";
import type { DomLyricPlayer, LyricLineMouseEvent } from "@applemusic-like-lyrics/core";
import { preprocessLyrics, getProcessedLyrics, type AMLLLine } from "@/utils/LyricsProcessor";
import "@applemusic-like-lyrics/core/style.css";

const site = siteStore();
const music = musicStore();
const setting = settingStore();

const playerKey = shallowRef(Symbol());
const playerRef = useTemplateRef<LyricPlayerRef>("playerRef");
const amllLyricLines = shallowRef<AMLLLine[]>([]);

const playState = computed(() => music.playState);

const currentTime = computed(
  () => music.persistData.playSongTime.currentTime * 1000 + (setting.lyricTimeOffset ?? 0),
);

const alignAnchor = computed(() => (setting.lyricsBlock === "center" ? "center" : "top"));

const alignPosition = computed(() => (setting.lyricsBlock === "center" ? 0.5 : 0.2));

const mainColor = computed(() => {
  if (!setting.immersivePlayer) return "rgb(239, 239, 239)";
  return `rgb(${site.songPicColor})`;
});

const lyricStyles = computed(() => ({
  "--amll-lp-color": mainColor.value,
  "--amll-lyric-view-color": mainColor.value,
  "font-weight": setting.lyricFontWeight,
  "font-family": setting.lyricFont,
  "letter-spacing": setting.lyricLetterSpacing,
  cursor: "pointer",
  "-webkit-tap-highlight-color": "transparent",
}));

const emit = defineEmits<{
  "line-click": [evt: LyricLineMouseEvent];
  lrcTextClick: [time: number];
}>();

function getDomPlayer(): DomLyricPlayer | undefined {
  const player = playerRef.value?.lyricPlayer;
  if (!player) return undefined;
  return ("value" in player ? player.value : player) as DomLyricPlayer | undefined;
}

function lineClickStartTime(evt: LyricLineMouseEvent): number | undefined {
  const fromGetLine = evt.line?.getLine?.()?.startTime;
  if (typeof fromGetLine === "number") return fromGetLine;
  const fromLyricLine = (evt.line as { lyricLine?: { startTime: number } })?.lyricLine?.startTime;
  if (typeof fromLyricLine === "number") return fromLyricLine;
  return undefined;
}

const jumpSeek = (evt: LyricLineMouseEvent) => {
  const time = lineClickStartTime(evt);
  if (typeof time !== "number") return;
  const player = getDomPlayer();
  player?.setCurrentTime(time, true);
  player?.resetScroll();
  emit("lrcTextClick", time / 1000);
  emit("line-click", evt);
};

function passToPlayer(event: Event) {
  const playerEl = getDomPlayer()?.getElement();
  if (!playerEl) return;
  playerEl.dispatchEvent(new (event.constructor as typeof Event)(event.type, event));
}

watch(
  () => [music.songLyric, setting.showYrc, setting.showRoma, setting.showTransl],
  () => {
    const rawSongLyric = toRaw(music.songLyric);

    if (!rawSongLyric) {
      amllLyricLines.value = [];
      return;
    }

    try {
      preprocessLyrics(rawSongLyric, {
        showYrc: setting.showYrc,
        showRoma: setting.showRoma,
        showTransl: setting.showTransl,
      });
    } catch (error) {
      console.error("[LyricPlayer] 预处理歌词失败", error);
    }

    const processed = getProcessedLyrics(rawSongLyric, {
      showYrc: setting.showYrc,
      showRoma: setting.showRoma,
      showTransl: setting.showTransl,
    });

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
    playerKey.value = Symbol();
  },
  { immediate: true, deep: true },
);
</script>

<style lang="scss" scoped>
.lyric-player-wrapper {
  width: 100%;
  height: 100%;
  touch-action: pan-y;
  container-type: size;
  overflow: hidden;
  display: flex;
  justify-content: center;

  // Inactive bg lines should not expand the measured lyric line height.
  :deep(
    .amll-lyric-player[class*="_playing"] [class*="_bgWrapper"]:not([class*="_bgWrapperActive"])
  ) {
    position: absolute !important;
    top: 100% !important;
    left: 0 !important;
    pointer-events: none;
  }
}
</style>
