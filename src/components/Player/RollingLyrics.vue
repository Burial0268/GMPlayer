<template>
  <div :class="lyricClasses">
    <LyricPlayer
      class="am-lyric"
      @lrcTextClick="handleLrcTextClick"
      @lineShare="(lines, lineIndex, origin) => lyricShare.open({ lines, lineIndex, origin })"
    >
      <template v-if="credit" #bottomLine>
        <strong>{{ t(`player.credits.${credit.kind}`) }}</strong>
        <template v-for="(name, index) in credit.names" :key="name">
          <a
            v-if="credit.kind === 'authors'"
            class="credit-author"
            :href="`https://github.com/${name}`"
            target="_blank"
            rel="noreferrer noopener"
            >{{ name }}</a
          >
          <span v-else>{{ name }}</span>
          <span v-if="index < credit.names.length - 1">, </span>
        </template>
      </template>
    </LyricPlayer>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { musicStore, settingStore } from "../../store";
import LyricPlayer from "../../libs/apple-music-like/LyricPlayer.vue";
import { useLyricShare } from "@/composables/useLyricShare";

const emit = defineEmits<{
  lrcTextClick: [time: number];
}>();

const music = musicStore();
const lyricShare = useLyricShare();
const setting = settingStore();
const { t } = useI18n();

// 直接处理从 LyricPlayer 传递的 lrcTextClick 事件
const handleLrcTextClick = (time: number) => {
  emit("lrcTextClick", time);
};

type CreditKind = "authors" | "songwriters";

// 尾栏署名，取舍规则同 AMLL Player 的 BottomLyricInfo
const credit = computed(() => {
  const credits = music.songLyric?.credits;
  const mode = setting.lyricCreditsMode;
  if (!credits || mode === "none") return null;
  const order: CreditKind[] =
    mode === "preferAuthors"
      ? ["authors", "songwriters"]
      : mode === "preferSongwriters"
        ? ["songwriters", "authors"]
        : [mode];
  const kind = order.find((k) => credits[k].length > 0);
  return kind ? { kind, names: credits[kind] } : null;
});

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
    transform var(--duration-500) cubic-bezier(0.34, 1.56, 0.64, 1),
    opacity var(--duration-500) cubic-bezier(0.34, 1.56, 0.64, 1);

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

  .credit-author {
    padding: 0.1em 0.3em;
    border-radius: 0.25em;
    color: inherit;
    text-decoration: none;
    transition: background-color 0.25s;

    @media (hover: hover) {
      &:hover {
        background-color: var(--amll-lp-hover-bg-color, #fff1);
      }
    }

    &:active {
      background-color: var(--amll-lp-hover-bg-color, #ffffff05);
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
