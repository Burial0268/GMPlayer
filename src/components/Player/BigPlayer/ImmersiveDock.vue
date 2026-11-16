<template>
  <div class="immersive-dock">
    <div class="dock-content">
      <div class="dock-head">
        <div class="dock-text">
          <span class="dock-name">
            <Transition name="automix-meta" mode="out-in">
              <OverflowMarquee :key="music.getPlaySongData?.id" :speed="36">
                {{ music.getPlaySongData ? music.getPlaySongData.name : $t("other.noSong") }}
              </OverflowMarquee>
            </Transition>
          </span>
          <span v-if="artistList.length" class="dock-artists">
            <OverflowMarquee>
              <template v-for="(ar, index) in artistList" :key="ar.id">
                <span v-button class="artist-name" @click="routerJump('/artist', { id: ar.id })">
                  {{ ar.name }}
                </span>
                <span v-if="index < artistList.length - 1">&nbsp;/&nbsp;</span>
              </template>
            </OverflowMarquee>
          </span>
        </div>
        <button
          class="dock-icon"
          type="button"
          :aria-label="isLiked ? $t('player.controls.unlike') : $t('player.controls.like')"
          :aria-pressed="isLiked"
          @click.stop="toggleLike"
        >
          <n-icon :component="isLiked ? StarRound : StarBorderRound" />
        </button>
        <n-dropdown
          v-if="moreOptions.length"
          :options="moreOptions"
          trigger="click"
          placement="top-end"
          @select="handleMoreSelect"
        >
          <button class="dock-icon" type="button" :aria-label="$t('player.controls.more')">
            <n-icon :component="MoreHorizRound" />
          </button>
        </n-dropdown>
      </div>

      <div class="dock-progress">
        <BouncingSlider
          :value="music.getPlaySongTime.currentTime || 0"
          :min="0"
          :max="music.getPlaySongTime.duration || 1"
          :is-playing="music.getPlayState"
          :transition-phase="autoMixBarPhase"
          @update:value="handleProgressSeek"
        />
        <div class="dock-time">
          <span>{{ elapsedLabel }}</span>
          <span>{{ remainingTime }}</span>
        </div>
      </div>

      <div class="dock-controls">
        <div class="dock-transport">
          <button
            class="dock-button"
            type="button"
            :class="{ active: music.getPlaySongMode !== 'normal' }"
            :disabled="music.getPersonalFmMode"
            :aria-label="$t('player.controls.playMode')"
            @click.stop="music.setPlaySongMode()"
          >
            <n-icon :component="playModeIcon" />
          </button>
          <button
            v-if="!music.getPersonalFmMode"
            class="dock-button skip"
            type="button"
            :aria-label="$t('player.controls.previous')"
            @click.stop="music.setPlaySongIndex('prev')"
          >
            <n-icon :component="IconRewind" />
          </button>
          <button
            v-else
            class="dock-button"
            type="button"
            :disabled="!user.userLogin"
            :aria-label="$t('player.controls.dislike')"
            @click.stop="music.setFmDislike(music.getPersonalFmData.id)"
          >
            <n-icon :component="ThumbDownRound" />
          </button>
          <button
            class="dock-button"
            type="button"
            :class="{ loading: loadBlocksPlay }"
            :aria-busy="music.getLoadingState"
            :aria-label="
              music.getPlayState ? $t('player.controls.pause') : $t('player.controls.play')
            "
            @click.stop="!loadBlocksPlay && music.setPlayState(!music.getPlayState)"
          >
            <!-- Not :disabled — that dims to 0.2 and would hide the spinner. -->
            <Transition name="fade" mode="out-in">
              <n-spin v-if="music.getLoadingState" :size="22" stroke="currentColor" />
              <n-icon v-else :component="music.getPlayState ? IconPause : IconPlay" />
            </Transition>
          </button>
          <button
            class="dock-button skip"
            type="button"
            :aria-label="$t('player.controls.next')"
            @click.stop="music.setPlaySongIndex('next')"
          >
            <n-icon :component="IconForward" />
          </button>
          <button
            class="dock-button"
            type="button"
            :aria-label="$t('player.controls.comment')"
            @click.stop="emit('openComments')"
          >
            <n-icon :component="MessageRound" />
          </button>
        </div>

        <BouncingSlider
          class="dock-volume"
          :value="persistData.playVolume"
          :min="0"
          :max="1"
          :change-on-drag="true"
          @update:value="(val) => (persistData.playVolume = val)"
        >
          <template #before-icon>
            <n-icon :component="volumeIcon" />
          </template>
        </BouncingSlider>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { vButton } from "@/utils/buttonRole";
import {
  ClosedCaptionRound,
  MessageRound,
  MoreHorizRound,
  PictureInPictureAltRound,
  StarBorderRound,
  StarRound,
  SubtitlesRound,
  ThumbDownRound,
  VolumeOffRound,
  VolumeUpRound,
} from "@vicons/material";
import { PlayCycle, PlayOnce, ShuffleOne } from "@icon-park/vue-next";
import { computed, h, ref, type Component } from "vue";
import { emit as emitTauriEvent } from "@tauri-apps/api/event";
import { NIcon } from "naive-ui";
import { storeToRefs } from "pinia";
import { useI18n } from "vue-i18n";
import { useLayerNavigation } from "@/utils/navigation";
import type { LocationQueryRaw } from "vue-router";
import { musicStore, settingStore, userStore } from "@/store";
import {
  useAutoMixBarPhase,
  useAutoMixElapsed,
  useAutoMixTransition,
} from "@/composables/useAutoMixTransition";
import { isWindowsTauri } from "@/utils/tauri/core/runtime";
import { windowManager } from "@/utils/tauri/window/manager";
import BouncingSlider from "../BouncingSlider.vue";
import IconForward from "../icons/IconForward.vue";
import IconPause from "../icons/IconPause.vue";
import IconPlay from "../icons/IconPlay.vue";
import IconRewind from "../icons/IconRewind.vue";

defineProps<{
  handleProgressSeek: (val: number) => void;
}>();

const emit = defineEmits<{
  openComments: [];
}>();

const navigation = useLayerNavigation();
const music = musicStore();
const autoMixBarPhase = useAutoMixBarPhase();
const autoMixTransitioning = useAutoMixTransition(autoMixBarPhase);
const elapsedLabel = useAutoMixElapsed(autoMixTransitioning);
const user = userStore();
const setting = settingStore();
const { persistData } = storeToRefs(music);
const { t } = useI18n();

const isTauriEnv = ref(typeof window !== "undefined" && "__TAURI__" in window);

const artistList = computed(() => music.getPlaySongData?.artist ?? []);
const isLiked = computed(
  () => !!music.getPlaySongData && music.getSongIsLike(music.getPlaySongData.id),
);

const loadBlocksPlay = computed(
  () => music.getLoadingState && music.getLoadingStage !== "rebuffering",
);

// 剩余时间（负数格式，与封面模式一致）
const remainingTime = computed(() => {
  const songTime = music.getPlaySongTime;
  if (!songTime?.duration) return "-0:00";
  const remainingSeconds = Math.max(0, songTime.duration - (songTime.currentTime || 0));
  const minutes = Math.floor(remainingSeconds / 60);
  const seconds = Math.floor(remainingSeconds % 60);
  return `-${minutes}:${seconds.toString().padStart(2, "0")}`;
});

const playModeIcon = computed(() => {
  const mode = music.getPlaySongMode;
  if (mode === "random") return ShuffleOne;
  if (mode === "single") return PlayOnce;
  return PlayCycle;
});

// 卡片窄，音量条只保留一个随音量切换的图标而不是首尾各一个
const volumeIcon = computed(() =>
  persistData.value.playVolume > 0 ? VolumeUpRound : VolumeOffRound,
);

const toggleLike = () => {
  const song = music.getPlaySongData;
  if (!song) return;
  music.changeLikeList(song.id, !music.getSongIsLike(song.id));
};

// 更多菜单：与封面模式同一组从属窗口开关
const renderIcon = (icon: Component) => () => h(NIcon, { size: 18 }, { default: () => h(icon) });

const moreOptions = computed(() => {
  if (!isTauriEnv.value) return [];
  const options = [
    {
      label: t("setting.miniPlayer"),
      key: "miniPlayer",
      icon: renderIcon(PictureInPictureAltRound),
    },
    { label: t("setting.desktopLyrics"), key: "desktopLyrics", icon: renderIcon(SubtitlesRound) },
  ];
  if (setting.taskbarLyrics && isWindowsTauri()) {
    options.push({
      label: t("setting.taskbarLyrics"),
      key: "taskbarLyrics",
      icon: renderIcon(ClosedCaptionRound),
    });
  }
  return options;
});

const toggleMiniPlayer = async () => {
  const state = await windowManager.getWindowState("mini-player");
  if (state?.exists) windowManager.toggleWindow("mini-player");
  else windowManager.createWindow("mini-player");
};

const toggleDesktopLyrics = async () => {
  const state = await windowManager.getWindowState("desktop-lyrics");
  if (!state?.exists) {
    windowManager.createWindow("desktop-lyrics");
    return;
  }
  if (state.visible) await emitTauriEvent("desktop-lyrics-unlock");
  else windowManager.showWindow("desktop-lyrics");
};

const handleMoreSelect = (key: string) => {
  if (key === "miniPlayer") toggleMiniPlayer();
  else if (key === "desktopLyrics") toggleDesktopLyrics();
  else if (key === "taskbarLyrics") windowManager.openTaskbarLyrics();
};

const routerJump = (url: string, query: LocationQueryRaw) => {
  navigation.openPage({ path: url, query });
};
</script>

<style lang="scss" scoped>
// 沉浸模式的浮动控制：没有卡片底，控件直接浮在封面底部的虚化带上
// （ImmersivePlayerLayout 的 .immersive-artwork-veil 负责模糊 + 压暗）。
// 曾经用一张对齐的模糊封面副本做「假玻璃」，但它没有封面右缘的椭圆渐隐、
// 不跟封面缩放、也不随 AutoMix 换图，叠在已经糊过的虚化带上只像一块暗盒子。
// .dock-content 的 plus-lighter 要混到身下的封面上，所以本节点不能带
// blend / filter / opacity / mask 之类的层叠隔断。
.immersive-dock {
  // 宽度/定位由 .immersive-dock-slot 统一负责（纯 vw，无上下限）。
  // 内部字号、图标、间距同样按 vw 缩放并各自 clamp。
  position: relative;
  width: 100%;
  box-sizing: border-box;
}

.dock-content {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: clamp(0.4rem, 0.55vw, 0.8rem);
  padding: clamp(0.55rem, 0.72vw, 1rem) clamp(0.62rem, 0.82vw, 1.15rem)
    clamp(0.6rem, 0.78vw, 1.1rem);
  color: var(--main-cover-color);
  mix-blend-mode: plus-lighter;
}

// 图标直接做 .dock-head 的 flex 子项，省掉一层 actions 容器：
// gap 只负责两个图标之间的 0.35rem，文本与图标组之间的 0.75rem 由
// .dock-text 的 margin-right 补足（等价于原来的 space-between + gap: 0.75rem）。
.dock-head {
  display: flex;
  align-items: flex-start;
  gap: 0.35rem;
  min-width: 0;
}

.dock-text {
  flex: 1 1 auto;
  min-width: 0;
  margin-right: 0.4rem;
  display: flex;
  flex-direction: column;
  gap: 0.1rem;

  .dock-name {
    font-size: clamp(0.85rem, 1.1vw, 1.2rem);
    font-weight: 650;
    line-height: 1.25;
  }

  .dock-artists {
    font-size: clamp(0.68rem, 0.85vw, 0.92rem);
    opacity: 0.82;

    .artist-name {
      cursor: pointer;

      @media (hover: hover) {
        &:hover {
          opacity: 1;
          text-decoration: underline;
        }
      }
    }
  }
}

// 图标控件一律是真正的 <button>：原来用 <n-icon @click> 渲染出的是 <i>，
// 键盘 Tab 不到、回车/空格无效、读屏器也读不出可操作性。
%dock-icon-button {
  appearance: none;
  border: none;
  padding: 0;
  background: transparent;
  color: inherit;
  cursor: pointer;

  &:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
    border-radius: var(--radius-sm);
  }

  &:disabled {
    opacity: 0.2;
    pointer-events: none;
  }
}

.dock-icon {
  @extend %dock-icon-button;
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  font-size: clamp(1rem, 1.35vw, 1.45rem);
  opacity: 0.85;
  transition:
    opacity var(--duration-200) var(--ease-out),
    transform var(--duration-150) var(--ease-out);

  @media (hover: hover) {
    &:hover {
      opacity: 1;
    }
  }

  &:active {
    transform: scale(0.92);
  }
}

.dock-progress {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;

  .dock-time {
    display: flex;
    justify-content: space-between;
    font-size: clamp(0.62rem, 0.76vw, 0.84rem);
    opacity: 0.8;
    font-variant-numeric: tabular-nums;
  }
}

// 走位/音量并作一行：参考图里的卡片只有信息 + 进度两段，
// 控制区必须压到一行才不会把卡片撑成参考图的三倍高。
.dock-controls {
  display: flex;
  align-items: center;
  gap: clamp(0.4rem, 0.6vw, 0.9rem);
  min-width: 0;
}

.dock-transport {
  flex: 0 0 auto;
  display: flex;
  align-items: center;

  .dock-button {
    @extend %dock-icon-button;
    width: clamp(1.45rem, 2.05vw, 2.45rem);
    height: clamp(1.45rem, 2.05vw, 2.45rem);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: clamp(0.95rem, 1.35vw, 1.6rem);
    opacity: 0.92;
    transition:
      opacity var(--duration-200) var(--ease-out),
      transform var(--duration-150) var(--ease-out);

    @media (hover: hover) {
      &:hover {
        opacity: 1;
        transform: scale(1.1);
      }
    }

    &:active {
      transform: scale(0.94);
    }

    &.skip {
      font-size: clamp(1.3rem, 1.85vw, 2.2rem);
    }

    &.active {
      opacity: 1;
    }

    &.loading {
      pointer-events: none;
    }
  }
}

// 直接挂在 BouncingSlider 根节点上，省掉一层包装 div：
// .bouncing-slider 自身已是 display: flex / align-items: center / width: 100%。
.dock-volume {
  flex: 1 1 auto;
  min-width: clamp(3.25rem, 5.5vw, 8rem);
  // BouncingSlider 的图标尺寸由变量控制（组件内是 !important），n-icon 的 size 不生效
  --bouncing-slider-icon-size: clamp(12px, 1.05vw, 19px);
  --bouncing-slider-icon-gap: clamp(5px, 0.6vw, 10px);

  :deep(.n-icon) {
    opacity: 0.75;
  }
}
</style>
