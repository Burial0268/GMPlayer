<template>
  <div class="lrcShow">
    <RollingLyrics @mouseenter="onMouseEnter" @mouseleave="$emit('lrcAllLeave')"
      @lrcTextClick="$emit('lrcTextClick', $event)" />

    <DesktopRecordMenu
      :menuShow="menuShow"
      :handleProgressSeek="handleProgressSeek"
    />
  </div>
</template>

<script setup lang="ts">
import { settingStore } from "@/store";
import RollingLyrics from "../RollingLyrics.vue";
import DesktopRecordMenu from "./DesktopRecordMenu.vue";

const setting = settingStore();

defineProps<{
  menuShow: boolean;
  handleProgressSeek: (val: number) => void;
}>();

const emit = defineEmits<{
  lrcMouseEnter: [];
  lrcAllLeave: [];
  lrcTextClick: [time: number];
}>();

const onMouseEnter = () => {
  emit('lrcMouseEnter');
};
</script>

<style lang="scss" scoped>
.lrcShow {
  height: 100%;
  display: flex;
  justify-content: center;
  flex-direction: column;

  .data {
    padding: 0 3vh;
    margin-bottom: 8px;
    text-shadow: 0 0 0.3em color-mix(in srgb, currentColor 15%, transparent);

    .name {
      font-size: 3vh;
      -webkit-line-clamp: 2;
      line-clamp: 2;
      padding-right: 26px;
      will-change: transform, opacity;

      span {
        &:nth-of-type(2) {
          margin-left: 12px;
          font-size: 2.3vh;
          opacity: 0.6;
        }
      }
    }

    .artists {
      margin-top: 4px;
      opacity: 0.6;
      font-size: 1.8vh;
      will-change: transform, opacity;

      .artist {
        span {
          &:nth-of-type(2) {
            margin: 0 2px;
          }
        }
      }
    }
  }
}
</style>
