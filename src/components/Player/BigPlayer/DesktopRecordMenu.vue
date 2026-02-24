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
      <n-icon
        v-if="!music.getPersonalFmMode"
        class="prev"
        size="30"
        :component="IconRewind"
        @click.stop="music.setPlaySongIndex('prev')"
      />
      <n-icon
        v-else
        class="dislike"
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
      <n-icon
        class="next"
        size="30"
        :component="IconForward"
        @click.stop="music.setPlaySongIndex('next')"
      />
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

    .next,
    .prev,
    .dislike {
      cursor: pointer;
      padding: 4px;
      border-radius: 50%;
      transition: all 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
      will-change: transform, background-color;

      &:hover {
        background-color: var(--main-color);
        transform: scale(1.1);
      }

      &:active {
        transform: scale(0.9);
      }
    }

    .dislike {
      padding: 9px;
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
  .prev,
  .next {
    width: 30px;
    height: 30px;
  }

  .control-icon {
    width: 42px;
    height: 42px;
  }
}
</style>
