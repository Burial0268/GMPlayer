<template>
  <div :class="[
    hasLyrics && !music.getLoadingState ? 'all' : 'all noLrc'
  ]">
    <div class="tip" ref="tipRef" v-show="lrcMouseStatus">
      <n-text>{{ $t("other.lrcClicks") }}</n-text>
    </div>

    <div class="left" ref="leftContentRef">
      <PlayerCover v-if="setting.playerStyle === 'cover'" />
      <PlayerRecord v-else-if="setting.playerStyle === 'record'" />
    </div>

    <div class="right" ref="rightContentRef" v-if="hasLyrics && !music.getLoadingState">
      <DesktopLyricsPanel
        :menuShow="menuShow"
        :handleProgressSeek="handleProgressSeek"
        @lrcMouseEnter="$emit('lrcMouseEnter')"
        @lrcAllLeave="$emit('lrcAllLeave')"
        @lrcTextClick="$emit('lrcTextClick', $event)"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { musicStore, settingStore } from "@/store";
import PlayerCover from "../PlayerCover.vue";
import PlayerRecord from "../PlayerRecord.vue";
import DesktopLyricsPanel from "./DesktopLyricsPanel.vue";

defineProps<{
  lrcMouseStatus: boolean;
  menuShow: boolean;
  hasLyrics: boolean;
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
    justify-content: center;

    .left {
      padding-right: 0;
      width: auto;
      transform: none;
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
    width: 40%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding-left: 2rem;
    box-sizing: border-box;
  }

  .right {
    flex: 1;
    height: 100%;
    mix-blend-mode: plus-lighter;
    padding-right: 1rem;
  }
}

/* 桌面端左侧控制区 plus-lighter — :global 绕过 scoped 组件边界 */
:global(.bplayer .left .controls),
:global(.bplayer .left .controls .bouncing-slider),
:global(.bplayer .left .controls .n-icon) {
  mix-blend-mode: plus-lighter;
}
</style>
