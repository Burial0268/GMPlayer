<template>
  <div class="record">
    <img
      :class="music.getPlayState ? 'pointer play' : 'pointer'"
      src="/images/ico/pointer.png"
      alt="pointer"
    />
    <div
      class="pic"
      :style="{
        animationPlayState: music.getPlayState ? 'running' : 'paused',
      }"
    >
      <img
        class="album"
        :src="
          music.getPlaySongData
            ? music.getPlaySongData.album.picUrl.replace(/^http:/, 'https:') + '?param=500y500'
            : '/images/pic/default.png'
        "
        alt="cover"
      />
    </div>
    <div class="controls">
      <div class="song-info">
        <div class="text">
          <span class="name text-hidden">
            {{ music.getPlaySongData ? music.getPlaySongData.name : $t("other.noSong") }}
          </span>
          <span v-if="music.getPlaySongData" class="artists text-hidden">
            <span v-for="(ar, index) in music.getPlaySongData.artist" :key="ar.id">
              <span class="artist-name" @click="routerJump('/artist', { id: ar.id })">{{
                ar.name
              }}</span>
              <span v-if="index < music.getPlaySongData.artist.length - 1"> / </span>
            </span>
          </span>
        </div>
        <div class="action-row">
          <n-icon
            class="like-button"
            size="24"
            :component="
              music.getPlaySongData && music.getSongIsLike(music.getPlaySongData.id)
                ? StarRound
                : StarBorderRound
            "
            @click.stop="
              music.getPlaySongData &&
              (music.getSongIsLike(music.getPlaySongData.id)
                ? music.changeLikeList(music.getPlaySongData.id, false)
                : music.changeLikeList(music.getPlaySongData.id, true))
            "
          />
        </div>
      </div>
      <div class="progress-bar">
        <div class="slider-wrapper">
          <BouncingSlider
            :value="music.getPlaySongTime.currentTime || 0"
            :min="0"
            :max="music.getPlaySongTime.duration || 1"
            :is-playing="music.getPlayState"
            @update:value="handleProgressSeek"
            @seek-start="music.setPlayState(false)"
            @seek-end="music.setPlayState(true)"
          />
        </div>
        <div class="time-info">
          <span class="time-text">{{ music.getPlaySongTime.songTimePlayed }}</span>
          <span class="time-text">{{ remainingTime }}</span>
        </div>
      </div>
      <div class="buttons">
        <n-icon
          :style="music.getPersonalFmMode ? 'opacity: 0.2;pointer-events: none;' : null"
          class="button-icon"
          :class="{ active: music.getPlaySongMode !== 'normal' }"
          :component="playModeIcon"
          @click="cyclePlayMode"
        />
        <n-icon
          v-if="!music.getPersonalFmMode"
          class="button-icon"
          :component="IconRewind"
          @click.stop="music.setPlaySongIndex('prev')"
        />
        <n-icon
          v-else
          class="button-icon dislike"
          :style="!user.userLogin ? 'opacity: 0.2;pointer-events: none;' : null"
          :component="ThumbDownRound"
          @click="music.setFmDislike(music.getPersonalFmData.id)"
        />
        <div class="play-state">
          <n-button text :focusable="false" :loading="music.getLoadingState">
            <template #icon>
              <n-icon
                :component="music.getPlayState ? IconPause : IconPlay"
                @click.stop="music.setPlayState(!music.getPlayState)"
              />
            </template>
          </n-button>
        </div>
        <n-icon
          class="button-icon"
          :component="IconForward"
          @click.stop="music.setPlaySongIndex('next')"
        />
        <n-icon class="button-icon" :component="MessageRound" @click="goToComment" />
      </div>
      <div class="volume-control">
        <BouncingSlider
          :value="persistData.playVolume"
          :min="0"
          :max="1"
          :change-on-drag="true"
          @update:value="(val) => (persistData.playVolume = val)"
        >
          <template #before-icon>
            <n-icon size="18" :component="VolumeOffRound" />
          </template>
          <template #after-icon>
            <n-icon size="18" :component="VolumeUpRound" />
          </template>
        </BouncingSlider>
      </div>
    </div>
  </div>
</template>

<script setup>
import {
  SkipPreviousRound,
  SkipNextRound,
  ThumbDownRound,
  StarBorderRound,
  StarRound,
  VolumeOffRound,
  VolumeUpRound,
  MessageRound,
} from "@vicons/material";
import { ShuffleOne, PlayOnce, PlayCycle } from "@icon-park/vue-next";
import { computed } from "vue";
import IconForward from "./icons/IconForward.vue";
import IconRewind from "./icons/IconRewind.vue";
import IconPlay from "./icons/IconPlay.vue";
import IconPause from "./icons/IconPause.vue";
import { musicStore, userStore } from "@/store";
import { storeToRefs } from "pinia";
import { useRouter } from "vue-router";
import { setSeek } from "@/utils/AudioContext";
import BouncingSlider from "./BouncingSlider.vue";

const router = useRouter();
const music = musicStore();
const user = userStore();
const { persistData } = storeToRefs(music);

// 剩余时间（负数格式）
const remainingTime = computed(() => {
  const songTime = music.getPlaySongTime;
  if (!songTime?.duration) return "-0:00";
  const remainingSeconds = Math.max(0, songTime.duration - (songTime.currentTime || 0));
  const minutes = Math.floor(remainingSeconds / 60);
  const seconds = Math.floor(remainingSeconds % 60);
  return `-${minutes}:${seconds.toString().padStart(2, "0")}`;
});

// 播放模式图标
const playModeIcon = computed(() => {
  const mode = music.getPlaySongMode;
  if (mode === "random") return ShuffleOne;
  if (mode === "single") return PlayOnce;
  return PlayCycle;
});

// 循环切换播放模式
const cyclePlayMode = () => {
  const mode = music.getPlaySongMode;
  if (mode === "normal") {
    music.setPlaySongMode("random");
  } else if (mode === "random") {
    music.setPlaySongMode("single");
  } else {
    music.setPlaySongMode("normal");
  }
};

// 歌曲进度条更新
const handleProgressSeek = (val) => {
  if (typeof $player !== "undefined" && music.getPlaySongTime?.duration) {
    music.persistData.playSongTime.currentTime = val;
    setSeek($player, val);
  }
};

// 页面跳转
const routerJump = (url, query) => {
  music.setBigPlayerState(false);
  router.push({ path: url, query });
};

// 跳转到评论
const goToComment = () => {
  if (music.getPlaySongData?.id) {
    music.setBigPlayerState(false);
    router.push({
      path: "/comment",
      query: { id: music.getPlaySongData.id },
    });
  }
};
</script>

<style lang="scss" scoped>
.record {
  --cover-size: min(50vh, 38vw);
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2rem;

  @media screen and (max-height: 768px) {
    --cover-size: min(45vh, 38vw);
    gap: 1.5rem;
  }
  &:hover {
    .control {
      opacity: 1;
    }
  }
  .pointer {
    position: absolute;
    width: calc(var(--cover-size) * 0.35);
    left: calc(50% - var(--cover-size) * 0.045);
    top: calc(var(--cover-size) * -0.35);
    transform: rotate(-20deg);
    transform-origin: calc(var(--cover-size) * 0.045) calc(var(--cover-size) * 0.045);
    z-index: 1;
    transition: all 0.3s;
    &.play {
      transform: rotate(0);
    }
  }
  .pic {
    animation: rotate 18s linear infinite;
    border-radius: 50%;
    border: calc(var(--cover-size) * 0.025) solid #ffffff30;
    background:
      linear-gradient(black 0%, transparent, black 98%),
      radial-gradient(
        #000 52%,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555,
        #000,
        #555
      );
    background-clip: content-box;
    width: var(--cover-size);
    height: var(--cover-size);
    display: flex;
    justify-content: center;
    align-items: center;
    .album {
      border: calc(var(--cover-size) * 0.025) solid #ffffff40;
      border-radius: 50%;
      width: 70%;
      height: 70%;
      object-fit: cover;
    }
  }
  .controls {
    width: var(--cover-size);
    display: flex;
    flex-direction: column;
    gap: 1.25rem;
    .song-info {
      display: flex;
      justify-content: space-between;
      align-items: center;
      color: var(--main-cover-color);
      .text {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        .name {
          font-size: 1.5rem;
          font-weight: 600;
        }
        .artists {
          font-size: 1rem;
          opacity: 0.7;
          .artist-name {
            cursor: pointer;
            &:hover {
              opacity: 1;
            }
          }
        }
      }
      .action-row {
        display: flex;
        align-items: center;
        .like-button {
          font-size: 1.75rem;
          cursor: pointer;
          opacity: 0.7;
          transition: opacity 0.2s ease;
        }
      }
    }
    .progress-bar {
      width: 100%;
      display: flex;
      flex-direction: column;
      gap: 0.5rem;
      .slider-wrapper {
        width: 100%;
      }
      .time-info {
        display: flex;
        justify-content: space-between;
        align-items: center;
        color: var(--main-cover-color);
        .time-text {
          font-size: 0.75rem;
          opacity: 0.7;
          min-width: 36px;
          &:last-child {
            text-align: right;
          }
        }
      }
    }
    .buttons {
      display: flex;
      justify-content: space-around;
      align-items: center;
      .play-state {
        .n-button {
          font-size: 3rem;
          color: var(--main-cover-color);
        }
      }
      .button-icon {
        font-size: 1.5rem;
        color: var(--main-cover-color);
        opacity: 0.8;
        cursor: pointer;
        transition:
          opacity 0.2s ease,
          transform 0.1s ease-out;
        &:hover {
          opacity: 1;
        }
        &.active {
          opacity: 1;
          color: var(--primary-color);
        }
      }
    }
    .volume-control {
      display: flex;
      align-items: center;
      :deep(.n-icon) {
        color: var(--main-cover-color);
        opacity: 0.7;
      }
    }
  }
}

// 旋转动画
@keyframes rotate {
  0% {
    transform: rotate(0deg);
  }
  100% {
    transform: rotate(360deg);
  }
}
</style>
