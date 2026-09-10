<template>
  <div ref="coverContainerRef" class="player-cover-container">
    <div class="cover-stage">
      <Transition name="fade" mode="out-in">
        <div
          :key="`cover_pic--${music.getPlaySongData?.album?.pic ?? defaultCover}`"
          :class="[
            'pic',
            !music.getPlayState ? 'pause' : '',
            music.getLoadingState ? 'loading' : '',
          ]"
        >
          <Motion
            :key="sharedLayoutIds.cover"
            as-child
            :layout-id="sharedLayoutIds.cover"
            :transition="sharedContentTransition"
          >
            <!-- `decoding="async"` because a local cover is stored at whatever
                 resolution the file embedded — often 1500-3000px, with no
                 server-side `?param=` resize to lean on — and a synchronous
                 decode of that lands on the main thread at every track change. -->
            <img
              class="album"
              decoding="async"
              :src="coverUrl(music.getPlaySongData?.album?.picUrl, 1024)"
              alt="cover"
            />
          </Motion>
        </div>
      </Transition>
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
          <div v-if="qualityText" class="quality-badge">
            <n-icon :component="IconLossless" />
            <span class="quality-label">{{ qualityText }}</span>
          </div>
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
  MoreHorizRound,
  ThumbDownRound,
  StarBorderRound,
  StarRound,
  VolumeOffRound,
  VolumeUpRound,
  MessageRound,
  PictureInPictureAltRound,
  ClosedCaptionRound,
  SubtitlesRound,
} from "@vicons/material";
import { computed, h, onMounted, ref } from "vue";
import IconForward from "./icons/IconForward.vue";
import IconRewind from "./icons/IconRewind.vue";
import IconPlay from "./icons/IconPlay.vue";
import IconLossless from "./icons/IconLossless.vue";
import IconPause from "./icons/IconPause.vue";
import { ShuffleOne, PlayOnce, PlayCycle } from "@icon-park/vue-next";
import { musicStore, userStore, settingStore } from "@/store";
import { storeToRefs } from "pinia";
import { useLayerNavigation } from "@/utils/navigation";
import { setSeek } from "@/utils/AudioContext";
import { coverUrl } from "@/utils/coverUrl";
import { NativeRustSound } from "@/utils/tauri/audio/nativeRustSound";
import BouncingSlider from "./BouncingSlider.vue";
import defaultCover from "/images/pic/default.png?url";
import gsap from "gsap";
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
// Shared elements should hand off only between views of the same song. Remounting
// them under a song-scoped layout id prevents track changes from scaling old
// artwork/text into the new song's geometry.
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

// 音质标签
const qualityLevelText = computed(() => {
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

const nativeAudioQuality = computed(() => {
  if (!music.getPlaySongData || music.getLoadingState) return null;
  const player = typeof window !== "undefined" ? window.$player : undefined;
  if (!(player instanceof NativeRustSound)) return null;
  return player.getAudioQuality();
});

const formatSampleRate = (sampleRate) => {
  if (!Number.isFinite(sampleRate) || sampleRate <= 0) return null;
  const khz = sampleRate / 1000;
  return `${Number.isInteger(khz) ? khz.toFixed(0) : khz.toFixed(1)} kHz`;
};

const formatBitrate = (bitrate) => {
  if (!Number.isFinite(bitrate) || bitrate <= 0) return null;
  const kbps = bitrate / 1000;
  return `${kbps >= 100 ? Math.round(kbps) : kbps.toFixed(1)} kbps`;
};

const qualityText = computed(() => {
  const quality = nativeAudioQuality.value;
  if (!quality) return qualityLevelText.value;

  const details = [formatSampleRate(quality.sampleRate), formatBitrate(quality.bitrate)].filter(
    Boolean,
  );
  if (!details.length) return qualityLevelText.value;
  return [qualityLevelText.value, ...details].filter(Boolean).join(" · ");
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
    setSeek($player, val);
  }
};

// 页面跳转
const routerJump = (url, query) => {
  navigation.openPage({
    path: url,
    query,
  });
};

// GSAP 动画
// Scoped to this component's own buttons (a document-wide query would attach
// to other components' .button-icon nodes and outlive this instance), and
// removed on unmount so playerStyle toggles don't stack orphaned closures.
const coverContainerRef = ref(null);
const buttonAnimationCleanups = [];
// 卸载时要 kill 的目标。不能用 gsap.context：context 只在它那个函数**同步执行
// 期间**把新建的 tween 记进作用域（gsap-core 里 Context.add 在调用前后设置 /
// 还原模块级的 _context，Animation 构造函数只在 _context 有值时 push 自己）。
// 这里 tween 全是之后在事件回调里建的，那时 _context 早已还原成 null，
// ctx.data 始终是空的，revert() 什么都不会 kill、也不会还原内联样式。
// 直接留住节点、卸载时 killTweensOf 才真的有效。
let animatedButtons = [];

// 每个交互的时长/缓动是刻意不同的（按下更快更硬），所以这里保留 gsap.to 而
// 不是收敛成一个 quickTo。overwrite: "auto" 是关键：快速进出时新 tween 会
// 接管同一目标上同属性的旧 tween，否则两条 tween 会同时 tick 并互相抢写。
const BUTTON_SCALE_TWEENS = {
  enter: { scale: 1.1, duration: 0.2, ease: "power1.out" },
  leave: { scale: 1, duration: 0.2, ease: "power1.inOut" },
  down: { scale: 0.9, duration: 0.1, ease: "power1.in" },
  up: { scale: 1.1, duration: 0.2, ease: "power1.out" },
};

onMounted(() => {
  const root = coverContainerRef.value;
  if (!root) return;
  animatedButtons = Array.from(root.querySelectorAll(".button-icon"));
  animatedButtons.forEach((button) => {
    const tweenTo = (vars) => gsap.to(button, { ...vars, overwrite: "auto" });
    const onMouseEnter = () => tweenTo(BUTTON_SCALE_TWEENS.enter);
    const onMouseLeave = () => tweenTo(BUTTON_SCALE_TWEENS.leave);
    const onMouseDown = () => tweenTo(BUTTON_SCALE_TWEENS.down);
    const onMouseUp = () => tweenTo(BUTTON_SCALE_TWEENS.up);

    button.addEventListener("mouseenter", onMouseEnter);
    button.addEventListener("mouseleave", onMouseLeave);
    button.addEventListener("mousedown", onMouseDown);
    button.addEventListener("mouseup", onMouseUp);
    buttonAnimationCleanups.push(() => {
      button.removeEventListener("mouseenter", onMouseEnter);
      button.removeEventListener("mouseleave", onMouseLeave);
      button.removeEventListener("mousedown", onMouseDown);
      button.removeEventListener("mouseup", onMouseUp);
    });
  });
});

onUnmounted(() => {
  buttonAnimationCleanups.forEach((cleanup) => cleanup());
  buttonAnimationCleanups.length = 0;
  if (animatedButtons.length) {
    // 悬停中卸载（点歌手名跳转、切 playerStyle）时 tween 还在飞：不 kill 的话
    // 它会继续对着已脱离文档的节点跑完。clearProps 抹掉 GSAP 写在行内的
    // transform，这样 DOM 被复用时不会残留 scale。
    gsap.killTweensOf(animatedButtons);
    gsap.set(animatedButtons, { clearProps: "transform" });
    animatedButtons = [];
  }
});
</script>

<style lang="scss" scoped>
.player-cover-container {
  /* 高度预算：控件区约 15rem + 纵向 gap 2rem + 上下对称 padding 各 2.5rem */
  --cover-controls-budget: 19.5rem;
  --cover-size: max(10rem, min(50vh, 38vw, calc(100vh - var(--cover-controls-budget) - 5rem)));
  width: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2rem;
  /* 顶部为绝对定位的关闭手柄保留出画余量（矮窗口不被居中布局顶出视口）；
     底部等量 padding 保持视觉中心对称 */
  padding: 2.5rem 0;

  @media screen and (max-height: 768px) {
    --cover-size: max(9rem, min(45vh, 38vw, calc(100vh - var(--cover-controls-budget) - 4.5rem)));
    gap: 1.5rem;
    padding: 2.25rem 0;
  }

  .cover-stage {
    position: relative;
    /* 同时受容器实际宽度约束（.left 为 40% 宽减内边距，38vw 在窄局部会超宽） */
    width: min(var(--cover-size), 100%);
    aspect-ratio: 1 / 1;
  }

  .pic {
    position: relative;
    width: 100%;
    height: 100%;
    border-radius: var(--radius-panel);
    transition:
      transform var(--duration-500) var(--ease-out),
      filter var(--duration-500) var(--ease-out);
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
      border-radius: var(--radius-panel);
    }
  }
  .controls {
    width: min(var(--cover-size), 100%);
    /* 封面被矮窗口压得很小时，控件区保底宽度，避免按钮/时间挤作一团 */
    min-width: min(18rem, 100%);
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
        mix-blend-mode: plus-lighter;
        flex-direction: column;
        gap: 0.25rem;
        .name {
          font-size: clamp(1.15rem, calc(var(--cover-size) * 0.055), 1.5rem);
          font-weight: 600;
        }
        .artists {
          font-size: clamp(0.85rem, calc(var(--cover-size) * 0.04), 1rem);
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
        mix-blend-mode: plus-lighter;
        gap: 0.5rem;
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
        gap: 8px;
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
        .quality-badge {
          display: flex;
          align-items: center;
          gap: 4px;
          flex: 0 1 auto;
          min-width: 0;
          max-width: calc(100% - 88px);
          background-color: rgba(255, 255, 255, 0.1);
          color: var(--main-cover-mix-color, rgb(239, 239, 239));
          opacity: 1;
          font-size: 0.75rem;
          padding: 2px 8px;
          border-radius: var(--radius-xs);
          white-space: nowrap;
          .wave-icon {
            width: 14px;
            height: 14px;
          }
          .quality-label {
            min-width: 0;
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
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
</style>
