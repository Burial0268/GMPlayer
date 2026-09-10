<template>
  <div class="record">
    <div class="record-stage">
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
        <Motion
          :key="sharedLayoutIds.cover"
          as-child
          :layout-id="sharedLayoutIds.cover"
          :transition="sharedContentTransition"
        >
          <img
            class="album"
            :src="coverUrl(music.getPlaySongData?.album?.picUrl, 500)"
            alt="cover"
          />
        </Motion>
      </div>
    </div>
    <div class="controls">
      <div class="song-info">
        <div class="text">
          <Motion
            :key="sharedLayoutIds.title"
            as-child
            :layout-id="sharedLayoutIds.title"
            :transition="sharedContentTransition"
          >
            <span class="name text-hidden">
              {{ music.getPlaySongData ? music.getPlaySongData.name : $t("other.noSong") }}
            </span>
          </Motion>
          <Motion
            v-if="music.getPlaySongData"
            :key="sharedLayoutIds.artists"
            as-child
            :layout-id="sharedLayoutIds.artists"
            :transition="sharedContentTransition"
          >
            <span class="artists text-hidden">
              <span v-for="(ar, index) in music.getPlaySongData.artist" :key="ar.id">
                <span class="artist-name" @click="routerJump('/artist', { id: ar.id })">{{
                  ar.name
                }}</span>
                <span v-if="index < music.getPlaySongData.artist.length - 1"> / </span>
              </span>
            </span>
          </Motion>
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
        <n-icon class="button-icon" :component="MessageRound" @click.stop="$emit('openComments')" />
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
import { coverUrl } from "@/utils/coverUrl";
import { storeToRefs } from "pinia";
import { useLayerNavigation } from "@/utils/navigation";
import { setSeek } from "@/utils/AudioContext";
import BouncingSlider from "./BouncingSlider.vue";
import { NIcon } from "naive-ui";
import { useI18n } from "vue-i18n";
import { isWindowsTauri } from "@/utils/tauri/core/runtime";
import { windowManager } from "@/utils/tauri/window/manager";
import { Motion } from "motion-v";
import { getDesktopPlayerSharedLayoutIds } from "./desktopSharedLayout";

const navigation = useLayerNavigation();
defineEmits(["openComments"]);
const music = musicStore();
const user = userStore();
const setting = settingStore();
const { persistData } = storeToRefs(music);
const { t } = useI18n();
const isTauriEnv = ref(typeof window !== "undefined" && "__TAURI__" in window);
const sharedLayoutIds = computed(() => getDesktopPlayerSharedLayoutIds(music.getPlaySongData?.id));
const sharedContentTransition = {
  type: "spring",
  stiffness: 180,
  damping: 42,
  mass: 1.35,
  restDelta: 0.001,
  restSpeed: 0.01,
};

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
    setSeek($player, val);
  }
};

// 页面跳转
const routerJump = (url, query) => {
  navigation.openPage({ path: url, query });
};
</script>

<style lang="scss" scoped>
.record {
  /* 高度预算：控件区约 15rem + 纵向 gap 2rem + 上下对称 padding 各 2.5rem；
     stage 高为封面的 1.25 倍，故预算需除以 1.25 */
  --cover-controls-budget: 19.5rem;
  --cover-size: max(
    10rem,
    min(50vh, 38vw, calc((100vh - var(--cover-controls-budget) - 5rem) / 1.18))
  );
  position: relative;
  width: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2rem;
  /* 顶部为绝对定位的关闭手柄保留出画余量（矮窗口不被居中布局顶出视口）；
     底部 padding 额外偏置 0.06×封面：唱臂预留区视觉上偏空，
     纯几何居中会显得整体偏下，此处做光学居中（预算中 1.18 = 1.12 + 0.06） */
  padding: 2.5rem 0 calc(2.5rem + var(--cover-size) * 0.06);

  @media screen and (max-height: 768px) {
    --cover-size: max(
      9rem,
      min(45vh, 38vw, calc((100vh - var(--cover-controls-budget) - 4.5rem) / 1.18))
    );
    gap: 1.5rem;
    padding: 2.25rem 0 calc(2.25rem + var(--cover-size) * 0.06);
  }
  &:hover {
    .control {
      opacity: 1;
    }
  }
  .record-stage {
    position: relative;
    width: min(var(--cover-size), 100%);
    /* 1.12 倍高：压缩唱臂预留区，唱臂与盘面重叠更贴近真实唱机，
       同时减少顶部视觉空腔 */
    aspect-ratio: 1 / 1.12;
    display: grid;
    place-items: end center;

    /* 盘面高光：固定光源，不随唱片旋转（旋转动画在 .pic 上） */
    &::after {
      content: "";
      position: absolute;
      bottom: 0;
      left: 50%;
      transform: translateX(-50%);
      width: min(var(--cover-size), 100%);
      aspect-ratio: 1 / 1;
      border-radius: 50%;
      background: conic-gradient(
        from 210deg,
        transparent 0deg,
        rgb(255 255 255 / 0.07) 42deg,
        transparent 95deg,
        transparent 185deg,
        rgb(255 255 255 / 0.04) 235deg,
        transparent 285deg
      );
      pointer-events: none;
      z-index: 1;
    }

    .pointer {
      position: absolute;
      width: calc(var(--cover-size) * 0.35);
      left: calc(50% - var(--cover-size) * 0.045);
      top: calc(var(--cover-size) * 0.02);
      transform: rotate(-20deg);
      transform-origin: calc(var(--cover-size) * 0.045) calc(var(--cover-size) * 0.045);
      z-index: 2;
      filter: drop-shadow(0 6px 10px rgb(0 0 0 / 0.3));
      transition: transform var(--duration-400) var(--ease-out);
      &.play {
        transform: rotate(0);
      }
    }
    .pic {
      position: relative;
      animation: rotate 18s linear infinite;
      border-radius: 50%;
      width: min(var(--cover-size), 100%);
      height: auto;
      aspect-ratio: 1 / 1;
      display: flex;
      justify-content: center;
      align-items: center;
      /* 黑胶盘面：细密沟槽 + 低对比中心过渡，替代原先的粗黑灰环 */
      background:
        radial-gradient(circle at center, rgb(255 255 255 / 0.05) 0%, transparent 60%),
        repeating-radial-gradient(
          circle at center,
          #0b0b0d 0,
          #0b0b0d 2.5px,
          #191a1d 3px,
          #0b0b0d 3.5px
        ),
        #0b0b0d;
      box-shadow:
        0 20px 46px rgb(0 0 0 / 0.38),
        inset 0 0 0 1px rgb(255 255 255 / 0.12),
        inset 0 0 calc(var(--cover-size) * 0.12) rgb(0 0 0 / 0.55);

      /* 中心轴点 */
      &::after {
        content: "";
        position: absolute;
        width: calc(var(--cover-size) * 0.02);
        height: calc(var(--cover-size) * 0.02);
        border-radius: 50%;
        background: #e6e6e6;
        box-shadow:
          0 0 0 2px rgb(0 0 0 / 0.65),
          0 1px 3px rgb(0 0 0 / 0.5);
        z-index: 2;
      }

      .album {
        border-radius: 50%;
        width: 62%;
        height: 62%;
        object-fit: cover;
        border: 1px solid rgb(255 255 255 / 0.16);
        /* 标签与盘面间的过渡圈 */
        box-shadow:
          0 0 0 calc(var(--cover-size) * 0.012) rgb(0 0 0 / 0.42),
          0 0 calc(var(--cover-size) * 0.05) rgb(0 0 0 / 0.3);
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
          transition: opacity var(--duration-200) var(--ease-out);
        }
        .more-button {
          font-size: 1.75rem;
          cursor: pointer;
          opacity: 1;
          transition: opacity var(--duration-200) var(--ease-out);
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
          opacity var(--duration-200) var(--ease-out),
          transform var(--duration-150) var(--ease-out);
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
