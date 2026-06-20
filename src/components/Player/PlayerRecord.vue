<template>
  <div class="record">
    <div class="record-stage">
      <div class="amll-close-action">
        <ControlThumb aria-label="Close player" @click="closeBigPlayer" />
      </div>
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
          <n-dropdown
            v-if="music.getPlaySongData && moreOptions.length"
            :options="moreOptions"
            trigger="click"
            placement="bottom-end"
            @select="handleMoreSelect"
          >
            <n-icon class="more-button" size="24" :component="MoreHorizRound" />
          </n-dropdown>
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
          class="button-icon skip-icon"
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
        <n-icon
          class="button-icon"
          :class="{ loading: music.getLoadingState }"
          :component="music.getPlayState ? IconPause : IconPlay"
          @click.stop="!music.getLoadingState && music.setPlayState(!music.getPlayState)"
        />
        <n-icon
          class="button-icon skip-icon"
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
  ThumbDownRound,
  StarBorderRound,
  StarRound,
  VolumeOffRound,
  VolumeUpRound,
  MessageRound,
  MoreHorizRound,
  PictureInPictureAltRound,
  ClosedCaptionRound,
  SubtitlesRound,
} from "@vicons/material";
import { ShuffleOne, PlayOnce, PlayCycle } from "@icon-park/vue-next";
import { computed, h, ref } from "vue";
import IconForward from "./icons/IconForward.vue";
import IconRewind from "./icons/IconRewind.vue";
import IconPlay from "./icons/IconPlay.vue";
import IconPause from "./icons/IconPause.vue";
import { musicStore, settingStore, userStore } from "@/store";
import { storeToRefs } from "pinia";
import { useRouter } from "vue-router";
import { setSeek } from "@/utils/AudioContext";
import BouncingSlider from "./BouncingSlider.vue";
import ControlThumb from "./ControlThumb.vue";
import { NIcon } from "naive-ui";
import { useI18n } from "vue-i18n";
import { isWindowsTauri, windowManager } from "@/utils/tauri/windowManager";

const router = useRouter();
const music = musicStore();
const user = userStore();
const setting = settingStore();
const { persistData } = storeToRefs(music);
const { t } = useI18n();
const isTauriEnv = ref(typeof window !== "undefined" && "__TAURI__" in window);

// MiniPlayer / DesktopLyrics 切换
const toggleMiniPlayer = async () => {
  const state = await windowManager.getWindowState("mini-player");
  if (state?.exists) {
    windowManager.toggleWindow("mini-player");
  } else {
    windowManager.createWindow("mini-player");
  }
};

const toggleDesktopLyrics = async () => {
  const state = await windowManager.getWindowState("desktop-lyrics");
  if (state?.exists) {
    if (state.visible) {
      const tauri = window.__TAURI__;
      if (tauri) await tauri.event.emit("desktop-lyrics-unlock");
    } else {
      windowManager.showWindow("desktop-lyrics");
    }
  } else {
    windowManager.createWindow("desktop-lyrics");
  }
};

const openTaskbarLyrics = async () => {
  await windowManager.openTaskbarLyrics();
};

// 更多菜单
const renderIcon = (icon) => () => h(NIcon, { size: 18 }, { default: () => h(icon) });

const moreOptions = computed(() => {
  const options = [];
  if (isTauriEnv.value) {
    options.push(
      {
        label: t("setting.miniPlayer"),
        key: "miniPlayer",
        icon: renderIcon(PictureInPictureAltRound),
      },
      { label: t("setting.desktopLyrics"), key: "desktopLyrics", icon: renderIcon(SubtitlesRound) },
    );
    if (setting.taskbarLyrics && isWindowsTauri()) {
      options.push({
        label: t("setting.taskbarLyrics"),
        key: "taskbarLyrics",
        icon: renderIcon(ClosedCaptionRound),
      });
    }
  }
  return options;
});

const handleMoreSelect = (key) => {
  if (key === "miniPlayer") toggleMiniPlayer();
  else if (key === "desktopLyrics") toggleDesktopLyrics();
  else if (key === "taskbarLyrics") openTaskbarLyrics();
};

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

const closeBigPlayer = () => {
  music.setBigPlayerState(false);
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
  .record-stage {
    position: relative;
    width: var(--cover-size);
    height: calc(var(--cover-size) * 1.25);
    display: grid;
    place-items: end center;

    .amll-close-action {
      position: absolute;
      left: 50%;
      bottom: calc(100% + 1.5rem);
      width: 0;
      height: 0;
      z-index: 3;
      mix-blend-mode: plus-lighter;
    }

    .pointer {
      position: absolute;
      width: calc(var(--cover-size) * 0.35);
      left: calc(50% - var(--cover-size) * 0.045);
      top: calc(var(--cover-size) * 0.02);
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
        width: 64%;
        height: 64%;
        object-fit: cover;
      }
    }
  }
  .controls {
    width: var(--cover-size);
    display: flex;
    flex-direction: column;
    gap: 1.25rem;
    color: var(--main-cover-mix-color, rgb(239, 239, 239));

    :deep(.n-icon),
    :deep(svg),
    :deep(path) {
      mix-blend-mode: plus-lighter;
    }

    :deep(.bouncing-slider) {
      mix-blend-mode: plus-lighter;
    }

    .song-info {
      display: flex;
      justify-content: space-between;
      align-items: center;
      color: var(--main-cover-mix-color, rgb(239, 239, 239));
      .text {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        mix-blend-mode: plus-lighter;
        .name {
          font-size: 1.5rem;
          font-weight: 600;
        }
        .artists {
          font-size: 1rem;
          opacity: 1;
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
        gap: 0.5rem;
        mix-blend-mode: plus-lighter;
        .like-button {
          font-size: 1.75rem;
          cursor: pointer;
          opacity: 1;
          transition: opacity 0.2s ease;
        }
        .more-button {
          font-size: 1.75rem;
          cursor: pointer;
          opacity: 1;
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
        color: var(--main-cover-mix-color, rgb(239, 239, 239));
        .time-text {
          font-size: 0.75rem;
          opacity: 1;
          min-width: 36px;
          mix-blend-mode: plus-lighter;
          &:last-child {
            text-align: right;
          }
        }
      }
    }
    .buttons {
      display: grid;
      grid-template-columns: repeat(5, minmax(0, 1fr));
      align-items: center;
      justify-items: center;
      column-gap: clamp(0.25rem, calc(var(--cover-size) * 0.025), 0.75rem);

      > * {
        min-width: 0;
        justify-self: center;
      }

      .button-icon {
        width: clamp(2.25rem, calc(var(--cover-size) * 0.1), 3rem);
        height: clamp(2.25rem, calc(var(--cover-size) * 0.1), 3rem);
        display: flex;
        align-items: center;
        justify-content: center;
        font-size: clamp(1.65rem, calc(var(--cover-size) * 0.072), 1.95rem);
        color: var(--main-cover-mix-color, rgb(239, 239, 239));
        mix-blend-mode: plus-lighter;
        opacity: 1;
        cursor: pointer;
        transition:
          opacity 0.2s ease,
          transform 0.1s ease-out;
        &:hover {
          opacity: 1;
        }
        &.loading {
          opacity: 0.35;
          pointer-events: none;
        }
        &.active {
          opacity: 1;
          color: var(--main-cover-mix-color, rgb(239, 239, 239));
        }

        &.skip-icon {
          font-size: clamp(2.25rem, calc(var(--cover-size) * 0.105), 2.85rem);
        }
      }
    }
    .volume-control {
      display: flex;
      align-items: center;
      :deep(.n-icon) {
        color: var(--main-cover-mix-color, rgb(239, 239, 239));
        opacity: 1;
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
