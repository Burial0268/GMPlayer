<template>
  <div class="mobile-control-buttons">
    <n-icon
      class="mode-btn"
      size="22"
      :component="
        persistData.playSongMode === 'random'
          ? ShuffleOne
          : persistData.playSongMode === 'single'
            ? PlayOnce
            : PlayCycle
      "
      @click.stop="music.setPlaySongMode()"
    />
    <n-icon
      v-if="!music.getPersonalFmMode"
      class="prev"
      size="36"
      :component="IconRewind"
      @click.stop="music.setPlaySongIndex('prev')"
    />
    <n-icon
      v-else
      class="dislike"
      size="28"
      :component="ThumbDownRound"
      @click="music.setFmDislike(music.getPersonalFmData.id)"
    />
    <div class="play-state">
      <n-button
        :loading="music.getLoadingState"
        secondary
        circle
        :keyboard="false"
        :focusable="false"
        @click.stop="music.setPlayState(!music.getPlayState)"
      >
        <template #icon>
          <Transition name="fade" mode="out-in">
            <n-icon size="64" :component="music.getPlayState ? IconPause : IconPlay" />
          </Transition>
        </template>
      </n-button>
    </div>
    <n-icon
      class="next"
      size="36"
      :component="IconForward"
      @click.stop="music.setPlaySongIndex('next')"
    />
    <n-icon class="mode-btn" size="22" :component="MessageRound" @click.stop="$emit('toComment')" />
  </div>
  <!-- 音量控制 -->
  <div class="mobile-volume">
    <BouncingSlider
      :value="persistData.playVolume"
      :min="0"
      :max="1"
      :change-on-drag="true"
      @update:value="(val) => (persistData.playVolume = val)"
    >
      <template #before-icon>
        <n-icon size="20" :component="VolumeOffRound" />
      </template>
      <template #after-icon>
        <n-icon size="20" :component="VolumeUpRound" />
      </template>
    </BouncingSlider>
  </div>
</template>

<script setup lang="ts">
import { ThumbDownRound, MessageRound, VolumeUpRound, VolumeOffRound } from "@vicons/material";
import { ShuffleOne, PlayOnce, PlayCycle } from "@icon-park/vue-next";
import { musicStore } from "@/store";
import { storeToRefs } from "pinia";
import BouncingSlider from "../BouncingSlider.vue";
import IconPlay from "../icons/IconPlay.vue";
import IconPause from "../icons/IconPause.vue";
import IconForward from "../icons/IconForward.vue";
import IconRewind from "../icons/IconRewind.vue";

defineEmits<{
  toComment: [];
}>();

const music = musicStore();
const { persistData } = storeToRefs(music);
</script>

<style lang="scss" scoped>
.mobile-control-buttons {
  display: flex;
  align-items: center;
  justify-content: space-evenly;
  width: 100%;

  .n-icon {
    color: var(--main-cover-color);
    cursor: pointer;
    transition:
      transform 0.15s ease,
      opacity 0.15s ease;

    &:active {
      transform: scale(0.85);
    }
  }

  .mode-btn {
    opacity: 0.6;

    &:active {
      transform: scale(0.85);
    }
  }

  .prev,
  .next {
    opacity: 0.9;
  }

  .dislike {
    opacity: 0.9;
  }

  .play-state {
    display: flex;
    align-items: center;
    justify-content: center;

    .n-button {
      --n-width: min(16vw, 64px);
      --n-height: min(16vw, 64px);
      --n-color: transparent;
      --n-color-hover: rgba(255, 255, 255, 0.1);
      --n-color-pressed: rgba(255, 255, 255, 0.15);
      --n-text-color: var(--main-cover-color);
      --n-text-color-hover: var(--main-cover-color);
      --n-text-color-pressed: var(--main-cover-color);
      --n-border: none;
      border: none;
    }

    .n-icon {
      opacity: 1;
    }
  }
}

.mobile-volume {
  display: flex;
  align-items: center;
  width: 100%;
  margin-top: 16px;

  :deep(.n-icon) {
    color: var(--main-cover-color);
    opacity: 0.4;
    flex-shrink: 0;
  }
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
