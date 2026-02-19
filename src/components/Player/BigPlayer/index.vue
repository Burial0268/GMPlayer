<template>
  <Teleport to="body">
    <div ref="bigPlayerRef"
    :class="[
      'bplayer',
      `bplayer-${setting.backgroundImageShow}`,
      isMobile ? 'mobile-player' : 'desktop-player',
      music.showBigPlayer ? 'opened' : '',
      isMobile && mobileLayer === 2 ? 'layer2-active' : ''
    ]"
    :style="{
      '--cover-bg': songPicGradient,
      '--main-cover-color': `rgb(${setting.immersivePlayer ? songPicColor : '255,255,255'})`
    }">
      <!-- 共用部分: 背景和顶部菜单 -->
      <BigPlayerBackground
        :songPicGradient="songPicGradient"
        :backgroundImageShow="setting.backgroundImageShow"
        :hasPlayData="!!music.getPlaySongData"
        :isPlaying="music.getPlayState"
        :actualPlaying="actualPlayingProp"
        :fps="setting.fps"
        :blurAmount="setting.blurAmount"
        :contrastAmount="setting.contrastAmount"
        :renderScale="setting.renderScale"
        :coverImageUrl="coverImageUrl"
        :albumImageUrl="setting.albumImageUrl"
        :flowSpeed="setting.flowSpeed"
        :lowFreqVolume="computedLowFreqVolume"
        :staticMode="!music.showBigPlayer"
      />

      <BigPlayerTopBar
        :showLyricSetting="setting.showLyricSetting"
        :screenfullIcon="screenfullIcon"
        @openSettings="LyricSettingRef.openLyricSetting()"
        @toggleFullscreen="screenfullChange"
        @close="closeBigPlayer"
      />

      <!-- 移动端布局 -->
      <MobilePlayerLayout v-if="isMobile" ref="mobileLayoutRef"
        :songName="songName"
        :artistList="artistList"
        :isNameOverflow="isNameOverflow"
        :hasLyrics="hasLyrics"
        :remainingTime="remainingTime"
        :coverImageUrl500="coverImageUrl500"
        :currentCoverStyle="currentCoverStyle"
        :coverFrameStyle="coverFrameStyle"
        :handleProgressSeek="handleProgressSeek"
        @close="closeBigPlayer"
        @switchLayer="switchMobileLayer(mobileLayer === 1 ? 2 : 1)"
        @lrcMouseEnter="lrcMouseStatus = setting.lrcMousePause ? true : false"
        @lrcAllLeave="lrcAllLeave"
        @lrcTextClick="lrcTextClick"
        @toComment="toComment"
      />

      <!-- 桌面端布局 -->
      <DesktopPlayerLayout v-else ref="desktopLayoutRef"
        :lrcMouseStatus="lrcMouseStatus"
        :menuShow="menuShow"
        :hasLyrics="hasLyrics"
        :handleProgressSeek="handleProgressSeek"
        @lrcMouseEnter="lrcMouseStatus = setting.lrcMousePause ? true : false"
        @lrcAllLeave="lrcAllLeave"
        @lrcTextClick="lrcTextClick"
      />

      <!-- 共用组件 -->
      <Spectrum v-if="setting.musicFrequency" :height="60" :show="music.showBigPlayer" />
      <LyricSetting ref="LyricSettingRef" />
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { musicStore, settingStore, siteStore } from "@/store";
import Spectrum from "../Spectrum.vue";
import LyricSetting from "@/components/DataModal/LyricSetting.vue";
import { storeToRefs } from "pinia";
import gsap from "gsap";
import {
  onMounted,
  nextTick,
  watch,
  ref,
  computed,
  onBeforeUnmount
} from "vue";
import "../icons/icon-animations.css";

// 导入 composables
import { useResponsiveLayout } from "@/composables/useResponsiveLayout";
import { usePwaThemeColor } from "@/composables/usePwaThemeColor";
import { useBigPlayerCommon } from "@/composables/useBigPlayerCommon";
import { useFullscreen } from "@/composables/useFullscreen";
import { useMobileCoverFrame } from "@/composables/useMobileCoverFrame";

// 导入子组件
import BigPlayerBackground from "./BigPlayerBackground.vue";
import BigPlayerTopBar from "./BigPlayerTopBar.vue";
import MobilePlayerLayout from "./MobilePlayerLayout.vue";
import DesktopPlayerLayout from "./DesktopPlayerLayout.vue";

const music = musicStore();
const site = siteStore();
const setting = settingStore();

const { songPicGradient, songPicColor } = storeToRefs(site);

// --- Composables ---
const { isMobile } = useResponsiveLayout();
const { changePwaColor } = usePwaThemeColor();

const {
  coverImageUrl,
  coverImageUrl500,
  artistList,
  songName,
  remainingTime,
  hasLyrics,
  computedLowFreqVolume,
  lrcMouseStatus,
  lyricsScroll,
  lrcAllLeave,
  lrcTextClick,
  closeBigPlayer,
  handleProgressSeek,
  toComment,
  isNameOverflow,
  checkNameOverflow,
} = useBigPlayerCommon(isMobile);

// --- Template refs ---
const bigPlayerRef = ref<HTMLElement | null>(null);
const mobileLayoutRef = ref<InstanceType<typeof MobilePlayerLayout> | null>(null);
const desktopLayoutRef = ref<InstanceType<typeof DesktopPlayerLayout> | null>(null);

// Proxy refs for mobile cover frame composable (read from child component)
const phonyBigCoverRef = computed(() => mobileLayoutRef.value?.phonyBigCoverRef ?? null);
const phonySmallCoverRef = computed(() => mobileLayoutRef.value?.phonySmallCoverRef ?? null);

// 全屏切换
const { screenfullIcon, screenfullChange, cleanupFullscreen } = useFullscreen(
  bigPlayerRef,
  () => {
    lrcMouseStatus.value = false;
    lyricsScroll(music.getPlaySongLyricIndex);
  }
);

// 移动端层级 & 封面帧
const mobileLayer = ref(1);

const { currentCoverStyle, coverFrameStyle, updateCoverStyle, setupResizeObserver, cleanupResizeObserver } =
  useMobileCoverFrame(bigPlayerRef, phonyBigCoverRef, phonySmallCoverRef, mobileLayer);

// --- 仅本组件需要的局部状态 ---
const forcePlaying = ref(true);
const actualPlayingProp = computed(() => forcePlaying.value || music.getPlayState);
const menuShow = ref(false);
const LyricSettingRef = ref(null);

const switchMobileLayer = (targetLayer: number) => {
  if (targetLayer === mobileLayer.value) return;
  mobileLayer.value = targetLayer;
  updateCoverStyle();
};

const initMobileElements = () => {
  if (!isMobile.value) return;
  nextTick(() => checkNameOverflow());
};

// GSAP 提示动画
const animateTip = (isVisible: boolean) => {
  const tipEl = desktopLayoutRef.value?.tipRef;
  if (!tipEl) return;
  if (isVisible) {
    gsap.fromTo(tipEl,
      { opacity: 0, y: -20 },
      { opacity: 1, y: 0, duration: 0.3, ease: "power2.out" }
    );
  } else {
    gsap.to(tipEl, {
      opacity: 0, y: -20, duration: 0.3, ease: "power2.in"
    });
  }
};

// GSAP 入场动画 (desktop)
const animatePlayerIn = () => {
  if (!bigPlayerRef.value || isMobile.value) return;
  const leftEl = desktopLayoutRef.value?.leftContentRef;
  const rightEl = desktopLayoutRef.value?.rightContentRef;
  if (leftEl) {
    gsap.fromTo(leftEl,
      { opacity: 0, scale: 0.96 },
      { opacity: 1, scale: 1, duration: 0.5, delay: 0.15, ease: "power2.out" }
    );
  }
  if (rightEl) {
    gsap.fromTo(rightEl,
      { opacity: 0, scale: 0.96 },
      { opacity: 1, scale: 1, duration: 0.5, delay: 0.25, ease: "power2.out" }
    );
  }
};

// --- Lifecycle ---
onMounted(() => {
  gsap.config({ force3D: true, nullTargetWarn: false });
  initMobileElements();

  nextTick(() => {
    setupResizeObserver();
    forcePlaying.value = false;
    lyricsScroll(music.getPlaySongLyricIndex);
  });
});

onBeforeUnmount(() => {
  cleanupFullscreen();
  cleanupResizeObserver();
});

// --- Watchers ---
watch(() => music.showBigPlayer, (val) => {
  changePwaColor();
  if (val) {
    initMobileElements();
    requestAnimationFrame(() => {
      updateCoverStyle();
      music.showPlayList = false;
      lyricsScroll(music.getPlaySongLyricIndex);
      animatePlayerIn();
    });
  }
});

watch(() => isMobile.value, () => {
  nextTick(() => {
    setupResizeObserver();
    updateCoverStyle();
    lyricsScroll(music.getPlaySongLyricIndex);
  });
});

watch(() => lrcMouseStatus.value, (val) => animateTip(val));
watch(() => music.getPlaySongLyricIndex, (val) => lyricsScroll(val));
watch(() => music.getPlaySongData, () => checkNameOverflow(), { immediate: true });

</script>

<style lang="scss" scoped>
.bplayer {
  position: fixed;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  z-index: 2000;
  overflow: hidden;
  color: var(--main-cover-color);
  background-repeat: no-repeat;
  background-position: center;
  display: flex;
  justify-content: center;
  will-change: transform;

  // AMLL-style slide-up transition (closed state)
  pointer-events: none;
  transform: translateY(100%);
  border-radius: 1em 1em 0 0;
  transition:
    border-radius 0.25s ease,
    transform 0.5s cubic-bezier(0.25, 1, 0.5, 1);

  // Opened state
  &.opened {
    pointer-events: auto;
    transform: translateY(0%);
    border-radius: 0;
    transition:
      border-radius 0.25s 0.25s ease,
      transform 0.5s cubic-bezier(0.25, 1, 0.5, 1);
  }

  /* ═══════════════════════════════════════════════════════
     移动端样式 — grid 结构 + layer2 状态切换
     ═══════════════════════════════════════════════════════ */
  &.mobile-player {
    // AMLL .verticalLayout
    display: grid;
    min-height: 0;
    min-width: 0;
    justify-content: stretch;
    grid-template-rows:
      [thumb] calc(env(safe-area-inset-top, 0px) + 30px) [main-view] 1fr;
    grid-template-columns: 1fr;

    // ═══ 状态切换 (AMLL .hideLyric 模式反转) ═══
    // 默认: Layer 1 可见 (= AMLL hideLyric)
    :deep(.mobile-small-controls) {
      opacity: 0;
      transition: opacity 0.5s;
      pointer-events: none;
    }

    :deep(.mobile-cover-layout) {
      pointer-events: auto;
    }

    :deep(.mobile-lyric),
    :deep(.no-lyrics) {
      opacity: 0;
      transition: opacity 0.5s;
      pointer-events: none;
    }

    :deep(.mobile-big-controls) {
      opacity: 1;
    }

    // Layer 2 激活 (= AMLL default)
    &.layer2-active {
      :deep(.mobile-small-controls) {
        opacity: 1;
        transition: opacity 0.25s 0.25s;
        pointer-events: auto;
      }

      :deep(.mobile-cover-layout) {
        pointer-events: none;
      }

      :deep(.mobile-lyric),
      :deep(.no-lyrics) {
        opacity: 1;
        transition: opacity 0.5s 0.5s;
        pointer-events: auto;
      }

      :deep(.mobile-big-controls) {
        opacity: 0;
        pointer-events: none;
      }
    }
  }

  &.bplayer-eplor,
  &.bplayer-blur {
    background-color: gray !important;
  }
}

/* 添加自定义动画 */
@keyframes slowRotate {
  0% {
    transform: rotate(0deg);
  }

  100% {
    transform: rotate(360deg);
  }
}

/* CSS过渡效果 */
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
