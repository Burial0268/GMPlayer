<template>
  <div class="player-cover-container">
    <Transition name="fade" mode="out-in">
      <div
        :key="`cover_pic--${music.getPlaySongData?.album?.pic ?? defaultCover}`"
        :class="[
          'pic',
          !music.getPlayState ? 'pause' : '',
          music.getLoadingState ? 'loading' : '',
        ]"
      >
        <img
          class="album"
          :src="
            music.getPlaySongData && music.getPlaySongData.album
          ? music.getPlaySongData.album.picUrl.replace(/^http:/, 'https:') +
          '?param=1024y1024'
          : '/images/pic/default.png'
          "
          alt="cover"
        />
      </div>
    </Transition>
    <div class="controls">
      <div class="song-info">
        <div class="text">
          <span class="name text-hidden">
            {{
              music.getPlaySongData
                ? music.getPlaySongData.name
                : $t("other.noSong")
            }}
          </span>
          <span
            v-if="music.getPlaySongData"
            class="artists text-hidden"
          >
            <span
              v-for="(ar, index) in music.getPlaySongData.artist"
              :key="ar.id"
            >
              <span class="artist-name" @click="routerJump('/artist', { id: ar.id })">{{ ar.name }}</span>
              <span v-if="index < music.getPlaySongData.artist.length - 1"> / </span>
            </span>
          </span>
        </div>
        <div class="action-row">
          <n-icon class="like-button" size="24" :component="music.getPlaySongData && music.getSongIsLike(music.getPlaySongData.id) ? StarRound : StarBorderRound" @click.stop="
              music.getPlaySongData && (
                music.getSongIsLike(music.getPlaySongData.id)
                  ? music.changeLikeList(music.getPlaySongData.id, false)
                  : music.changeLikeList(music.getPlaySongData.id, true)
              )
        " />
        <n-icon
          v-if="music.getPlaySongData"
          class="more-button"
          size="24"
          :component="MoreHorizRound"
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
          <span class="time-text">{{
            music.getPlaySongTime.songTimePlayed
          }}</span>
          <div v-if="qualityText" class="quality-badge">
            <n-icon :component="IconLossless" />
            {{ qualityText }}
          </div>
          <span class="time-text">{{ remainingTime }}</span>
        </div>
      </div>
      <div class="buttons">
        <n-icon
          :style="
            music.getPersonalFmMode
          ? 'opacity: 0.2;pointer-events: none;'
          : null
          "
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
        <n-icon
          class="button-icon"
          :component="MessageRound"
          @click="goToComment"
        />
      </div>
      <div class="volume-control">
        <BouncingSlider
          :value="persistData.playVolume"
          :min="0"
          :max="1"
          :change-on-drag="true"
          @update:value="val => persistData.playVolume = val"
        >
          <template #before-icon>
            <n-icon
              size="18"
              :component="VolumeOffRound"
            />
          </template>
          <template #after-icon>
            <n-icon
              size="18"
              :component="VolumeUpRound"
            />
          </template>
        </BouncingSlider>
      </div>
    </div>
  </div>
</template>

<script setup>
import {
  MoreHorizRound,
  ThumbDownRound,
  StarBorderRound,
  StarRound,
  VolumeOffRound,
  VolumeUpRound,
  MessageRound,
} from "@vicons/material";
import { computed, onMounted } from "vue";
import IconForward from "./icons/IconForward.vue";
import IconRewind from "./icons/IconRewind.vue";
import IconPlay from "./icons/IconPlay.vue";
import IconLossless from "./icons/IconLossless.vue";
import IconPause from "./icons/IconPause.vue";
import { ShuffleOne, PlayOnce, PlayCycle } from "@icon-park/vue-next";
import { musicStore, userStore, settingStore } from "@/store";
import { storeToRefs } from "pinia";
import { useRouter } from "vue-router";
import { setSeek } from "@/utils/AudioContext";
import BouncingSlider from "./BouncingSlider.vue";
import defaultCover from "/images/pic/default.png?url";
import gsap from "gsap";

const router = useRouter();
const music = musicStore();
const user = userStore();
const setting = settingStore();
const { persistData } = storeToRefs(music);

// 音质标签
const qualityText = computed(() => {
  const level = setting.songLevel;
  const qualityMap = {
    standard: "标准",
    higher: "较高",
    exhigh: "极高",
    lossless: "无损",
    hires: "Hi-Res",
  };
  return qualityMap[level] || null;
});

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

// 循环切换播放模式: normal → random → single → normal
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
  router.push({
    path: url,
    query,
  });
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

// GSAP 动画
onMounted(() => {
  const buttons = document.querySelectorAll(".button-icon, .play-state");
  buttons.forEach((button) => {
    // 悬停动画
    button.addEventListener("mouseenter", () => {
      gsap.to(button, {
        scale: 1.1,
        duration: 0.2,
        ease: "power1.out",
      });
    });
    button.addEventListener("mouseleave", () => {
      gsap.to(button, {
        scale: 1,
        duration: 0.2,
        ease: "power1.inOut",
      });
    });
    // 点击动画
    button.addEventListener("mousedown", () => {
      gsap.to(button, {
        scale: 0.9,
        duration: 0.1,
        ease: "power1.in",
      });
    });
    button.addEventListener("mouseup", () => {
      gsap.to(button, {
        scale: 1.1,
        duration: 0.2,
        ease: "power1.out",
      });
    });
  });
});
</script>

<style lang="scss" scoped>
.player-cover-container {
  --cover-size: min(50vh, 38vw);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2rem;

  @media screen and (max-height: 768px) {
    --cover-size: min(45vh, 38vw);
    gap: 1.5rem;
  }

  .pic {
    position: relative;
    width: var(--cover-size);
    height: var(--cover-size);
    border-radius: 12px;
    transition: transform 0.5s ease-out, filter 0.5s ease-out;
    &.pause {
      transform: scale(0.95);
    }
    &.loading {
      transform: scale(0.95);
      filter: grayscale(0.8);
    }
    .album {
      width: 100%;
      height: 100%;
      border-radius: 12px;
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

      .more-button {
        font-size: 1.75rem;
        cursor: pointer;
        opacity: 0.7;
        transition: opacity 0.2s ease;
        &:hover {
          opacity: 1;
        }
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
        .quality-badge {
          display: flex;
          align-items: center;
          gap: 4px;
          background-color: rgba(255, 255, 255, 0.1);
          color: var(--main-cover-color);
          opacity: 0.8;
          font-size: 0.75rem;
          padding: 2px 8px;
          border-radius: 4px;
          white-space: nowrap;
          .wave-icon {
            width: 14px;
            height: 14px;
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
        transition: opacity 0.2s ease, transform 0.1s ease-out;
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
</style>
