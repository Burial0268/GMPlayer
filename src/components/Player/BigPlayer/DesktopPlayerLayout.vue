<template>
  <div :class="['all', { noLrc: !showLyrics }]">
    <div class="tip" ref="tipRef" v-show="lrcMouseStatus">
      <n-text>{{ $t("other.lrcClicks") }}</n-text>
    </div>

    <div class="left" ref="leftContentRef">
      <PlayerCover v-if="setting.playerStyle === 'cover'" />
      <PlayerRecord v-else-if="setting.playerStyle === 'record'" />
    </div>

    <div
      ref="rightContentRef"
      :class="['right', { 'lyrics-hidden': !showLyrics }]"
      :aria-hidden="!showLyrics"
    >
      <DesktopLyricsPanel
        :menuShow="menuShow"
        :handleProgressSeek="handleProgressSeek"
        @lrcMouseEnter="$emit('lrcMouseEnter')"
        @lrcAllLeave="$emit('lrcAllLeave')"
        @lrcTextClick="$emit('lrcTextClick', $event)"
      />
    </div>

    <DesktopQueuePanel :show="queueOpen" />
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { musicStore, settingStore } from "@/store";
import PlayerCover from "../PlayerCover.vue";
import PlayerRecord from "../PlayerRecord.vue";
import DesktopLyricsPanel from "./DesktopLyricsPanel.vue";
import DesktopQueuePanel from "./DesktopQueuePanel.vue";

const props = defineProps<{
  lrcMouseStatus: boolean;
  menuShow: boolean;
  hasLyrics: boolean;
  lyricsVisible: boolean;
  queueOpen: boolean;
  handleProgressSeek: (val: number) => void;
}>();

defineEmits<{
  lrcMouseEnter: [];
  lrcAllLeave: [];
  lrcTextClick: [time: number];
}>();

const music = musicStore();
const setting = settingStore();

const tipRef = ref<HTMLElement | null>(null);
const leftContentRef = ref<HTMLElement | null>(null);
const rightContentRef = ref<HTMLElement | null>(null);
const lyricsReady = computed(() => props.hasLyrics && !music.getLoadingState);
const showLyrics = computed(() => lyricsReady.value && props.lyricsVisible);

defineExpose({ tipRef, leftContentRef, rightContentRef });
</script>

<style lang="scss" scoped>
.all {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: row;
  align-items: center;
  position: relative;

  &.noLrc {
    justify-content: flex-start;

    .left {
      flex-basis: 100%;
      width: 100%;
      padding-right: 0;
      padding-left: 0;
      transform: translateX(0) scale(1);
      align-items: center;
    }
  }

  .tip {
    position: absolute;
    top: 24px;
    left: calc(50% - 150px);
    width: 300px;
    height: 40px;
    border-radius: 25px;
    background-color: #ffffff20;
    -webkit-backdrop-filter: blur(20px);
    backdrop-filter: blur(20px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 4;
    will-change: transform, opacity;

    span {
      color: #ffffffc7;
    }
  }

  .left {
    flex: 0 0 40%;
    width: 40%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding-left: 2rem;
    box-sizing: border-box;
    transition:
      flex-basis 0.34s cubic-bezier(0.25, 1, 0.5, 1),
      width 0.34s cubic-bezier(0.25, 1, 0.5, 1),
      padding 0.34s cubic-bezier(0.25, 1, 0.5, 1),
      transform 0.34s cubic-bezier(0.25, 1, 0.5, 1),
      opacity 0.24s ease;
    will-change: width, transform, opacity;
  }

  .right {
    position: absolute;
    inset: 0 0 0 auto;
    width: 60%;
    min-width: 0;
    height: 100%;
    z-index: 1;
    box-sizing: border-box;
    mix-blend-mode: plus-lighter;
    padding-right: 1rem;
    opacity: 1;
    overflow: hidden;
    visibility: visible;
    transform: translate3d(0, 0, 0);
    transition:
      transform 0.34s var(--ease-out),
      opacity 0.18s var(--ease-out) 0.16s,
      visibility 0s linear;
    will-change: transform, opacity;

    &.lyrics-hidden {
      opacity: 0;
      visibility: hidden;
      transform: translate3d(24px, 0, 0);
      pointer-events: none;
      transition:
        opacity 0.22s var(--ease-out),
        transform 0.28s var(--ease-out),
        visibility 0s linear 0.28s;
    }
  }
}
</style>
