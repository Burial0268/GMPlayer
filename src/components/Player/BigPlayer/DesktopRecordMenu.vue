<template>
  <div :class="menuShow ? 'menu show' : 'menu'" v-show="setting.playerStyle === 'record'">
    <div class="time">
      <span>{{ music.getPlaySongTime.songTimePlayed }}</span>
      <BouncingSlider
        :value="music.getPlaySongTime.currentTime || 0"
        :min="0"
        :max="music.getPlaySongTime.duration || 1"
        :is-playing="music.getPlayState"
        @update:value="handleProgressSeek"
        @seek-start="music.setPlayState(false)"
        @seek-end="music.setPlayState(true)"
      />
      <span>{{ music.getPlaySongTime.songTimeDuration }}</span>
    </div>
    <div class="control">
      <div class="skip-state" v-if="!music.getPersonalFmMode">
        <n-button
          circle
          :keyboard="false"
          :focusable="false"
          @click.stop="music.setPlaySongIndex('prev')"
        >
          <template #icon>
            <n-icon size="26" :component="IconRewind" />
          </template>
        </n-button>
      </div>
      <div class="skip-state" v-else>
        <n-button
          circle
          :keyboard="false"
          :focusable="false"
          @click.stop="music.setFmDislike(music.getPersonalFmData.id)"
        >
          <template #icon>
            <n-icon size="22" :component="ThumbDownRound" />
          </template>
        </n-button>
      </div>
      <div class="play-state">
        <n-button
          :loading="music.getLoadingState"
          secondary
          circle
          :keyboard="false"
          :focusable="false"
        >
          <template #icon>
            <Transition name="fade" mode="out-in">
              <n-icon
                size="42"
                :component="music.getPlayState ? IconPause : IconPlay"
                @click.stop="music.setPlayState(!music.getPlayState)"
              />
            </Transition>
          </template>
        </n-button>
      </div>
      <div class="skip-state">
        <n-button
          circle
          :keyboard="false"
          :focusable="false"
          @click.stop="music.setPlaySongIndex('next')"
        >
          <template #icon>
            <n-icon size="26" :component="IconForward" />
          </template>
        </n-button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ThumbDownRound } from "@vicons/material";
import { musicStore, settingStore } from "@/store";
import BouncingSlider from "../BouncingSlider.vue";
import IconPlay from "../icons/IconPlay.vue";
import IconPause from "../icons/IconPause.vue";
import IconForward from "../icons/IconForward.vue";
import IconRewind from "../icons/IconRewind.vue";

defineProps<{
  menuShow: boolean;
  handleProgressSeek: (val: number) => void;
}>();

const music = musicStore();
const setting = settingStore();
</script>

<style lang="scss" scoped>
.menu {
  opacity: 0;
  padding: 1vh 2vh;
  display: flex !important;
  justify-content: center;
  align-items: center;
  transition: all 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
  flex-direction: column;
  will-change: opacity;

  .time {
    display: flex;
    flex-direction: row;
    align-items: center;
    width: 100%;
    margin-right: 3em;
    margin-left: 3em;

    span {
      opacity: 0.8;
    }

    .bouncing-slider {
      margin: 0 10px;
      flex: 1;
    }
  }

  .control {
    margin-top: 0.8em;
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: center;
    transform: scale(1.4);

    .skip-state {
      display: flex;
      align-items: center;
      justify-content: center;

      .n-button {
        --n-width: 36px;
        --n-height: 36px;
        --n-color: transparent;
        --n-color-hover: var(--main-color);
        --n-color-pressed: var(--main-color);
        --n-text-color: var(--main-cover-color);
        --n-text-color-hover: var(--main-cover-color);
        --n-text-color-pressed: var(--main-cover-color);
        --n-border: none;
        border: none;
        transition: all 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
        will-change: transform, background-color;

        &:hover {
          transform: scale(1.1);
        }

        &:active {
          transform: scale(0.9);
        }
      }

      .n-icon {
        color: var(--main-cover-color);
      }
    }

    .play-state {
      --n-width: 42px;
      --n-height: 42px;
      color: var(--main-cover-color);
      margin: 0 12px;
      transition: all 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
      will-change: transform, background-color;

      .n-icon {
        transition: all 0.2s cubic-bezier(0.34, 1.56, 0.64, 1);
        color: var(--main-cover-color);
        will-change: transform, opacity;
      }

      &:active {
        transform: scale(1);
      }

      &:hover .n-icon {
        transform: scale(1.1);
      }
    }
  }

  &.show {
    opacity: 1;
  }

  .n-icon {
    font-size: 24px;
    cursor: pointer;
    padding: 8px;
    border-radius: 8px;
    opacity: 0.4;
    transition: all 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
    will-change: transform, opacity, background-color;

    &:hover {
      background-color: #ffffff30;
      transform: scale(1.05);
    }

    &:active {
      transform: scale(0.95);
    }

    &.open {
      opacity: 1;
    }
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

.control {
  .skip-state .n-button {
    width: 36px;
    height: 36px;
  }

  .control-icon {
    width: 42px;
    height: 42px;
  }
}
</style>
