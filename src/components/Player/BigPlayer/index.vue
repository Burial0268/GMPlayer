<template>
  <Teleport to="body">
    <div
      ref="bigPlayerRef"
      :class="[
        'bplayer',
        `bplayer-${setting.backgroundImageShow}`,
        isMobile ? 'mobile-player' : 'desktop-player',
        isMobile && mobileOverlayVisible ? 'mobile-visible' : '',
        isMobile && !mobileOverlayVisible ? 'mobile-closed' : '',
        mobilePlayerOpened ? 'opened' : '',
        isMobile && mobileTransitionActive && !music.showBigPlayer ? 'mobile-transitioning' : '',
        isMobile && mobileExiting ? 'mobile-exiting' : '',
        isMobile && mobileLayer === 2 ? 'layer2-active' : '',
        isMobile && mobileQueueOpen ? 'queue-active' : '',
      ]"
      :style="{
        '--cover-bg': songPicGradient,
        '--main-cover-color': `rgb(${setting.immersivePlayer ? songPicColor : '255,255,255'})`,
        '--main-cover-mix-color': setting.immersivePlayer
          ? `color-mix(in srgb, rgb(${songPicColor}) 72%, rgb(239, 239, 239) 28%)`
          : 'rgb(239, 239, 239)',
      }"
      @mousedown="handleDesktopWindowDrag"
    >
      <!-- 移动端布局 -->
      <template v-if="isMobile">
        <Motion as-child :style="mobileShellMotionStyle">
          <div
            ref="mobileShellRef"
            :class="[
              'mobile-player-shell',
              mobileInteractive ? 'interactive-transition' : '',
              `mobile-phase-${mobileTransitionPhase}`,
            ]"
          >
            <Motion
              class="mobile-background-visual"
              :transition="backgroundLayoutTransition"
              :style="mobileBackgroundVisualStyle"
            >
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
                :staticMode="mobileBackgroundStatic"
              />
            </Motion>

            <MobilePlayerLayout
              ref="mobileLayoutRef"
              :songName="songName"
              :artistList="artistList"
              :isNameOverflow="isNameOverflow"
              :hasLyrics="hasLyrics"
              :remainingTime="remainingTime"
              :coverImageUrl500="coverImageUrl500"
              :handleProgressSeek="handleProgressSeek"
              :queueOpen="mobileQueueOpen"
              :mobileLayer="mobileLayer"
              :layoutTransition="sharedLayoutTransition"
              :contentShellStyle="mobileContentShellStyle"
              :fullUiMotionStyle="mobileFullUiMotionStyle"
              :controlsMotionStyle="mobileControlsMotionStyle"
              :albumLayerStyle="mobileArtworkMotionStyle"
              :albumLayerVisible="mobileAlbumLayerVisible"
              @close="handleMobileClose"
              @openQueue="openMobileQueue"
              @closeQueue="closeMobileQueue"
              @closeDragStart="beginMobileInteractiveClose"
              @closeDragMove="updateMobileInteractiveClose"
              @closeDragEnd="finishMobileInteractiveClose"
              @switchLayer="switchMobileLayer(mobileLayer === 1 ? 2 : 1)"
              @lrcMouseEnter="lrcMouseStatus = setting.lrcMousePause ? true : false"
              @lrcAllLeave="lrcAllLeave"
              @lrcTextClick="lrcTextClick"
              @toComment="toComment"
            />

            <Spectrum
              v-if="setting.musicFrequency && !mobileQueueOpen && mobileContentReady"
              :height="60"
              :show="music.showBigPlayer"
            />
          </div>
        </Motion>
      </template>

      <!-- 桌面端布局 -->
      <template v-else>
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
          @openSettings="LyricSettingRef.openLyricSetting()"
        />

        <DesktopPlayerLayout
          ref="desktopLayoutRef"
          :lrcMouseStatus="lrcMouseStatus"
          :menuShow="menuShow"
          :hasLyrics="hasLyrics"
          :lyricsVisible="desktopLyricsVisible"
          :queueOpen="desktopQueueOpen"
          :handleProgressSeek="handleProgressSeek"
          @lrcMouseEnter="lrcMouseStatus = setting.lrcMousePause ? true : false"
          @lrcAllLeave="lrcAllLeave"
          @lrcTextClick="lrcTextClick"
        />

        <DesktopToggleControls
          :lyricsVisible="desktopLyricsVisible && hasLyrics"
          :queueOpen="desktopQueueOpen"
          :hasLyrics="hasLyrics"
          @toggleLyrics="toggleDesktopLyrics"
          @toggleQueue="desktopQueueOpen = !desktopQueueOpen"
        />

        <Spectrum v-if="setting.musicFrequency" :height="60" :show="music.showBigPlayer" />
      </template>

      <!-- 共用组件 -->
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
import { onMounted, nextTick, watch, ref, computed, onBeforeUnmount } from "vue";
import { Motion, animate, useMotionValue, type MotionValue } from "motion-v";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "../icons/icon-animations.css";

// 导入 composables
import { useResponsiveLayout } from "@/composables/useResponsiveLayout";
import { usePwaThemeColor } from "@/composables/usePwaThemeColor";
import { useBigPlayerCommon } from "@/composables/useBigPlayerCommon";
import { useMobileCoverFrame } from "@/composables/useMobileCoverFrame";

// 导入子组件
import BigPlayerBackground from "./BigPlayerBackground.vue";
import BigPlayerTopBar from "./BigPlayerTopBar.vue";
import MobilePlayerLayout from "./MobilePlayerLayout.vue";
import DesktopPlayerLayout from "./DesktopPlayerLayout.vue";
import DesktopToggleControls from "./DesktopToggleControls.vue";

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
const mobileShellRef = ref<HTMLElement | null>(null);
const mobileLayoutRef = ref<InstanceType<typeof MobilePlayerLayout> | null>(null);
const desktopLayoutRef = ref<InstanceType<typeof DesktopPlayerLayout> | null>(null);

// Proxy refs for mobile cover frame composable (read from child component)
const phonyBigCoverRef = computed(() => mobileLayoutRef.value?.phonyBigCoverRef ?? null);
const phonySmallCoverRef = computed(() => mobileLayoutRef.value?.phonySmallCoverRef ?? null);
const mobileCoverRootRef = computed(() =>
  isMobile.value ? mobileShellRef.value : bigPlayerRef.value,
);

// 移动端层级 & 封面帧
const mobileLayer = ref(1);
const mobileQueueOpen = ref(false);
const desktopQueueOpen = ref(false);
const mobileExiting = ref(false);
const mobileTransitionActive = ref(false);
const mobileAlbumLayerReady = ref(false);
const desktopLyricsVisible = ref(true);
const mobileOverlayVisible = computed(() =>
  isMobile.value
    ? music.showBigPlayer || mobileExiting.value || mobileTransitionActive.value
    : music.showBigPlayer,
);
const mobilePlayerOpened = computed(() =>
  isMobile.value ? music.showBigPlayer || mobileExiting.value : music.showBigPlayer,
);
const mobileBackgroundStatic = computed(
  () => !music.showBigPlayer && !mobileTransitionActive.value && !mobileExiting.value,
);
const sharedLayoutTransition = {
  type: "spring",
  stiffness: 560,
  damping: 23,
  mass: 0.78,
  restDelta: 0.001,
  restSpeed: 0.02,
} as const;
// 拖拽收尾用近临界弹簧：spring 从 MotionValue 继承释放速度，顺着手的惯性收尾；
// tween 会丢速度，松手瞬间出现"顿一下再动"的断档感。
const drawerProgressTransition = {
  type: "spring",
  stiffness: 420,
  damping: 38,
  mass: 0.9,
  restDelta: 0.001,
  restSpeed: 0.02,
} as const;
const backgroundLayoutTransition = {
  type: "tween",
  duration: 0.24,
  ease: [0.22, 1, 0.36, 1],
} as const;
const albumLayoutTransition = {
  type: "spring",
  stiffness: 380,
  damping: 42,
  mass: 0.95,
  restDelta: 0.001,
  restSpeed: 0.02,
} as const;
let mobileExitFallbackTimer: number | null = null;
let mobileSkipNextStoreCloseAnimation = false;
type SharedFrame = {
  left: number;
  top: number;
  width: number;
  height: number;
  borderRadius: number;
};
type MiniSharedFrames = {
  artwork?: DOMRect;
  background?: DOMRect;
};
type StaticStyleRecord = Record<string, string | number | undefined>;
type MotionStyleRecord = Record<string, string | number | MotionValue | undefined>;

const { calcCoverLayout } = useMobileCoverFrame(
  mobileCoverRootRef,
  phonyBigCoverRef,
  phonySmallCoverRef,
);

const mobileInteractive = ref(false);
const playerProgress = useMotionValue(0);
const fullUiOpacity = useMotionValue(0);
const fullUiY = useMotionValue(20);
const controlsOpacity = useMotionValue(0);
const controlsY = useMotionValue(18);
const backgroundVisualOpacity = useMotionValue(0);
const backgroundTop = useMotionValue(0);
const backgroundRadius = useMotionValue(0);
const artworkLeft = useMotionValue(0);
const artworkTop = useMotionValue(0);
const artworkWidth = useMotionValue(0);
const artworkHeight = useMotionValue(0);
const artworkRadius = useMotionValue(12);
const artworkOpacity = useMotionValue(1);
let progressAnimation: ReturnType<typeof animate> | null = null;
let artworkFrameAnimations: ReturnType<typeof animate>[] = [];
let interactiveFrames: {
  miniArtwork: SharedFrame;
  fullArtwork: SharedFrame;
  miniBg: SharedFrame;
  fullBg: SharedFrame;
} | null = null;
let pendingInteractiveProgress = 0;
let progressUnsubscribe: (() => void) | null = null;
const mobileContentReady = ref(false);
const mobileContentVisible = ref(false);
const mobileTransitionPhase = ref<"mini" | "shared-expand" | "full-content">("mini");
const mobileTransitionDirection = ref<"opening" | "closing" | null>(null);

const mobileShellMotionStyle = computed<MotionStyleRecord>(() => ({}));
const mobileArtworkMotionStyle = computed<MotionStyleRecord>(() => ({
  left: artworkLeft,
  top: artworkTop,
  width: artworkWidth,
  height: artworkHeight,
  borderRadius: artworkRadius,
  opacity: artworkOpacity,
}));
const mobileAlbumLayerVisible = computed(
  () => isMobile.value && mobileOverlayVisible.value && mobileAlbumLayerReady.value,
);
const mobileBackgroundVisualStyle = computed<MotionStyleRecord>(() => ({
  opacity: backgroundVisualOpacity,
  "--mobile-player-bg-reveal-y": backgroundTop,
  ...(mobileInteractive.value
    ? {
        y: backgroundTop,
        borderTopLeftRadius: backgroundRadius,
        borderTopRightRadius: backgroundRadius,
        borderBottomLeftRadius: 0,
        borderBottomRightRadius: 0,
      }
    : {}),
}));
const mobileContentShellStyle = computed<StaticStyleRecord>(() => ({
  visibility: mobileContentVisible.value ? "visible" : "hidden",
}));
const mobileFullUiMotionStyle = computed<MotionStyleRecord>(() => ({
  opacity: fullUiOpacity,
  y: fullUiY,
}));
const mobileControlsMotionStyle = computed<MotionStyleRecord>(() => ({
  opacity: controlsOpacity,
  y: controlsY,
}));

// --- 仅本组件需要的局部状态 ---
const forcePlaying = ref(true);
const actualPlayingProp = computed(() => forcePlaying.value || music.getPlayState);
const menuShow = ref(false);
const LyricSettingRef = ref(null);

const clamp = (value: number, min = 0, max = 1) => Math.min(max, Math.max(min, value));
const mix = (from: number, to: number, progress: number) => from + (to - from) * progress;
const easeOutCubic = (value: number) => 1 - Math.pow(1 - clamp(value), 3);
const rangeProgress = (value: number, from: number, to: number) =>
  clamp((value - from) / (to - from));

const MOBILE_MINI_BAR_HANDOFF_END = 0.1;
const MOBILE_MINI_CHROME_FADE_END = 0.1;
const MOBILE_MINI_UI_FADE_END = 0.32;
const MOBILE_MINI_SURFACE_FADE_START = 0.0;
const MOBILE_MINI_SURFACE_FADE_END = MOBILE_MINI_BAR_HANDOFF_END;
const MOBILE_BACKGROUND_EXPAND_END = 0.94;
const MOBILE_ARTWORK_MID_END = 0.64;
const MOBILE_ARTWORK_EXPAND_END = MOBILE_BACKGROUND_EXPAND_END;
const MOBILE_CONTENT_REVEAL_START = 0.76;
const MOBILE_CONTENT_REVEAL_END = 0.98;
const MOBILE_CONTROLS_REVEAL_START = 0.88;
const MOBILE_CONTENT_INTERACTIVE_START = 0.94;
const MOBILE_DRAG_DAMPING_POWER = 1.05;

const getTransitionDistance = () => Math.min(760, Math.max(460, window.innerHeight * 0.72 || 560));
const getMiniBarMaxLift = () => Math.min(44, Math.max(28, getTransitionDistance() * 0.11));
const getMiniBarY = (progress: number) =>
  -Math.min(getMiniBarMaxLift(), Math.max(0, progress) * getTransitionDistance());
const getCurrentMiniBarY = () => {
  const value = Number.parseFloat(
    getComputedStyle(document.documentElement).getPropertyValue("--mobile-mini-player-root-y"),
  );
  return Number.isFinite(value) ? value : 0;
};
const untranslateMiniRect = (rect: DOMRect | undefined | null) => {
  if (!rect) return null;
  return new DOMRect(rect.x, rect.y - getCurrentMiniBarY(), rect.width, rect.height);
};

const readRadius = (el: Element | null, fallback: number) => {
  if (!(el instanceof HTMLElement)) return fallback;
  const value = Number.parseFloat(getComputedStyle(el).borderTopLeftRadius);
  return Number.isFinite(value) ? value : fallback;
};

const miniUiStyleCache = new Map<string, string>();

const setMiniUiVar = (name: string, value: string) => {
  if (miniUiStyleCache.get(name) === value) return;
  document.documentElement.style.setProperty(name, value);
  miniUiStyleCache.set(name, value);
};

const removeMiniUiVar = (name: string) => {
  if (!miniUiStyleCache.has(name) && !document.documentElement.style.getPropertyValue(name)) return;
  document.documentElement.style.removeProperty(name);
  miniUiStyleCache.delete(name);
};

const frameFromRect = (rect: DOMRect, rootRect: DOMRect, borderRadius: number): SharedFrame => ({
  left: rect.left - rootRect.left,
  top: rect.top - rootRect.top,
  width: rect.width,
  height: rect.height,
  borderRadius,
});

const frameFromCoverLayout = (borderRadius: number): SharedFrame | null => {
  const layout = calcCoverLayout(mobileLayer.value === 1);
  if (!layout) return null;
  return {
    left: layout.left,
    top: layout.top,
    width: layout.width,
    height: layout.height,
    borderRadius: layout.borderRadius ?? borderRadius,
  };
};

const stopArtworkFrameAnimations = () => {
  artworkFrameAnimations.forEach((animation) => animation.stop());
  artworkFrameAnimations = [];
};

const applyArtworkFrame = (frame: SharedFrame) => {
  artworkLeft.set(frame.left);
  artworkTop.set(frame.top);
  artworkWidth.set(frame.width);
  artworkHeight.set(frame.height);
  artworkRadius.set(frame.borderRadius);
  mobileAlbumLayerReady.value = true;
};

const setArtworkFrame = (frame: SharedFrame) => {
  stopArtworkFrameAnimations();
  applyArtworkFrame(frame);
};

const readCurrentArtworkFrame = (): SharedFrame => ({
  left: artworkLeft.get(),
  top: artworkTop.get(),
  width: artworkWidth.get(),
  height: artworkHeight.get(),
  borderRadius: artworkRadius.get(),
});

const animateArtworkFrameTo = (frame: SharedFrame) => {
  stopArtworkFrameAnimations();
  mobileAlbumLayerReady.value = true;
  artworkFrameAnimations = [
    animate(artworkLeft, frame.left, albumLayoutTransition),
    animate(artworkTop, frame.top, albumLayoutTransition),
    animate(artworkWidth, frame.width, albumLayoutTransition),
    animate(artworkHeight, frame.height, albumLayoutTransition),
    animate(artworkRadius, frame.borderRadius, albumLayoutTransition),
  ];
};

const syncArtworkToCurrentLayer = () => {
  const frame = frameFromCoverLayout(mobileLayer.value === 1 ? 12 : 8);
  if (!frame) return false;
  setArtworkFrame(frame);
  return true;
};

const scheduleArtworkLayerSync = () => {
  nextTick(() => {
    requestAnimationFrame(() => {
      if (mobileInteractive.value) return;
      if (music.showBigPlayer) syncArtworkToCurrentLayer();
    });
  });
};

const readMiniSharedFrames = (frames?: MiniSharedFrames) => {
  const artworkEl = document.querySelector("[data-mobile-player-artwork]");
  const backgroundEl = document.querySelector("[data-mobile-player-bg]");
  return {
    artworkRect: frames?.artwork ?? untranslateMiniRect(artworkEl?.getBoundingClientRect()),
    backgroundRect: frames?.background ?? backgroundEl?.getBoundingClientRect() ?? null,
    artworkRadius: readRadius(artworkEl, 8),
    backgroundRadius: readRadius(backgroundEl, 16),
  };
};

const seedInteractiveFromMini = (frames?: MiniSharedFrames) => {
  const mini = readMiniSharedFrames(frames);
  const shellRect = mobileShellRef.value?.getBoundingClientRect();
  const rootLeft = shellRect?.left ?? 0;
  const rootTop = shellRect?.top ?? 0;
  if (mini.backgroundRect) {
    const backgroundTopValue = mini.backgroundRect.top - rootTop;
    backgroundTop.set(backgroundTopValue);
    backgroundRadius.set(0);
  }
  if (mini.artworkRect) {
    setArtworkFrame({
      left: mini.artworkRect.left - rootLeft,
      top: mini.artworkRect.top - rootTop,
      width: mini.artworkRect.width,
      height: mini.artworkRect.height,
      borderRadius: mini.artworkRadius,
    });
  }
};

const captureInteractiveFrames = (frames?: MiniSharedFrames) => {
  const shell = mobileShellRef.value;
  if (!shell) return false;
  const shellRect = shell.getBoundingClientRect();
  const mini = readMiniSharedFrames(frames);
  const miniArtwork = mini.artworkRect
    ? frameFromRect(mini.artworkRect, shellRect, mini.artworkRadius)
    : null;
  const miniBg = mini.backgroundRect
    ? frameFromRect(mini.backgroundRect, shellRect, mini.backgroundRadius)
    : null;
  const fullArtwork = frameFromCoverLayout(12);
  if (!miniArtwork || !miniBg || !fullArtwork) return false;

  interactiveFrames = {
    miniArtwork,
    fullArtwork,
    miniBg,
    fullBg: {
      left: 0,
      top: 0,
      width: shellRect.width,
      height: shellRect.height,
      borderRadius: 0,
    },
  };
  return true;
};

const applyFrame = (
  from: SharedFrame,
  to: SharedFrame,
  progress: number,
  setters: {
    left: ReturnType<typeof useMotionValue>;
    top: ReturnType<typeof useMotionValue>;
    width: ReturnType<typeof useMotionValue>;
    height: ReturnType<typeof useMotionValue>;
    radius: ReturnType<typeof useMotionValue>;
  },
) => {
  setters.left.set(mix(from.left, to.left, progress));
  setters.top.set(mix(from.top, to.top, progress));
  setters.width.set(mix(from.width, to.width, progress));
  setters.height.set(mix(from.height, to.height, progress));
  setters.radius.set(mix(from.borderRadius, to.borderRadius, progress));
};

const interpolateFrame = (from: SharedFrame, to: SharedFrame, progress: number): SharedFrame => ({
  left: mix(from.left, to.left, progress),
  top: mix(from.top, to.top, progress),
  width: mix(from.width, to.width, progress),
  height: mix(from.height, to.height, progress),
  borderRadius: mix(from.borderRadius, to.borderRadius, progress),
});

const getLiftedMiniFrame = (
  frame: SharedFrame,
  progress = MOBILE_MINI_BAR_HANDOFF_END,
): SharedFrame => ({
  ...frame,
  top: frame.top + getMiniBarY(progress),
});

const getArtworkMidFrame = (frames: NonNullable<typeof interactiveFrames>): SharedFrame => {
  const from = getLiftedMiniFrame(frames.miniArtwork);
  const compactTarget =
    frames.fullArtwork.width <= Math.max(from.width * 1.6, frames.fullBg.width * 0.28);
  if (compactTarget) {
    const size = mix(from.width, frames.fullArtwork.width, 0.46);
    return {
      left: mix(from.left, frames.fullArtwork.left, 0.44),
      top: mix(from.top, frames.fullArtwork.top, 0.48),
      width: size,
      height: size,
      borderRadius: mix(from.borderRadius, frames.fullArtwork.borderRadius, 0.46),
    };
  }

  const size = mix(from.width, frames.fullArtwork.width, 0.38);
  const centeredLeft = (frames.fullBg.width - size) / 2;
  return {
    left: mix(from.left, centeredLeft, 0.56),
    top: mix(from.top, frames.fullArtwork.top + Math.min(42, frames.fullBg.height * 0.05), 0.46),
    width: size,
    height: size,
    borderRadius: mix(from.borderRadius, frames.fullArtwork.borderRadius, 0.34),
  };
};

const keepArtworkInsideMovingBackground = (
  frames: NonNullable<typeof interactiveFrames>,
  frame: SharedFrame,
): SharedFrame => {
  const miniArtworkInsetTop = Math.max(0, frames.miniArtwork.top - frames.miniBg.top);
  const minTop = backgroundTop.get() + miniArtworkInsetTop;
  if (frame.top >= minTop) return frame;
  return {
    ...frame,
    top: minTop,
  };
};

const applyBackgroundFrame = (frames: NonNullable<typeof interactiveFrames>, progress: number) => {
  if (progress <= MOBILE_MINI_BAR_HANDOFF_END) {
    backgroundTop.set(frames.miniBg.top + getMiniBarY(progress));
    backgroundRadius.set(frames.miniBg.borderRadius || 12);
    return;
  }

  const sheetHandoffTop = frames.miniBg.top + getMiniBarY(MOBILE_MINI_BAR_HANDOFF_END);
  const sheetHandoffRadius = frames.miniBg.borderRadius || 12;
  const expandProgress = rangeProgress(
    progress,
    MOBILE_MINI_BAR_HANDOFF_END,
    MOBILE_BACKGROUND_EXPAND_END,
  );
  backgroundTop.set(mix(sheetHandoffTop, frames.fullBg.top, expandProgress));
  backgroundRadius.set(mix(sheetHandoffRadius, frames.fullBg.borderRadius, expandProgress));
};

const applyInteractiveFrame = (progress: number) => {
  if (!interactiveFrames) return;
  applyBackgroundFrame(interactiveFrames, progress);
  if (progress <= MOBILE_MINI_BAR_HANDOFF_END) {
    applyArtworkFrame(
      keepArtworkInsideMovingBackground(
        interactiveFrames,
        getLiftedMiniFrame(interactiveFrames.miniArtwork, progress),
      ),
    );
    return;
  }

  const liftedMiniArtwork = getLiftedMiniFrame(interactiveFrames.miniArtwork);
  const midArtwork = getArtworkMidFrame(interactiveFrames);

  if (progress <= MOBILE_ARTWORK_MID_END) {
    const artworkFrame = interpolateFrame(
      liftedMiniArtwork,
      midArtwork,
      rangeProgress(progress, MOBILE_MINI_BAR_HANDOFF_END, MOBILE_ARTWORK_MID_END),
    );
    applyArtworkFrame(keepArtworkInsideMovingBackground(interactiveFrames, artworkFrame));
    return;
  }

  const artworkFrame = interpolateFrame(
    midArtwork,
    interactiveFrames.fullArtwork,
    rangeProgress(progress, MOBILE_ARTWORK_MID_END, MOBILE_ARTWORK_EXPAND_END),
  );
  applyArtworkFrame(keepArtworkInsideMovingBackground(interactiveFrames, artworkFrame));
};

const clearMiniUiVars = () => {
  removeMiniUiVar("--mobile-mini-player-root-y");
  removeMiniUiVar("--mobile-mini-player-z-index");
  removeMiniUiVar("--mobile-mini-player-pointer-events");
  removeMiniUiVar("--mobile-mini-player-surface-bg");
  removeMiniUiVar("--mobile-mini-player-surface-opacity");
  removeMiniUiVar("--mobile-mini-player-mask-y");
  removeMiniUiVar("--mobile-mini-player-surface-border");
  removeMiniUiVar("--mobile-mini-player-surface-shadow");
  removeMiniUiVar("--mobile-mini-player-ui-opacity");
  removeMiniUiVar("--mobile-mini-player-chrome-opacity");
  removeMiniUiVar("--mobile-mini-player-text-opacity");
  removeMiniUiVar("--mobile-mini-player-detail-opacity");
  removeMiniUiVar("--mobile-mini-player-detail-height");
  removeMiniUiVar("--mobile-mini-player-detail-margin");
  removeMiniUiVar("--mobile-mini-player-ui-y");
  removeMiniUiVar("--mobile-mini-player-text-y");
  removeMiniUiVar("--mobile-mini-player-artwork-opacity");
  removeMiniUiVar("--mobile-mini-player-bottom-y");
  removeMiniUiVar("--mobile-mini-player-bottom-z-index");
  removeMiniUiVar("--mobile-mini-player-bottom-pointer-events");
};

const applyMiniUiVars = (progress: number) => {
  const miniLift = rangeProgress(progress, 0, MOBILE_MINI_BAR_HANDOFF_END);
  const miniTransitioning = progress > 0.001 || mobileTransitionActive.value || music.showBigPlayer;
  const miniChromeExit = miniTransitioning ? 1 : 0;
  const miniDetailExit = miniTransitioning ? 1 : 0;
  const miniTextExit = miniTransitioning ? 1 : 0;
  const bottomExit = rangeProgress(progress, 0, MOBILE_MINI_BAR_HANDOFF_END);
  const miniSurfaceExit = rangeProgress(
    progress,
    MOBILE_MINI_SURFACE_FADE_START,
    MOBILE_MINI_SURFACE_FADE_END,
  );
  const miniArtworkOpacity = miniTransitioning ? 0 : 1;
  const miniSurfaceOpacity = miniTransitioning ? 1 - easeOutCubic(miniSurfaceExit) : 1;
  const miniMaskY = miniTransitioning ? getMiniBarY(progress) : 0;
  setMiniUiVar("--mobile-mini-player-root-y", `${getMiniBarY(progress)}px`);
  setMiniUiVar(
    "--mobile-mini-player-z-index",
    miniTransitioning && progress < MOBILE_MINI_UI_FADE_END ? "2102" : "2",
  );
  setMiniUiVar("--mobile-mini-player-pointer-events", miniTransitioning ? "none" : "auto");
  if (miniTransitioning) {
    setMiniUiVar("--mobile-mini-player-surface-border", "transparent");
    setMiniUiVar("--mobile-mini-player-surface-shadow", "none");
  } else {
    removeMiniUiVar("--mobile-mini-player-surface-bg");
    removeMiniUiVar("--mobile-mini-player-surface-border");
    removeMiniUiVar("--mobile-mini-player-surface-shadow");
  }
  setMiniUiVar("--mobile-mini-player-surface-opacity", String(miniSurfaceOpacity));
  setMiniUiVar("--mobile-mini-player-mask-y", `${miniMaskY}px`);
  setMiniUiVar("--mobile-mini-player-ui-opacity", String(1 - miniChromeExit));
  setMiniUiVar("--mobile-mini-player-chrome-opacity", String(1 - miniChromeExit));
  setMiniUiVar("--mobile-mini-player-text-opacity", String(1 - miniTextExit));
  setMiniUiVar("--mobile-mini-player-detail-opacity", String(1 - miniDetailExit));
  setMiniUiVar("--mobile-mini-player-detail-height", `${mix(1.2, 0, miniDetailExit)}em`);
  setMiniUiVar("--mobile-mini-player-detail-margin", `${mix(2, 0, miniDetailExit)}px`);
  setMiniUiVar("--mobile-mini-player-ui-y", `${mix(0, -4, miniLift)}px`);
  setMiniUiVar("--mobile-mini-player-text-y", `${mix(0, -8, miniLift)}px`);
  setMiniUiVar("--mobile-mini-player-artwork-opacity", String(miniArtworkOpacity));
  setMiniUiVar("--mobile-mini-player-bottom-y", `${mix(0, 110, bottomExit)}%`);
  setMiniUiVar(
    "--mobile-mini-player-bottom-z-index",
    miniTransitioning && progress < MOBILE_MINI_BAR_HANDOFF_END ? "2101" : "1000",
  );
  setMiniUiVar("--mobile-mini-player-bottom-pointer-events", miniTransitioning ? "none" : "auto");
};

const applyProgressState = (value: number) => {
  if (!isMobile.value) {
    clearMiniUiVars();
    backgroundVisualOpacity.set(0);
    artworkOpacity.set(1);
    mobileAlbumLayerReady.value = false;
    return;
  }
  const progress = clamp(value);
  const fullUiEnter = rangeProgress(
    progress,
    MOBILE_CONTENT_REVEAL_START,
    MOBILE_CONTENT_REVEAL_END,
  );
  const controlsEnter = rangeProgress(progress, MOBILE_CONTROLS_REVEAL_START, 1);
  const backgroundVisible = progress > 0.001 || music.showBigPlayer || mobileTransitionActive.value;
  const albumLayerOpacity = backgroundVisible ? 1 : 0;
  const contentVisible = progress >= MOBILE_CONTENT_REVEAL_START;
  const contentReady = progress >= MOBILE_CONTENT_INTERACTIVE_START;
  const nextPhase =
    progress <= 0.001
      ? "mini"
      : progress < MOBILE_CONTENT_REVEAL_START
        ? "shared-expand"
        : "full-content";
  if (mobileContentVisible.value !== contentVisible) {
    mobileContentVisible.value = contentVisible;
  }
  if (mobileContentReady.value !== contentReady) mobileContentReady.value = contentReady;
  if (mobileTransitionPhase.value !== nextPhase) {
    mobileTransitionPhase.value = nextPhase;
  }
  fullUiOpacity.set(fullUiEnter);
  fullUiY.set(mix(14, 0, easeOutCubic(fullUiEnter)));
  controlsOpacity.set(controlsEnter);
  controlsY.set(mix(14, 0, easeOutCubic(controlsEnter)));
  backgroundVisualOpacity.set(backgroundVisible ? 1 : 0);
  artworkOpacity.set(albumLayerOpacity);
  applyMiniUiVars(progress);

  if (!mobileInteractive.value || !interactiveFrames) return;
  applyInteractiveFrame(progress);
};

const dampInteractiveDragProgress = (progress: number) => {
  const nextProgress = clamp(progress);
  if (mobileTransitionDirection.value === "closing") {
    return 1 - Math.pow(1 - nextProgress, MOBILE_DRAG_DAMPING_POWER);
  }
  return Math.pow(nextProgress, MOBILE_DRAG_DAMPING_POWER);
};

const animateProgressTo = (target: number, onComplete?: () => void) => {
  progressAnimation?.stop();
  progressAnimation = animate(playerProgress, clamp(target), {
    ...drawerProgressTransition,
    onComplete: () => {
      progressAnimation = null;
      onComplete?.();
    },
  });
};

const resetClosedMobileState = () => {
  stopArtworkFrameAnimations();
  mobileLayer.value = 1;
  mobileInteractive.value = false;
  mobileTransitionActive.value = false;
  mobileTransitionDirection.value = null;
  mobileExiting.value = false;
  mobileContentReady.value = false;
  mobileContentVisible.value = false;
  mobileTransitionPhase.value = "mini";
  mobileAlbumLayerReady.value = false;
  pendingInteractiveProgress = 0;
  interactiveFrames = null;
  playerProgress.set(0);
  fullUiOpacity.set(0);
  fullUiY.set(14);
  controlsOpacity.set(0);
  controlsY.set(14);
  backgroundVisualOpacity.set(0);
  backgroundTop.set(0);
  backgroundRadius.set(0);
  artworkOpacity.set(0);
  resetMobileQueueState();
  clearMiniUiVars();
};

const switchMobileLayer = (targetLayer: number) => {
  if (targetLayer !== 1 && targetLayer !== 2) return;
  if (targetLayer === mobileLayer.value) return;
  if (mobileInteractive.value) return;
  mobileLayer.value = targetLayer;
  nextTick(() => {
    requestAnimationFrame(() => {
      const frame = frameFromCoverLayout(targetLayer === 1 ? 12 : 8);
      if (frame) animateArtworkFrameTo(frame);
    });
  });
};

const desktopDragBlockSelector = [
  ".icon-menu",
  ".player-cover-container",
  ".record",
  ".right",
  ".tip",
  ".desktop-toggle-controls",
  ".desktop-queue-panel",
  ".amll-close-action",
  ".control-thumb",
  ".controls",
  ".lrcShow",
  ".desktop-lyric-offset",
  ".bouncing-slider",
  ".n-slider",
  ".vue-slider",
  ".n-icon",
  "button",
  "a",
  "input",
  "textarea",
  "select",
  "[role='button']",
  "[role='slider']",
].join(",");

const handleDesktopWindowDrag = (event: MouseEvent) => {
  if (isMobile.value || event.button !== 0 || event.detail > 1 || !music.showBigPlayer) return;
  if (typeof window === "undefined" || !("__TAURI__" in window)) return;
  const target = event.target;
  if (!(target instanceof Element) || target.closest(desktopDragBlockSelector)) return;
  event.preventDefault();
  void getCurrentWindow()
    .startDragging()
    .catch(() => {});
};

const cleanupClosedMobileTransition = () => {
  resetClosedMobileState();
};

const completeClosedMobileTransition = (updateStore: boolean) => {
  playerProgress.set(0);
  restoreMiniSharedAlbum();
  if (updateStore) {
    mobileSkipNextStoreCloseAnimation = true;
    music.setBigPlayerState(false);
  }
  cleanupClosedMobileTransition();
};

const seedInteractiveFromFull = () => {
  const captured = captureInteractiveFrames();
  if (!captured) return false;
  if (mobileAlbumLayerReady.value && interactiveFrames) {
    interactiveFrames.fullArtwork = readCurrentArtworkFrame();
  }
  applyInteractiveFrame(1);
  return true;
};

const clearMobileExitFallback = () => {
  if (mobileExitFallbackTimer === null) return;
  window.clearTimeout(mobileExitFallbackTimer);
  mobileExitFallbackTimer = null;
};

const finishMobileExit = () => {
  clearMobileExitFallback();
  if (!music.showBigPlayer) {
    playerProgress.set(0);
    restoreMiniSharedAlbum();
    cleanupClosedMobileTransition();
    return;
  }
  mobileExiting.value = false;
};

const scheduleMobileExitFallback = () => {
  clearMobileExitFallback();
  mobileExitFallbackTimer = window.setTimeout(finishMobileExit, 700);
};

const openMobileQueue = () => {
  mobileQueueOpen.value = true;
  music.showPlayList = false;
};

const closeMobileQueue = () => {
  mobileQueueOpen.value = false;
};

const resetMobileQueueState = () => {
  mobileQueueOpen.value = false;
  music.showPlayList = false;
};

const resetDesktopQueueState = () => {
  desktopQueueOpen.value = false;
};

const toggleDesktopLyrics = () => {
  const rightEl = desktopLayoutRef.value?.rightContentRef;
  if (rightEl) {
    gsap.killTweensOf(rightEl);
    gsap.set(rightEl, { clearProps: "opacity,transform,transition" });
  }
  desktopLyricsVisible.value = !desktopLyricsVisible.value;
};

const detachMiniSharedAlbum = () => {
  window.dispatchEvent(new Event("splayer-mobile-player-detach-mini-album"));
};

const restoreMiniSharedAlbum = () => {
  window.dispatchEvent(new Event("splayer-mobile-player-restore-mini-album"));
};

const prepareMiniAlbumCloseHandoff = () => {
  restoreMiniSharedAlbum();
};

const beginMobileInteractive = async (
  frames: MiniSharedFrames | undefined,
  initialProgress: number,
) => {
  if (!isMobile.value) return;
  progressAnimation?.stop();
  progressAnimation = null;
  stopArtworkFrameAnimations();
  pendingInteractiveProgress = clamp(initialProgress);
  mobileTransitionDirection.value = pendingInteractiveProgress >= 0.999 ? "closing" : "opening";
  if (pendingInteractiveProgress <= 0.001) {
    mobileLayer.value = 1;
    mobileContentReady.value = false;
    mobileContentVisible.value = false;
    mobileTransitionPhase.value = "mini";
    mobileAlbumLayerReady.value = false;
  }
  mobileInteractive.value = true;
  mobileTransitionActive.value = true;
  mobileExiting.value = false;
  clearMobileExitFallback();
  await nextTick();
  if (pendingInteractiveProgress <= 0.001) {
    detachMiniSharedAlbum();
    seedInteractiveFromMini(frames);
  } else if (pendingInteractiveProgress >= 0.999) {
    prepareMiniAlbumCloseHandoff();
    seedInteractiveFromFull();
  }
  playerProgress.set(pendingInteractiveProgress);
  applyProgressState(pendingInteractiveProgress);

  await nextTick();
  await new Promise<void>((resolve) => {
    requestAnimationFrame(() => {
      if (
        captureInteractiveFrames(frames) &&
        pendingInteractiveProgress >= 0.999 &&
        interactiveFrames
      ) {
        interactiveFrames.fullArtwork = readCurrentArtworkFrame();
      }
      playerProgress.set(pendingInteractiveProgress);
      applyProgressState(pendingInteractiveProgress);
      resolve();
    });
  });
};

const beginMobileInteractiveOpen = (frames?: MiniSharedFrames) => {
  return beginMobileInteractive(frames, 0);
};

const beginMobileInteractiveClose = () => {
  if (mobileQueueOpen.value) return;
  beginMobileInteractive(undefined, 1);
};

const updateMobileInteractiveProgress = (progress: number) => {
  const nextProgress = clamp(progress);
  pendingInteractiveProgress = nextProgress;
  if (!mobileInteractive.value) return;
  playerProgress.set(dampInteractiveDragProgress(nextProgress));
};

const updateMobileInteractiveClose = (distance: number) => {
  updateMobileInteractiveProgress(1 - clamp(distance / getTransitionDistance()));
};

const beginStoreCloseHandoff = () => {
  if (!music.showBigPlayer) return;
  mobileExiting.value = true;
  mobileSkipNextStoreCloseAnimation = true;
  music.setBigPlayerState(false);
};

const finishMobileInteractive = (forceOpen?: boolean) =>
  new Promise<boolean>((resolve) => {
    // 甩动优先：释放速度足够时直接顺着惯性判向，不足半程也能一次关闭/打开
    const velocity = playerProgress.getVelocity();
    const shouldOpen =
      forceOpen ?? (Math.abs(velocity) > 0.6 ? velocity > 0 : pendingInteractiveProgress >= 0.5);
    mobileTransitionDirection.value = shouldOpen ? "opening" : "closing";
    if (shouldOpen) detachMiniSharedAlbum();
    else {
      restoreMiniSharedAlbum();
      beginStoreCloseHandoff();
    }
    animateProgressTo(shouldOpen ? 1 : 0, () => {
      if (!shouldOpen) {
        completeClosedMobileTransition(music.showBigPlayer);
      } else {
        if (!music.showBigPlayer) {
          music.setBigPlayerState(true);
        }
        mobileInteractive.value = false;
        mobileTransitionActive.value = false;
        mobileTransitionDirection.value = null;
        interactiveFrames = null;
        syncArtworkToCurrentLayer();
        applyProgressState(1);
      }
      resolve(shouldOpen);
    });
  });

const finishMobileInteractiveOpen = (forceOpen?: boolean) => {
  return finishMobileInteractive(forceOpen);
};

const finishMobileInteractiveClose = () => {
  return finishMobileInteractive();
};

const openMobileFromMini = async (frames?: MiniSharedFrames) => {
  if (!isMobile.value) {
    music.setBigPlayerState(true);
    return true;
  }
  resetMobileQueueState();
  await beginMobileInteractiveOpen(frames);
  return finishMobileInteractive(true);
};

const closeMobileWithProgress = async () => {
  if (!isMobile.value) {
    closeBigPlayer();
    return;
  }
  if (mobileInteractive.value) return;

  resetMobileQueueState();
  prepareMiniAlbumCloseHandoff();
  progressAnimation?.stop();
  progressAnimation = null;
  stopArtworkFrameAnimations();
  pendingInteractiveProgress = 1;
  mobileTransitionDirection.value = "closing";
  seedInteractiveFromFull();
  mobileInteractive.value = true;
  mobileTransitionActive.value = true;
  mobileExiting.value = true;
  beginStoreCloseHandoff();
  clearMobileExitFallback();
  playerProgress.set(1);
  applyProgressState(1);

  await nextTick();
  requestAnimationFrame(() => {
    if (captureInteractiveFrames() && interactiveFrames) {
      interactiveFrames.fullArtwork = readCurrentArtworkFrame();
    }
    playerProgress.set(1);
    applyProgressState(1);
    animateProgressTo(0, () => {
      completeClosedMobileTransition(music.showBigPlayer);
    });
  });
};

const handleMobileClose = () => {
  if (mobileQueueOpen.value) {
    closeMobileQueue();
    return;
  }
  closeMobileWithProgress();
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
    gsap.fromTo(
      tipEl,
      { opacity: 0, y: -20 },
      { opacity: 1, y: 0, duration: 0.3, ease: "power2.out" },
    );
  } else {
    gsap.to(tipEl, {
      opacity: 0,
      y: -20,
      duration: 0.3,
      ease: "power2.in",
    });
  }
};

// GSAP 入场动画 (desktop)
// .left/.right 自带 opacity/transform 的 CSS transition（供 noLrc / lyrics-hidden 切换用），
// 会把 GSAP 立即写入的 from 状态再做一次过渡，造成 亮->暗->亮 闪烁；
// 入场期间必须先禁用元素自身 transition，结束后 clearProps 恢复。
const animatePlayerIn = () => {
  if (!bigPlayerRef.value || isMobile.value) return;
  const leftEl = desktopLayoutRef.value?.leftContentRef;
  const rightEl = desktopLayoutRef.value?.rightContentRef;
  if (leftEl) {
    gsap.set(leftEl, { transition: "none" });
    gsap.fromTo(
      leftEl,
      { opacity: 0, scale: 0.96 },
      {
        opacity: 1,
        scale: 1,
        duration: 0.5,
        delay: 0.15,
        ease: "power2.out",
        onComplete: () => gsap.set(leftEl, { clearProps: "opacity,transform,transition" }),
      },
    );
  }
  if (rightEl) {
    gsap.set(rightEl, { transition: "none" });
    gsap.fromTo(
      rightEl,
      { opacity: 0, scale: 0.96 },
      {
        opacity: 1,
        scale: 1,
        duration: 0.5,
        delay: 0.25,
        ease: "power2.out",
        onComplete: () => gsap.set(rightEl, { clearProps: "opacity,transform,transition" }),
      },
    );
  }
};

// --- Lifecycle ---
onMounted(() => {
  gsap.config({ force3D: true, nullTargetWarn: false });
  initMobileElements();
  progressUnsubscribe = playerProgress.on("change", applyProgressState);
  if (isMobile.value) {
    applyProgressState(music.showBigPlayer ? 1 : 0);
    if (music.showBigPlayer) scheduleArtworkLayerSync();
  } else {
    clearMiniUiVars();
  }

  nextTick(() => {
    forcePlaying.value = false;
    lyricsScroll(music.getPlaySongLyricIndex);
  });
});

onBeforeUnmount(() => {
  progressAnimation?.stop();
  stopArtworkFrameAnimations();
  progressUnsubscribe?.();
  clearMiniUiVars();
  clearMobileExitFallback();
});

// --- Watchers ---
watch(
  () => music.showBigPlayer,
  (val) => {
    changePwaColor();
    if (val) {
      if (isMobile.value) {
        resetMobileQueueState();
        mobileExiting.value = false;
        clearMobileExitFallback();
        initMobileElements();
        if (!mobileInteractive.value && playerProgress.get() < 0.999) {
          void openMobileFromMini();
        } else {
          scheduleArtworkLayerSync();
          applyProgressState(1);
        }
        nextTick(() => lyricsScroll(music.getPlaySongLyricIndex));
        return;
      }
      resetDesktopQueueState();
      clearMiniUiVars();
      requestAnimationFrame(() => {
        lyricsScroll(music.getPlaySongLyricIndex);
        animatePlayerIn();
      });
    } else if (isMobile.value) {
      if (mobileSkipNextStoreCloseAnimation) {
        mobileSkipNextStoreCloseAnimation = false;
        resetMobileQueueState();
        clearMobileExitFallback();
        return;
      }
      resetMobileQueueState();
      mobileExiting.value = true;
      if (!mobileInteractive.value) animateProgressTo(0, finishMobileExit);
      scheduleMobileExitFallback();
    } else {
      resetDesktopQueueState();
    }
  },
);

watch(
  () => isMobile.value,
  (val) => {
    if (!val) {
      progressAnimation?.stop();
      progressAnimation = null;
      resetMobileQueueState();
      desktopQueueOpen.value = false;
      mobileExiting.value = false;
      mobileTransitionActive.value = false;
      mobileTransitionDirection.value = null;
      mobileAlbumLayerReady.value = false;
      stopArtworkFrameAnimations();
      clearMiniUiVars();
      clearMobileExitFallback();
    } else {
      applyProgressState(music.showBigPlayer ? 1 : 0);
      if (music.showBigPlayer) scheduleArtworkLayerSync();
    }
    nextTick(() => {
      lyricsScroll(music.getPlaySongLyricIndex);
    });
  },
);

watch(
  () => lrcMouseStatus.value,
  (val) => animateTip(val),
);
watch(
  () => music.getPlaySongLyricIndex,
  (val) => lyricsScroll(val),
);
watch(
  () => music.getPlaySongData,
  () => {
    checkNameOverflow();
    if (isMobile.value) {
      if (music.showBigPlayer) scheduleArtworkLayerSync();
    }
  },
  { immediate: true },
);

defineExpose({
  openMobileFromMini,
  beginMobileInteractiveOpen,
  updateMobileInteractiveProgress,
  finishMobileInteractiveOpen,
});
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
    z-index: 2100;
    display: block;
    min-height: 0;
    min-width: 0;
    transform: none;
    border-radius: 0;
    opacity: 1;
    visibility: visible;
    transition: none;
    background-color: transparent !important;
    pointer-events: none;

    &.mobile-visible {
      opacity: 1;
      visibility: visible;
      transform: none;
      border-radius: 0;
      transition: none;
    }

    &.mobile-closed {
      opacity: 1;
      visibility: visible;
      pointer-events: none;
    }

    &.opened {
      pointer-events: none;
    }

    &.mobile-transitioning {
      pointer-events: none;
    }

    &.mobile-exiting {
      pointer-events: none;
    }

    .mobile-player-shell {
      position: absolute;
      inset: 0;
      min-width: 0;
      min-height: 0;
      overflow: hidden;
      display: grid;
      grid-template-rows: [thumb] calc(var(--app-safe-area-top, 0px) + 30px) [main-view] 1fr;
      grid-template-columns: 1fr;
      border-radius: 0;
      background-color: transparent;
      transform-origin: bottom center;
      will-change: transform, opacity;
      isolation: isolate;

      // 不设 z-index：靠 DOM 顺序垫在 .mobile-pages 之下。若在此抬 z-index，
      // .mobile-pages 就得跟着抬，从而变成 stacking context、隔断歌词层的
      // plus-lighter 与背景画布的混合。
      .mobile-background-visual {
        position: absolute;
        left: 0;
        top: 0;
        width: 100%;
        height: 100%;
        overflow: hidden;
        background: rgb(0 0 0);
        pointer-events: none;
        will-change: opacity, border-radius;

        :deep(.big-player-background) {
          z-index: 0;
          transform: translate3d(0, calc(var(--mobile-player-bg-reveal-y, 0) * -1px), 0);
          will-change: transform, opacity;
        }
      }

      :deep(.mobile-thumb) {
        pointer-events: none;
      }
    }

    // .mobile-pages 保持无 z-index（paint 顺序已由 DOM 顺序保证在背景之上）：
    // 它是歌词 plus-lighter 混合链上的祖先，成为 stacking context 会隔断混合。

    :deep(.mobile-cover-frame) {
      pointer-events: none;
    }

    // ═══ 状态切换 (AMLL .hideLyric 模式反转) ═══
    // 默认: Layer 1 可见 (= AMLL hideLyric)
    :deep(.mobile-small-controls) {
      opacity: 0;
      transition: opacity 0.5s;
      pointer-events: none;
    }

    :deep(.mobile-cover-layout) {
      pointer-events: none;
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

    &.mobile-visible {
      :deep(.mobile-thumb),
      :deep(.mobile-cover-frame),
      :deep(.mobile-cover-layout) {
        pointer-events: auto;
      }
    }

    // Layer 2 激活 (= AMLL default)
    &.layer2-active {
      :deep(.mobile-small-controls) {
        opacity: 1;
        transition: opacity 0.25s 0.25s;
      }

      :deep(.mobile-cover-layout) {
        pointer-events: none;
      }

      :deep(.mobile-lyric),
      :deep(.no-lyrics) {
        opacity: 1;
        transition: opacity 0.5s 0.5s;
      }

      :deep(.mobile-big-controls) {
        opacity: 0;
        pointer-events: none;
      }
    }

    &.mobile-visible.layer2-active {
      :deep(.mobile-small-controls),
      :deep(.mobile-lyric),
      :deep(.no-lyrics) {
        pointer-events: auto;
      }
    }

    &.queue-active {
      :deep(.mobile-cover-frame),
      :deep(.mobile-cover-layout),
      :deep(.mobile-lyric-layout) {
        pointer-events: none;
      }
    }
  }

  // eplor/blur 的 WebGL 画布每帧清屏为透明且 alpha 随低频音量在 0.5~1 间浮动，
  // 页面底色会透过画布混合：灰色会抬亮暗部并把着色器抖动压到 1 LSB 以下，
  // 产生色带条纹；必须保持纯黑（与 AMLL 参考实现一致）。
  &.bplayer-eplor,
  &.bplayer-blur {
    background-color: #000 !important;
  }

  &.mobile-player.bplayer-eplor,
  &.mobile-player.bplayer-blur {
    background-color: transparent !important;
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
