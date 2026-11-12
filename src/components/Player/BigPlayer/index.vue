<template>
  <Teleport to="body">
    <div
      ref="bigPlayerRef"
      data-navigation-layer="player"
      role="dialog"
      :aria-modal="music.showBigPlayer || undefined"
      :aria-label="$t('navigation.player')"
      :inert="!navigation.overlays.value.some((layer) => layer.kind === 'player')"
      :class="[
        'bplayer',
        { 'navigation-suspended': playerSuspended, 'navigation-resuming': resumingPlayer },
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
        <Motion class="mobile-scrim" :style="mobileScrimStyle" />
        <Motion as-child :style="mobileShellMotionStyle">
          <div
            ref="mobileShellRef"
            :class="[
              'mobile-player-shell',
              mobileInteractive ? 'interactive-transition' : '',
              `mobile-phase-${mobileTransitionPhase}`,
            ]"
          >
            <Motion class="mobile-background-visual" :style="mobileBackgroundVisualStyle">
              <BigPlayerBackground
                ref="background"
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
                :staticMode="mobileBackgroundStatic"
              />
            </Motion>

            <MobilePlayerLayout
              ref="mobileLayoutRef"
              :class="['share-fade', { 'share-hidden': shareOpen }]"
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
              :headerMotionStyle="mobileHeaderMotionStyle"
              :handleMotionStyle="mobileHandleMotionStyle"
              :titleMotionStyle="mobileTitleMotionStyle"
              :titleRealStyle="mobileTitleRealStyle"
              :textTravelling="mobileInteractive"
              :titleMetrics="mobileTitleMetrics"
              :artistsMotionStyle="mobileArtistsMotionStyle"
              :artistsMetrics="mobileArtistsMetrics"
              :albumLayerStyle="mobileArtworkMotionStyle"
              :albumLayerVisible="mobileAlbumLayerVisible"
              :tinted="setting.immersivePlayer"
              @close="handleMobileClose"
              @openQueue="openMobileQueue"
              @closeQueue="closeMobileQueue"
              @closeDragStart="beginMobileInteractiveClose"
              @closeDragMove="updateMobileInteractiveClose"
              @closeDragEnd="finishMobileInteractiveClose"
              @closeDragCancel="cancelMobileInteractiveClose"
              @switchLayer="switchMobileLayer(mobileLayer === 1 ? 2 : 1)"
              @lrcMouseEnter="lrcMouseStatus = setting.lrcMousePause ? true : false"
              @lrcAllLeave="lrcAllLeave"
              @lrcTextClick="lrcTextClick"
              @toComment="toComment(retractMobilePlayer)"
              @openPage="openPageFromMobilePlayer"
            />

            <Spectrum
              v-if="setting.musicFrequency && !mobileQueueOpen && !shareOpen && mobileContentReady"
              :height="60"
              :show="music.showBigPlayer"
            />
          </div>
        </Motion>
      </template>

      <!-- 桌面端布局 -->
      <template v-else>
        <BigPlayerBackground
          ref="background"
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
          :staticMode="!music.showBigPlayer"
        />

        <BigPlayerTopBar
          :class="['share-fade', { 'share-hidden': shareOpen }]"
          :showLyricSetting="setting.showLyricSetting"
          @openSettings="openLyricSetting()"
        />

        <DesktopPlayerLayout
          ref="desktopLayoutRef"
          :class="['share-fade', { 'share-hidden': shareOpen }]"
          :lrcMouseStatus="lrcMouseStatus"
          :menuShow="menuShow"
          :hasLyrics="hasLyrics"
          :lyricsVisible="desktopLyricsVisible"
          :queueOpen="desktopQueueOpen"
          :commentsOpen="desktopCommentsOpen"
          :handleProgressSeek="handleProgressSeek"
          @lrcMouseEnter="lrcMouseStatus = setting.lrcMousePause ? true : false"
          @lrcAllLeave="lrcAllLeave"
          @lrcTextClick="lrcTextClick"
          @openComments="openDesktopComments"
          @closeComments="desktopCommentsOpen = false"
        />

        <DesktopToggleControls
          :class="['share-fade', { 'share-hidden': shareOpen }]"
          :lyricsVisible="desktopLyricsVisible && hasLyrics"
          :queueOpen="desktopQueueOpen"
          :hasLyrics="hasLyrics"
          @toggleLyrics="toggleDesktopLyrics"
          @toggleQueue="toggleDesktopQueue"
        />

        <Spectrum
          v-if="setting.musicFrequency && !shareOpen"
          :height="60"
          :show="music.showBigPlayer"
        />
      </template>

      <!-- 共用组件 -->
      <LyricShare :captureStill="captureBackgroundStill" />
      <!-- 首次点开设置才拉取：它经 SettingsWorkspace 拖进整个 n-color-picker 分包，
           而它只是个模态框，不参与大播放器的开合动画。 -->
      <component :is="LyricSettingComponent" v-if="LyricSettingComponent" ref="LyricSettingRef" />
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import type { RouteLocationRaw } from "vue-router";
import { musicStore, settingStore, siteStore } from "@/store";
import Spectrum from "../Spectrum.vue";
import { storeToRefs } from "pinia";
import gsap from "gsap";
import {
  onMounted,
  nextTick,
  watch,
  ref,
  computed,
  onBeforeUnmount,
  shallowRef,
  useTemplateRef,
} from "vue";
import { Motion, animate, useMotionValue, useTransform, type MotionValue } from "motion-v";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "../icons/icon-animations.css";

// 导入 composables
import { useResponsiveLayout } from "@/composables/useResponsiveLayout";
import { usePwaThemeColor } from "@/composables/usePwaThemeColor";
import { useBigPlayerCommon } from "@/composables/useBigPlayerCommon";
import { useMobileCoverFrame } from "@/composables/useMobileCoverFrame";
import { prefersReducedMotion } from "@/utils/reducedMotion";
import { useMotionInterruption } from "@/composables/useMotionInterruption";
import { useLayerNavigation } from "@/utils/navigation";
import { layerMotion } from "@/utils/navigation/motion";

// 导入子组件
import BigPlayerBackground from "./BigPlayerBackground.vue";
import BigPlayerTopBar from "./BigPlayerTopBar.vue";
import MobilePlayerLayout from "./MobilePlayerLayout.vue";
import DesktopPlayerLayout from "./DesktopPlayerLayout.vue";
import DesktopToggleControls from "./DesktopToggleControls.vue";
import LyricShare from "../LyricShare.vue";

const music = musicStore();
const navigation = useLayerNavigation();
const playerSuspended = computed(
  () => navigation.hasLayer("player") && !navigation.playerVisible.value,
);
const resumingPlayer = computed(
  () =>
    navigation.isTransitioning.value &&
    navigation.transition.value.direction === "pop" &&
    navigation.transition.value.from.some((layer) => layer.kind === "player"),
);
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
// Measured against the player page, not the shell. The queue pager no longer
// translates that page (a transform there isolates plus-lighter for the whole
// slide); the cover applies the shift itself, and the measurement strips it.
const mobileCoverRootRef = computed(() =>
  isMobile.value
    ? (phonyBigCoverRef.value?.closest<HTMLElement>(".mobile-player-content") ??
      mobileShellRef.value)
    : bigPlayerRef.value,
);

// 移动端层级 & 封面帧
const mobileLayer = ref(1);
const mobileQueueOpen = computed(() => isMobile.value && navigation.playerQueueVisible.value);
// The lyric share layer is a page of the player: everything but the background
// steps aside for it.
const shareOpen = navigation.playerShareVisible;
const background = useTemplateRef<InstanceType<typeof BigPlayerBackground>>("background");
const captureBackgroundStill = async () => background.value?.captureStill();
const desktopQueueOpen = computed(() => !isMobile.value && navigation.playerQueueVisible.value);
const desktopCommentsOpen = ref(false);
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
type TextKind = "name" | "artists";
// What the float is laid out at: the larger of the two layers' sizes, fixed for as long as the
// float lives. The glyph size on screen is a frame value (`TextFrame.size`) and is drawn as a
// scale of this, so nothing but the frame ever changes, in a gesture or in a layer switch.
type TextMetrics = { size: number; height: number };
// A text's frame, like the artwork's: its top-left corner, its glyph size and how wide a window it
// may run in before it is cut off (all in screen pixels).
type TextFrame = { left: number; top: number; size: number; width: number };
type Rgba = [number, number, number, number];
type TextFrames = {
  mini: TextFrame;
  full: TextFrame;
  // The text's colour in the bar and in the active layer; null when either could not be read.
  color: { from: Rgba; to: Rgba } | null;
  metrics: TextMetrics;
};
type StaticStyleRecord = Record<string, string | number | undefined>;
type MotionStyleRecord = Record<string, string | number | MotionValue | undefined>;

const { calcCoverLayout } = useMobileCoverFrame(
  mobileCoverRootRef,
  phonyBigCoverRef,
  phonySmallCoverRef,
);

const mobileInteractive = ref(false);
// The one source of truth, like the queue sheet's. The sheet's offset, fade and corners, the scrim
// and the reveal windows are all derived from it below, so nothing is pushed or reset per frame and
// an interrupted gesture cannot leave a stale offset behind.
const playerProgress = useMotionValue(0);
// Layer 2's title row sits beside the small cover, so it rides the cover's travel inside the sheet.
const headerY = useMotionValue(0);
// How far below its open position the closed sheet sits: the mini bar's top edge. Measured per
// gesture; it only scales (1 - progress), so it is irrelevant once the player is open.
const sheetTravel = useMotionValue(0);
const sheetMiniRadius = useMotionValue(12);
// The song name, and the artists line when the bar shows artists rather than a lyric, are shared
// with the mini bar like the artwork: one floating element each, swapped in for the bar's in a
// single frame, travelling (and changing colour) to the active layer's slot and staying there as
// the visible text. The real ones are measuring slots only.
const createSharedText = () => {
  const size = useMotionValue(0);
  const span = useMotionValue(0);
  const base = useMotionValue(0);
  return {
    left: useMotionValue(0),
    top: useMotionValue(0),
    size,
    // The window the text is cut off in, on screen. It travels from the bar's width to the slot's,
    // so no frame shows more or less text than its neighbours.
    span,
    base,
    // What the float is drawn with: its glyphs are `base` px, scaled to `size`, and its own width
    // is the window before that scale.
    scale: useTransform(() => (base.get() ? size.get() / base.get() : 1)),
    width: useTransform(() => (size.get() ? (span.get() * base.get()) / size.get() : span.get())),
    color: useMotionValue(""),
    // 1 while the text has a float. A MotionValue because derived values read it.
    morph: useMotionValue(0),
    metrics: ref<TextMetrics | null>(null),
  };
};
type SharedText = ReturnType<typeof createSharedText>;
const readTextFrame = (text: SharedText): TextFrame => ({
  left: text.left.get(),
  top: text.top.get(),
  size: text.size.get(),
  width: text.span.get(),
});
const setTextFrame = (text: SharedText, frame: TextFrame) => {
  text.left.set(frame.left);
  text.top.set(frame.top);
  text.size.set(frame.size);
  text.span.set(frame.width);
};
const sharedTexts = { name: createSharedText(), artists: createSharedText() };
const textKinds = Object.keys(sharedTexts) as TextKind[];
const mobileTitleMetrics = sharedTexts.name.metrics;
const mobileArtistsMetrics = sharedTexts.artists.metrics;
const artworkLeft = useMotionValue(0);
const artworkTop = useMotionValue(0);
const artworkWidth = useMotionValue(0);
const artworkHeight = useMotionValue(0);
const artworkRadius = useMotionValue(12);
const artworkOpacity = useMotionValue(1);
let progressAnimation: ReturnType<typeof animate> | null = null;
let interactiveGeneration = 0;
let resolveInteractive: ((open: boolean) => void) | undefined;
let artworkFrameAnimations: ReturnType<typeof animate>[] = [];
let interactiveFrames: {
  miniArtwork: SharedFrame;
  fullArtwork: SharedFrame;
  texts: Record<TextKind, TextFrames | null>;
} | null = null;
// Where the texts were when a close began, if a layer switch had not finished: the gesture's
// "open" end for them, like `fullArtwork` is for the artwork. Kept for the gesture's re-measures.
let restTextFrames: Partial<Record<TextKind, TextFrame>> | null = null;
let pendingInteractiveProgress = 0;
let closeDragOrigin = 1;
let progressUnsubscribe: (() => void) | null = null;
const mobileContentReady = ref(false);
const mobileContentVisible = ref(false);
const mobileTransitionPhase = ref<"mini" | "shared-expand" | "full-content">("mini");
const mobileTransitionDirection = ref<"opening" | "closing" | null>(null);

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
// `inherit`, not `visible`: an explicit `visible` on these wrappers beats the
// `visibility: hidden` that `.navigation-suspended` puts on the root, so the song
// info and controls stayed painted over the page (comments, …) opened on top.
const mobileContentShellStyle = computed<StaticStyleRecord>(() => ({
  visibility: mobileContentVisible.value ? "inherit" : "hidden",
}));

// --- 仅本组件需要的局部状态 ---
const forcePlaying = ref(true);
const actualPlayingProp = computed(() => forcePlaying.value || music.getPlayState);
const menuShow = ref(false);
const LyricSettingRef = ref(null);
// 显式 import() 而不是 defineAsyncComponent：这里要靠 ref 调命令式的
// openLyricSetting()，需要一个「已解析且已挂载」的确定时机，
// 而 defineAsyncComponent 解析完成的时刻是拿不到的。
const LyricSettingComponent = shallowRef<any>(null);
const openLyricSetting = async () => {
  if (!LyricSettingComponent.value) {
    LyricSettingComponent.value = (await import("@/components/DataModal/LyricSetting.vue")).default;
    await nextTick();
  }
  (LyricSettingRef.value as any)?.openLyricSetting();
};

const clamp = (value: number, min = 0, max = 1) => Math.min(max, Math.max(min, value));
const mix = (from: number, to: number, progress: number) => from + (to - from) * progress;
const easeOutCubic = (value: number) => 1 - Math.pow(1 - clamp(value), 3);
const rangeProgress = (value: number, from: number, to: number) =>
  clamp((value - from) / (to - from));

// One spring drives everything. The sheet (offset, corners, fade) and the artwork ride the whole
// span and the content is carried by the sheet; only a few things get a window, and each window
// opens while something is already moving.
// The mini bar rides the sheet's edge while it fades, so it is gone by the time the tab bar is.
const MOBILE_MINI_CHROME_FADE_END = 0.3;
const MOBILE_SHEET_FADE_END = 0.12;
const MOBILE_TABBAR_EXIT_END = 0.3;
const MOBILE_SHEET_PEAK_RADIUS = 28;
const MOBILE_SHEET_SETTLE_START = 0.7;
// The mini bar stays above the sheet until its own fade is done.
const MOBILE_MINI_UI_FADE_END = 0.3;
// What can be on screen while the sheet is still low — the handle on its top edge, and in layer 2
// the compact title row and the lyrics — sits exactly where the mini bar is carried, so it waits
// for the bar to be nearly gone (MOBILE_MINI_CHROME_FADE_END) and two sets of text never share a
// spot; the handle goes first. Layer 1's info and controls start below the screen edge and need
// no window.
const MOBILE_HANDLE_REVEAL_START = 0.25;
const MOBILE_HANDLE_REVEAL_END = 0.45;
const MOBILE_CONTENT_REVEAL_START = 0.3;
const MOBILE_CONTENT_REVEAL_END = 0.55;
const MOBILE_CONTENT_INTERACTIVE_START = 0.94;
const MOBILE_DRAG_DAMPING_POWER = 1.05;
const MOBILE_SCRIM_OPACITY = 0.75;

// The sheet is the whole shell (background + content), one rigid layer like the queue drawer's:
// only its own transform, fade and corners move per frame, nothing inside it is touched.
const sheetOffset = (progress: number) => (1 - clamp(progress)) * sheetTravel.get();
const sheetY = useTransform(() => sheetOffset(playerProgress.get()));
// Cross-fades with the glass mini bar it rises from. Only the background: the artwork replaces the
// bar's own from the first pixel, so fading the whole sheet would make it blink.
const sheetOpacity = useTransform(() =>
  easeOutCubic(rangeProgress(playerProgress.get(), 0, MOBILE_SHEET_FADE_END)),
);
// Corners swell like a device's while the sheet rises, then relax to square on arrival.
const sheetRadius = useTransform(() => {
  const progress = clamp(playerProgress.get());
  const swelling = mix(
    sheetMiniRadius.get(),
    MOBILE_SHEET_PEAK_RADIUS,
    easeOutCubic(rangeProgress(progress, 0, 0.3)),
  );
  return mix(swelling, 0, rangeProgress(progress, MOBILE_SHEET_SETTLE_START, 1));
});
// Dims the page behind the sheet in step with it, like a drawer's backdrop.
const scrimOpacity = useTransform(() => MOBILE_SCRIM_OPACITY * clamp(playerProgress.get()));
const handleOpacity = useTransform(() =>
  rangeProgress(playerProgress.get(), MOBILE_HANDLE_REVEAL_START, MOBILE_HANDLE_REVEAL_END),
);
const contentOpacity = useTransform(() =>
  rangeProgress(playerProgress.get(), MOBILE_CONTENT_REVEAL_START, MOBILE_CONTENT_REVEAL_END),
);

// Like the album layer, the floating name is the only visible one for as long as it exists; the
// real name is a measuring slot and never shows. There is no hand-over at the open end to time.
// Without a float (nothing to measure) the real one is simply shown.
const titleRealOpacity = useTransform(() => (sharedTexts.name.morph.get() ? 0 : 1));

const mobileShellMotionStyle: MotionStyleRecord = {
  y: sheetY,
  borderTopLeftRadius: sheetRadius,
  borderTopRightRadius: sheetRadius,
};
const mobileBackgroundVisualStyle: MotionStyleRecord = { opacity: sheetOpacity };
const mobileScrimStyle: MotionStyleRecord = { opacity: scrimOpacity };
const mobileFullUiMotionStyle: MotionStyleRecord = { opacity: contentOpacity };
const mobileHeaderMotionStyle: MotionStyleRecord = { opacity: contentOpacity, y: headerY };
const mobileHandleMotionStyle: MotionStyleRecord = { opacity: handleOpacity };
const textMotionStyle = (text: SharedText): MotionStyleRecord => ({
  x: text.left,
  y: text.top,
  scale: text.scale,
  width: text.width,
  color: text.color,
});
const mobileTitleMotionStyle = textMotionStyle(sharedTexts.name);
const mobileArtistsMotionStyle = textMotionStyle(sharedTexts.artists);
const mobileTitleRealStyle: MotionStyleRecord = { opacity: titleRealOpacity };

// The sheet follows the finger one to one, like the queue drawer; the fallback only covers a bar
// that could not be measured.
const getTransitionDistance = () =>
  sheetTravel.get() || Math.min(760, Math.max(460, window.innerHeight * 0.72 || 560));

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
  if (prefersReducedMotion()) {
    applyArtworkFrame(frame);
    return;
  }
  mobileAlbumLayerReady.value = true;
  artworkFrameAnimations = [
    animate(artworkLeft, frame.left, albumLayoutTransition),
    animate(artworkTop, frame.top, albumLayoutTransition),
    animate(artworkWidth, frame.width, albumLayoutTransition),
    animate(artworkHeight, frame.height, albumLayoutTransition),
    animate(artworkRadius, frame.borderRadius, albumLayoutTransition),
  ];
};

// Where a text rests: the active layer's slot, at its own size, in the page's own colour. The
// artists line only follows a float a gesture started: it appears when the bar shows artists, not
// on its own.
const syncTextToCurrentLayer = (kind: TextKind) => {
  const text = sharedTexts[kind];
  if (kind === "artists" && !text.metrics.value) return;
  const contentRect = mobileCoverRootRef.value?.getBoundingClientRect();
  const slot = contentRect && measureTextSlot(contentRect, kind);
  if (!slot) return setTextMorph(kind, null);
  setTextMorph(kind, slot.metrics);
  setTextFrame(text, slot.frame);
  text.color.set("");
};

// Layer switch: the float's frame goes to the new slot's together with the artwork's, from
// wherever it is. Like the artwork, nothing else is touched, so any number of interruptions is just
// a new target.
const animateTextToCurrentLayer = (kind: TextKind) => {
  const text = sharedTexts[kind];
  const previous = text.metrics.value;
  const contentRect = mobileCoverRootRef.value?.getBoundingClientRect();
  const slot = contentRect && measureTextSlot(contentRect, kind);
  if (!previous || !slot || prefersReducedMotion()) return syncTextToCurrentLayer(kind);
  text.color.set("");
  setTextMorph(kind, slot.metrics);
  artworkFrameAnimations.push(
    animate(text.left, slot.frame.left, albumLayoutTransition),
    animate(text.top, slot.frame.top, albumLayoutTransition),
    animate(text.size, slot.frame.size, albumLayoutTransition),
    animate(text.span, slot.frame.width, albumLayoutTransition),
  );
};

const syncArtworkToCurrentLayer = () => {
  textKinds.forEach(syncTextToCurrentLayer);
  const frame = frameFromCoverLayout(mobileLayer.value === 1 ? 12 : 8);
  if (!frame) return false;
  setArtworkFrame(frame);
  return true;
};

// What a gesture left short of its slot (a layer switch it cut off, then cancelled) glides the
// rest of the way instead of snapping; already there, it changes nothing.
const glideToCurrentLayer = () => {
  const frame = frameFromCoverLayout(mobileLayer.value === 1 ? 12 : 8);
  if (!frame) return syncArtworkToCurrentLayer();
  animateArtworkFrameTo(frame);
  textKinds.forEach(animateTextToCurrentLayer);
};

const scheduleArtworkLayerSync = () => {
  nextTick(() => {
    requestAnimationFrame(() => {
      if (mobileInteractive.value) return;
      if (music.showBigPlayer) syncArtworkToCurrentLayer();
    });
  });
};

// Where the bar sits at rest: a gesture that began a frame ago may already have lifted it.
const readRestRect = (el: Element | null) => {
  const rect = el?.getBoundingClientRect();
  return rect && miniBarRise
    ? new DOMRect(rect.x, rect.y - miniBarRise, rect.width, rect.height)
    : rect;
};

const readMiniSharedFrames = (frames?: MiniSharedFrames) => {
  const artworkEl = document.querySelector("[data-mobile-player-artwork]");
  const backgroundEl = document.querySelector("[data-mobile-player-bg]");
  return {
    artworkRect: frames?.artwork ?? readRestRect(artworkEl),
    backgroundRect: frames?.background ?? readRestRect(backgroundEl) ?? null,
    artworkRadius: readRadius(artworkEl, 8),
    backgroundRadius: readRadius(backgroundEl, 16),
  };
};

// Frames are measured against the player root, not the shell: the shell is the sheet and moves.
// Synchronous and first in a gesture, because the finger-to-sheet ratio depends on the travel.
const measureSheetTravel = (frames?: MiniSharedFrames) => {
  const mini = readMiniSharedFrames(frames);
  if (!mini.backgroundRect) return;
  sheetTravel.set(mini.backgroundRect.top - (bigPlayerRef.value?.getBoundingClientRect().top ?? 0));
  sheetMiniRadius.set(mini.backgroundRadius || 12);
};

const seedInteractiveFromMini = (frames?: MiniSharedFrames) => {
  const mini = readMiniSharedFrames(frames);
  const rootRect = bigPlayerRef.value?.getBoundingClientRect();
  const rootLeft = rootRect?.left ?? 0;
  const rootTop = rootRect?.top ?? 0;
  if (mini.artworkRect) {
    // The artwork sits inside the sheet, which starts out `sheetTravel` down.
    setArtworkFrame({
      left: mini.artworkRect.left - rootLeft,
      top: mini.artworkRect.top - rootTop - sheetTravel.get(),
      width: mini.artworkRect.width,
      height: mini.artworkRect.height,
      borderRadius: mini.artworkRadius,
    });
  }
};

// color-mix() computes to color(srgb …); shared text also reads the mini bar's rgb()/rgba().
const parseColor = (css: string): Rgba | null => {
  const srgb = css.match(/color\(srgb\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)(?:\s*\/\s*([\d.]+))?\s*\)/);
  if (srgb) {
    return [
      Number(srgb[1]) * 255,
      Number(srgb[2]) * 255,
      Number(srgb[3]) * 255,
      srgb[4] === undefined ? 1 : Number(srgb[4]),
    ];
  }
  const match = css.match(
    /rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:\s*[,/]\s*([\d.]+)(%?))?\s*\)/,
  );
  if (!match) return null;
  const alpha = match[4] === undefined ? 1 : Number(match[4]) / (match[5] ? 100 : 1);
  return [Number(match[1]), Number(match[2]), Number(match[3]), alpha];
};

const textSelectors: Record<TextKind, { real: string; mini: string }> = {
  name: { real: ".name", mini: "[data-mobile-player-title]" },
  artists: { real: ".artists", mini: "[data-mobile-player-artists]" },
};

// A rect carries the translation of every ancestor up to the content root: layer 2's header rides
// the cover during a gesture (`headerY`), and the controls sit low while hidden and rise as they
// come back. A slot is where the text is in the page without those, which is what the artwork gets
// from its static placeholders; measured with them, a text lands as far off as they were at the
// moment of measuring. Computed styles and rects are read from the same state, so they agree.
const ancestorShift = (el: HTMLElement, root: HTMLElement | null) => {
  let x = 0;
  let y = 0;
  for (let node = el.parentElement; node && node !== root; node = node.parentElement) {
    const { transform } = getComputedStyle(node);
    if (transform === "none") continue;
    const matrix = new DOMMatrixReadOnly(transform);
    x += matrix.m41;
    y += matrix.m42;
  }
  return { x, y };
};

// What the float is laid out at: the larger of the two layers' texts. Both layers' slots are in
// the page at all times, so it does not depend on which one is active.
const measureTextMetrics = (kind: TextKind): TextMetrics | null => {
  const layout = mobileLayoutRef.value;
  let best: TextMetrics | null = null;
  for (const info of [layout?.bigSongInfoRef, layout?.smallSongInfoRef]) {
    const el = info?.querySelector<HTMLElement>(textSelectors[kind].real);
    const size = el ? Number.parseFloat(getComputedStyle(el).fontSize) : 0;
    if (el && size > (best?.size ?? 0)) best = { size, height: el.getBoundingClientRect().height };
  }
  return best;
};

// The active layer's slot for a text: the frame its float has to take, in what colour. The slot's
// own box (no transform on it, unlike its wrapper's motion style), and its colour with the
// element's own opacity folded in, since the float has none.
const measureTextSlot = (contentRect: DOMRect, kind: TextKind) => {
  const layout = mobileLayoutRef.value;
  const info =
    (mobileLayer.value === 1 ? layout?.bigSongInfoRef : layout?.smallSongInfoRef) ?? null;
  const el = info?.querySelector<HTMLElement>(textSelectors[kind].real);
  if (!el) return null;
  const style = getComputedStyle(el);
  const size = Number.parseFloat(style.fontSize);
  if (!(size > 0)) return null;
  const metrics = measureTextMetrics(kind);
  if (!metrics) return null;
  const rect = el.getBoundingClientRect();
  const shift = ancestorShift(el, mobileCoverRootRef.value);
  const color = parseColor(style.color);
  if (color) color[3] *= Number(style.opacity);
  return {
    frame: {
      left: rect.left - contentRect.left - shift.x,
      top: rect.top - contentRect.top - shift.y,
      size,
      width: rect.width,
    } satisfies TextFrame,
    color,
    metrics,
  };
};

// The bar's text (top-left corner, font size, colour) against the active layer's slot, as the
// text's shared frames. Null when the bar has no such text (a lyric instead of the artists).
const measureText = (kind: TextKind, rootRect: DOMRect): TextFrames | null => {
  const miniEl = document.querySelector<HTMLElement>(textSelectors[kind].mini);
  const miniRect = readRestRect(miniEl);
  const contentRect = mobileCoverRootRef.value?.getBoundingClientRect();
  const slot = contentRect && measureTextSlot(contentRect, kind);
  if (!miniEl || !miniRect || !slot) return null;

  // The artists are n-text spans inside the bar's row; their colour is on the span.
  const miniStyle = getComputedStyle(miniEl.querySelector<HTMLElement>(".name") ?? miniEl);
  const miniSize = Number.parseFloat(getComputedStyle(miniEl).fontSize);
  if (!(miniSize > 0)) return null;
  const from = parseColor(miniStyle.color);
  // The bar's text column rides up a few pixels as the bar sheds its chrome (progress 1 has it
  // fully shifted); where the text sits at rest is without that.
  const exitY = Number.parseFloat(
    document.documentElement.style.getPropertyValue("--mobile-mini-player-text-y"),
  );
  return {
    mini: {
      left: miniRect.left - rootRect.left,
      top: miniRect.top - rootRect.top - (exitY || 0),
      size: miniSize,
      width: miniRect.width,
    },
    full: slot.frame,
    color: from && slot.color ? { from, to: slot.color } : null,
    metrics: slot.metrics,
  };
};

// The ref changes the layout's props, so only touch it when the numbers really differ.
const setTextMorph = (kind: TextKind, metrics: TextMetrics | null) => {
  const text = sharedTexts[kind];
  text.morph.set(metrics ? 1 : 0);
  text.base.set(metrics?.size ?? 0);
  const current = text.metrics.value;
  const same =
    metrics === current ||
    (metrics &&
      current &&
      (Object.keys(metrics) as (keyof TextMetrics)[]).every(
        (key) => metrics[key] === current[key],
      ));
  if (!same) text.metrics.value = metrics;
};

// The texts are not part of this: their floats stay for as long as the player is open, like the
// album layer. Only closing (or a slot that cannot be measured) takes them away.
const releaseInteractiveFrames = () => {
  interactiveFrames = null;
  restTextFrames = null;
};

const captureInteractiveFrames = (frames?: MiniSharedFrames) => {
  const root = bigPlayerRef.value;
  if (!root) return false;
  const rootRect = root.getBoundingClientRect();
  const mini = readMiniSharedFrames(frames);
  const fullArtwork = frameFromCoverLayout(12);
  if (!mini.artworkRect || !mini.backgroundRect || !fullArtwork) return false;

  const texts = {
    name: measureText("name", rootRect),
    artists: measureText("artists", rootRect),
  };
  textKinds.forEach((kind) => {
    const rest = restTextFrames?.[kind];
    const frames = texts[kind];
    if (rest && frames) frames.full = rest;
  });
  interactiveFrames = {
    miniArtwork: frameFromRect(mini.artworkRect, rootRect, mini.artworkRadius),
    fullArtwork,
    texts,
  };
  textKinds.forEach((kind) => setTextMorph(kind, texts[kind]?.metrics ?? null));
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

// Sheet and artwork ride the same 0→1 span, so they leave the bar together and land
// together. Nothing here is piecewise: the old lift → mid-frame → full chain had a velocity
// kink at each seam and a clamp to keep the artwork inside a sheet that moved differently.
// Both start with the artwork inset inside the sheet and end with it well below the top
// edge, so a straight blend keeps it inside without help.
const applyInteractiveFrame = (progress: number) => {
  if (!interactiveFrames) return;
  const { miniArtwork, fullArtwork, texts } = interactiveFrames;
  // The blend is the artwork's path on screen; it lives inside the sheet, so subtract the
  // sheet's own offset to place it. The title row beside the small cover follows the same way.
  const artwork = interpolateFrame(miniArtwork, fullArtwork, progress);
  artwork.top -= sheetOffset(progress);
  applyArtworkFrame(artwork);
  headerY.set(artwork.top - fullArtwork.top);
  // In the sheet's space, from where a text sits inside the bar to its slot. It starts out riding
  // the sheet together with the rest of the bar's content (a smoothstep has no slope at 0) and has
  // caught up with a straight blend by the middle.
  const weight = progress * progress * (3 - 2 * progress);
  // The colour turns with the sheet's fade-in, so the text and what is behind it change together.
  const turn = easeOutCubic(rangeProgress(progress, 0, MOBILE_SHEET_FADE_END));
  textKinds.forEach((kind) => {
    const frames = texts[kind];
    if (!frames) return;
    const text = sharedTexts[kind];
    // The sheet-space path described above, for each part of the frame. The window is the bar's at
    // the start and the slot's at the end, so the cut-off moves smoothly instead of jumping when
    // the float takes over from the bar (or hands back to it).
    text.left.set(mix(frames.mini.left, frames.full.left, weight));
    text.top.set(mix(frames.mini.top - sheetTravel.get(), frames.full.top, weight));
    text.size.set(mix(frames.mini.size, frames.full.size, weight));
    text.span.set(mix(frames.mini.width, frames.full.width, weight));
    if (!frames.color) return;
    const [r, g, b, a] = frames.color.from.map((value, i) => mix(value, frames.color!.to[i], turn));
    text.color.set(`rgba(${r}, ${g}, ${b}, ${a})`);
  });
};

// The mini bar is the sheet's top while it lifts: it rides the sheet's edge and fades as it goes,
// instead of fading in place and leaving a new sheet to appear behind it. A transform on the
// bar's own element, not a variable on <html>: that restyles the whole document every frame, and
// this one changes for the whole span. Cleared at rest, where the bar's rect is what the next
// gesture measures.
let miniBarElement: HTMLElement | null = null;
let miniBarRise = 0;
const setMiniBarRise = (y: number) => {
  if (miniBarRise === y) return;
  if (!miniBarElement?.isConnected) {
    miniBarElement = document.querySelector<HTMLElement>("[data-mobile-player-bg]");
  }
  if (!miniBarElement) return;
  miniBarRise = y;
  miniBarElement.style.transform = y ? `translate3d(0, ${y}px, 0)` : "";
};

const clearMiniUiVars = () => {
  setMiniBarRise(0);
  removeMiniUiVar("--mobile-mini-player-root-y");
  removeMiniUiVar("--mobile-mini-player-z-index");
  removeMiniUiVar("--mobile-mini-player-pointer-events");
  removeMiniUiVar("--mobile-mini-player-surface-opacity");
  removeMiniUiVar("--mobile-mini-player-ui-opacity");
  removeMiniUiVar("--mobile-mini-player-chrome-opacity");
  removeMiniUiVar("--mobile-mini-player-text-opacity");
  removeMiniUiVar("--mobile-mini-player-detail-opacity");
  removeMiniUiVar("--mobile-mini-player-detail-height");
  removeMiniUiVar("--mobile-mini-player-detail-margin");
  removeMiniUiVar("--mobile-mini-player-ui-y");
  removeMiniUiVar("--mobile-mini-player-text-y");
  removeMiniUiVar("--mobile-mini-player-title-opacity");
  removeMiniUiVar("--mobile-mini-player-artists-opacity");
  removeMiniUiVar("--mobile-mini-player-artwork-opacity");
  removeMiniUiVar("--mobile-mini-player-bottom-y");
  removeMiniUiVar("--mobile-mini-player-bottom-z-index");
  removeMiniUiVar("--mobile-mini-player-bottom-pointer-events");
};

const applyMiniUiVars = (progress: number) => {
  const miniTransitioning = progress > 0.001 || mobileTransitionActive.value || music.showBigPlayer;
  // Everything the mini bar sheds is a function of progress, never a switch. A switch is a
  // one-frame cut, and the cut is what read as the bar blinking out before the sheet arrived.
  // Linear: the bar is seen travelling up, so the fade must not be front-loaded.
  const chromeExit = rangeProgress(progress, 0, MOBILE_MINI_CHROME_FADE_END);
  const tabBarExit = easeOutCubic(rangeProgress(progress, 0, MOBILE_TABBAR_EXIT_END));
  const chromeOpacity = String(1 - chromeExit);
  setMiniUiVar(
    "--mobile-mini-player-z-index",
    miniTransitioning && progress < MOBILE_MINI_UI_FADE_END ? "2102" : "2",
  );
  setMiniUiVar("--mobile-mini-player-pointer-events", miniTransitioning ? "none" : "auto");
  // Drives .bottom-glass in App.vue — the shared glass behind the mini bar and the tab
  // bar. It has to dissolve as BigPlayer takes over, or the band's blur outlives the
  // bars it belongs to and smears the expanding artwork.
  setMiniUiVar("--mobile-mini-player-surface-opacity", chromeOpacity);
  setMiniUiVar("--mobile-mini-player-ui-opacity", chromeOpacity);
  setMiniUiVar("--mobile-mini-player-chrome-opacity", chromeOpacity);
  setMiniUiVar("--mobile-mini-player-text-opacity", chromeOpacity);
  setMiniUiVar("--mobile-mini-player-detail-opacity", chromeOpacity);
  setMiniUiVar("--mobile-mini-player-ui-y", `${mix(0, -4, chromeExit)}px`);
  setMiniUiVar("--mobile-mini-player-text-y", `${mix(0, -8, chromeExit)}px`);
  // The name, and the artists when the bar shows them, are shared: each hands over to its floating
  // element in the same frame, like the artwork, so there is never a second copy on screen. A lyric
  // line is not shared and fades with the bar.
  setMiniUiVar("--mobile-mini-player-title-opacity", sharedTexts.name.morph.get() ? "0" : "1");
  setMiniUiVar("--mobile-mini-player-artists-opacity", sharedTexts.artists.morph.get() ? "0" : "1");
  // The album layer sits on the exact same rect at progress 0, so this swap is invisible.
  setMiniUiVar("--mobile-mini-player-artwork-opacity", miniTransitioning ? "0" : "1");
  setMiniUiVar("--mobile-mini-player-bottom-y", `${mix(0, 110, tabBarExit)}%`);
  setMiniUiVar(
    "--mobile-mini-player-bottom-z-index",
    miniTransitioning && progress < MOBILE_TABBAR_EXIT_END ? "2101" : "1000",
  );
  setMiniUiVar("--mobile-mini-player-bottom-pointer-events", miniTransitioning ? "none" : "auto");
};

const applyProgressState = (value: number) => {
  if (!isMobile.value) {
    clearMiniUiVars();
    artworkOpacity.set(1);
    mobileAlbumLayerReady.value = false;
    return;
  }
  const progress = clamp(value);
  const backgroundVisible = progress > 0.001 || music.showBigPlayer || mobileTransitionActive.value;
  const albumLayerOpacity = backgroundVisible ? 1 : 0;
  // The earliest window: the shell must be visible for the handle to show.
  const contentVisible = progress >= MOBILE_HANDLE_REVEAL_START;
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
  artworkOpacity.set(albumLayerOpacity);
  applyMiniUiVars(progress);
  setMiniBarRise(progress > 0.001 && progress < 0.999 ? -progress * sheetTravel.get() : 0);

  if (!mobileInteractive.value || !interactiveFrames) {
    headerY.set(0);
    return;
  }
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
  progressAnimation = null;
  if (prefersReducedMotion()) {
    playerProgress.set(clamp(target));
    onComplete?.();
    return;
  }
  const animation = animate(playerProgress, clamp(target), {
    ...layerMotion.settle,
    onComplete: () => {
      if (progressAnimation !== animation) return;
      progressAnimation = null;
      onComplete?.();
    },
  });
  progressAnimation = animation;
};

const resetClosedMobileState = () => {
  interactiveGeneration++;
  resolveInteractive?.(false);
  resolveInteractive = undefined;
  stopArtworkFrameAnimations();
  mobileInteractive.value = false;
  mobileTransitionActive.value = false;
  mobileTransitionDirection.value = null;
  mobileExiting.value = false;
  mobileContentReady.value = false;
  mobileContentVisible.value = false;
  mobileTransitionPhase.value = "mini";
  mobileAlbumLayerReady.value = false;
  pendingInteractiveProgress = 0;
  releaseInteractiveFrames();
  textKinds.forEach((kind) => setTextMorph(kind, null));
  playerProgress.set(0);
  headerY.set(0);
  artworkOpacity.set(0);
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
      textKinds.forEach(animateTextToCurrentLayer);
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
  ".desktop-comment-panel",
  ".immersive-dock",
  ".immersive-lyrics",
  ".immersive-comment-stage",
  ".amll-close-action",
  ".control-thumb",
  ".controls",
  ".lrcShow",
  ".desktop-lyric-offset",
  // The share layer's empty background still drags, like the player's.
  ".lyric-share .column",
  ".lyric-share .stage",
  ".lyric-share .dismiss",
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
    navigation.closeTop("player");
  }
  cleanupClosedMobileTransition();
};

const seedInteractiveFromFull = () => {
  // A layer switch may still be under way: the texts start the close from where they are (as the
  // artwork does below), not from a slot they have not reached, or they would jump to it.
  const rest: NonNullable<typeof restTextFrames> = {};
  textKinds.forEach((kind) => {
    const text = sharedTexts[kind];
    if (text.metrics.value) rest[kind] = readTextFrame(text);
  });
  restTextFrames = rest;
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

const openMobileQueue = () => navigation.openQueue();
const closeMobileQueue = () => navigation.closeQueue();

const resetDesktopOverlayState = () => {
  desktopCommentsOpen.value = false;
};

const openDesktopComments = () => {
  if (!music.getPlaySongData?.id) return;
  if (navigation.current.value?.kind === "queue") navigation.closeQueue();
  desktopCommentsOpen.value = true;
  if (hasLyrics.value) desktopLyricsVisible.value = true;
};

const toggleDesktopQueue = () => {
  const nextState = !desktopQueueOpen.value;
  if (nextState) navigation.openQueue();
  else navigation.closeQueue();
  if (nextState) desktopCommentsOpen.value = false;
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
  direction: "opening" | "closing",
) => {
  if (!isMobile.value) return false;
  const retainFrames = mobileInteractive.value && Boolean(interactiveFrames);
  const generation = ++interactiveGeneration;
  resolveInteractive?.(music.showBigPlayer);
  resolveInteractive = undefined;
  progressAnimation?.stop();
  progressAnimation = null;
  stopArtworkFrameAnimations();
  pendingInteractiveProgress = clamp(initialProgress);
  mobileTransitionDirection.value = direction;
  if (pendingInteractiveProgress <= 0.001) {
    mobileContentReady.value = false;
    mobileContentVisible.value = false;
    mobileTransitionPhase.value = "mini";
    mobileAlbumLayerReady.value = false;
  }
  mobileInteractive.value = true;
  mobileTransitionActive.value = true;
  mobileExiting.value = false;
  clearMobileExitFallback();
  if (!retainFrames) measureSheetTravel(frames);
  playerProgress.jump(pendingInteractiveProgress);
  await nextTick();
  if (generation !== interactiveGeneration) return false;
  if (!retainFrames && initialProgress <= 0.001) {
    detachMiniSharedAlbum();
    seedInteractiveFromMini(frames);
    // A drag has already moved by the time the next frame comes round, so the artwork and the
    // title must follow the finger from the first painted frame; the frame-after capture below
    // only refreshes these numbers.
    captureInteractiveFrames(frames);
  } else if (!retainFrames && initialProgress >= 0.999) {
    prepareMiniAlbumCloseHandoff();
    seedInteractiveFromFull();
  }
  applyProgressState(playerProgress.get());

  await nextTick();
  if (generation !== interactiveGeneration) return false;
  await new Promise<void>((resolve) => {
    requestAnimationFrame(() => {
      if (generation !== interactiveGeneration) return resolve();
      if (
        !retainFrames &&
        captureInteractiveFrames(frames) &&
        initialProgress >= 0.999 &&
        interactiveFrames
      ) {
        interactiveFrames.fullArtwork = readCurrentArtworkFrame();
      }
      // Touch moves can arrive during preparation; keep their latest visual progress.
      applyProgressState(playerProgress.get());
      resolve();
    });
  });
  return generation === interactiveGeneration;
};

const beginMobileInteractiveOpen = (frames?: MiniSharedFrames) => {
  return beginMobileInteractive(frames, 0, "opening");
};

const beginMobileInteractiveClose = () => {
  if (mobileQueueOpen.value) return;
  const progress = clamp(playerProgress.get());
  // Invert drag resistance so grabbing a partially open player does not change its position.
  closeDragOrigin = 1 - Math.pow(1 - progress, 1 / MOBILE_DRAG_DAMPING_POWER);
  return beginMobileInteractive(undefined, progress, "closing");
};

const updateMobileInteractiveProgress = (progress: number) => {
  const nextProgress = clamp(progress);
  pendingInteractiveProgress = nextProgress;
  if (!mobileInteractive.value) return;
  playerProgress.set(dampInteractiveDragProgress(nextProgress));
};

const updateMobileInteractiveClose = (distance: number) => {
  updateMobileInteractiveProgress(closeDragOrigin - distance / getTransitionDistance());
};

const beginStoreCloseHandoff = () => {
  // Also when the store has already closed (back, Escape): the overlay must outlive the flag.
  mobileExiting.value = true;
  if (!music.showBigPlayer) return;
  mobileSkipNextStoreCloseAnimation = true;
  navigation.closeTop("player");
};

const finishMobileInteractive = (forceOpen?: boolean) =>
  new Promise<boolean>((resolve) => {
    resolveInteractive?.(music.showBigPlayer);
    resolveInteractive = resolve;
    // 甩动优先：释放速度足够时直接顺着惯性判向，不足半程也能一次关闭/打开
    const velocity = playerProgress.getVelocity();
    const shouldOpen =
      forceOpen ?? (Math.abs(velocity) > 0.6 ? velocity > 0 : pendingInteractiveProgress >= 0.5);
    mobileTransitionDirection.value = shouldOpen ? "opening" : "closing";
    if (shouldOpen) {
      mobileExiting.value = false;
      clearMobileExitFallback();
      detachMiniSharedAlbum();
      if (!music.showBigPlayer)
        void navigation.openPlayer(
          document.querySelector<HTMLElement>("[data-mobile-player-bg]") ?? undefined,
        );
    } else {
      restoreMiniSharedAlbum();
      beginStoreCloseHandoff();
    }
    animateProgressTo(shouldOpen ? 1 : 0, () => {
      if (!shouldOpen) {
        completeClosedMobileTransition(music.showBigPlayer);
      } else {
        mobileInteractive.value = false;
        mobileTransitionActive.value = false;
        mobileTransitionDirection.value = null;
        releaseInteractiveFrames();
        glideToCurrentLayer();
        applyProgressState(1);
      }
      if (resolveInteractive === resolve) resolveInteractive = undefined;
      resolve(shouldOpen);
    });
  });

const finishMobileInteractiveOpen = (forceOpen?: boolean) => {
  return finishMobileInteractive(forceOpen);
};

const finishMobileInteractiveClose = () => {
  return finishMobileInteractive();
};

const cancelMobileInteractiveClose = () => finishMobileInteractive(true);

const openMobileFromMini = async (frames?: MiniSharedFrames) => {
  if (!isMobile.value) {
    void navigation.openPlayer(
      document.querySelector<HTMLElement>("[data-mobile-player-bg]") ?? undefined,
    );
    return true;
  }
  if (!(await beginMobileInteractiveOpen(frames))) return false;
  return finishMobileInteractive(true);
};

const closeMobileWithProgress = async () => {
  if (!isMobile.value) {
    closeBigPlayer();
    return;
  }
  if (await beginMobileInteractiveClose()) await finishMobileInteractive(false);
};

// For a page opened from the player: retract first, so the page lands on the one
// underneath and back returns there rather than to a parked player. True only once
// the player layer has really left the stack.
const retractMobilePlayer = async () => {
  if (!(await beginMobileInteractiveClose())) return false;
  if (await finishMobileInteractive(false)) return false;
  // closeTop's router.back() lands asynchronously, and under reduced motion the
  // animation does not outlast it.
  if (navigation.hasLayer("player")) {
    await new Promise<void>((resolve) => {
      const stop = watch(
        () => navigation.hasLayer("player"),
        () => {
          stop();
          resolve();
        },
      );
    });
  }
  return true;
};

const openPageFromMobilePlayer = async (to: RouteLocationRaw) => {
  if (await retractMobilePlayer()) navigation.openPage(to);
};

const removeCancelledNavigation = navigation.onNavigationCancelled(() => {
  if (!isMobile.value || !mobileTransitionActive.value) return;
  interactiveGeneration++;
  mobileSkipNextStoreCloseAnimation = false;
  void finishMobileInteractive(music.showBigPlayer);
});

const handleMobileClose = () => {
  if (mobileQueueOpen.value) {
    closeMobileQueue();
    return;
  }
  closeMobileWithProgress();
};

useMotionInterruption(() => {
  interactiveGeneration++;
  progressAnimation?.stop();
  progressAnimation = null;
  stopArtworkFrameAnimations();
  clearMobileExitFallback();
  resolveInteractive?.(music.showBigPlayer);
  resolveInteractive = undefined;
  mobileSkipNextStoreCloseAnimation = false;
  if (!isMobile.value || !music.showBigPlayer) {
    restoreMiniSharedAlbum();
    resetClosedMobileState();
  } else {
    pendingInteractiveProgress = 1;
    mobileInteractive.value = false;
    mobileTransitionActive.value = false;
    mobileTransitionDirection.value = null;
    mobileExiting.value = false;
    releaseInteractiveFrames();
    playerProgress.set(1);
    applyProgressState(1);
    detachMiniSharedAlbum();
    scheduleArtworkLayerSync();
  }
  [desktopLayoutRef.value?.leftContentRef, desktopLayoutRef.value?.rightContentRef].forEach(
    (element) => {
      if (!element) return;
      gsap.killTweensOf(element);
      gsap.set(element, { clearProps: "opacity,transform,transition" });
    },
  );
  const leftEl = desktopLayoutRef.value?.leftContentRef;
  if (leftEl) clearLeftEnter(leftEl);
});

const initMobileElements = () => {
  if (!isMobile.value) return;
  nextTick(() => checkNameOverflow());
};

// GSAP 提示动画
const animateTip = (isVisible: boolean) => {
  const tipEl = desktopLayoutRef.value?.tipRef;
  if (!tipEl) return;
  // GSAP 不吃 CSS 的 prefers-reduced-motion 兜底，这里显式跳到终态
  if (prefersReducedMotion()) {
    gsap.set(tipEl, { opacity: isVisible ? 1 : 0, y: 0 });
    return;
  }
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
// .right 自带 opacity/transform 的 CSS transition（供 lyrics-hidden 切换用），
// 会把 GSAP 立即写入的 from 状态再做一次过渡，造成 亮->暗->亮 闪烁；
// 入场期间必须先禁用元素自身 transition，结束后 clearProps 恢复。
// .left 不能直接动：它在 metadata plus-lighter 到背景的路上，动 opacity/transform 会让
// metadata 在动画期间失去加亮、落定才突然亮起。只补间变量，由内容各自套用（DesktopPlayerLayout）。
const LEFT_ENTER_VARS = ["--left-enter-opacity", "--left-enter-scale"];
const clearLeftEnter = (el: HTMLElement) =>
  LEFT_ENTER_VARS.forEach((name) => el.style.removeProperty(name));

const animatePlayerIn = () => {
  if (!bigPlayerRef.value || isMobile.value) return;
  const leftEl = desktopLayoutRef.value?.leftContentRef;
  const rightEl = desktopLayoutRef.value?.rightContentRef;
  // 同上：减弱动态效果时直接呈现终态，不做缩放淡入
  if (prefersReducedMotion()) {
    if (leftEl) clearLeftEnter(leftEl);
    if (rightEl) gsap.set(rightEl, { clearProps: "opacity,transform,transition" });
    return;
  }
  if (leftEl) {
    gsap.fromTo(
      leftEl,
      { "--left-enter-opacity": 0, "--left-enter-scale": 0.96 },
      {
        "--left-enter-opacity": 1,
        "--left-enter-scale": 1,
        duration: 0.5,
        delay: 0.15,
        ease: "power2.out",
        onComplete: () => clearLeftEnter(leftEl),
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
    // The sheet is derived from the progress, so it has to move too, not only the side state.
    playerProgress.set(music.showBigPlayer ? 1 : 0);
    applyProgressState(playerProgress.get());
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
  removeCancelledNavigation();
  interactiveGeneration++;
  resolveInteractive?.(music.showBigPlayer);
  resolveInteractive = undefined;
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
        mobileExiting.value = false;
        clearMobileExitFallback();
        initMobileElements();
        if (resumingPlayer.value) {
          playerProgress.set(1);
          applyProgressState(1);
          detachMiniSharedAlbum();
          scheduleArtworkLayerSync();
        } else if (mobileInteractive.value) {
          return;
        } else if (playerProgress.get() < 0.999) {
          void openMobileFromMini();
        } else {
          scheduleArtworkLayerSync();
          applyProgressState(1);
        }
        nextTick(() => lyricsScroll(music.getPlaySongLyricIndex));
        return;
      }
      resetDesktopOverlayState();
      clearMiniUiVars();
      requestAnimationFrame(() => {
        lyricsScroll(music.getPlaySongLyricIndex);
        if (!resumingPlayer.value) animatePlayerIn();
      });
    } else if (navigation.hasLayer("player")) {
      interactiveGeneration++;
      resolveInteractive?.(false);
      resolveInteractive = undefined;
      progressAnimation?.stop();
      progressAnimation = null;
      mobileInteractive.value = false;
      mobileTransitionActive.value = false;
      mobileExiting.value = false;
      clearMobileExitFallback();
      restoreMiniSharedAlbum();
      clearMiniUiVars();
    } else if (isMobile.value) {
      if (mobileSkipNextStoreCloseAnimation) {
        mobileSkipNextStoreCloseAnimation = false;
        clearMobileExitFallback();
        return;
      }
      // Back and Escape close the way a drag does: the sheet sinks into the mini bar with
      // its content, instead of fading where it stands.
      mobileExiting.value = true;
      void closeMobileWithProgress();
      scheduleMobileExitFallback();
    } else {
      resetDesktopOverlayState();
    }
  },
);

watch(
  () => isMobile.value,
  (val) => {
    if (!val) {
      progressAnimation?.stop();
      progressAnimation = null;
      resetDesktopOverlayState();
      mobileExiting.value = false;
      mobileTransitionActive.value = false;
      mobileTransitionDirection.value = null;
      mobileAlbumLayerReady.value = false;
      stopArtworkFrameAnimations();
      clearMiniUiVars();
      clearMobileExitFallback();
    } else {
      playerProgress.set(music.showBigPlayer ? 1 : 0);
      applyProgressState(playerProgress.get());
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
.bplayer.navigation-suspended {
  visibility: hidden !important;
  pointer-events: none !important;
}
.bplayer.navigation-resuming {
  transition: none !important;
}

.share-fade {
  transition: opacity 0.25s ease-out;
}

.share-hidden {
  opacity: 0;
}

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
    border-radius var(--duration-200) var(--ease-out),
    transform var(--duration-500) cubic-bezier(0.25, 1, 0.5, 1);

  // Opened state
  &.opened {
    pointer-events: auto;
    transform: translateY(0%);
    border-radius: 0;
    transition:
      border-radius var(--duration-200) var(--ease-out) 0.25s,
      transform var(--duration-500) cubic-bezier(0.25, 1, 0.5, 1);
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

    .mobile-scrim {
      position: absolute;
      inset: 0;
      background: #000;
      opacity: 0;
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
      // The shell is the sheet: background and content move as one layer (y and top corners
      // come from `mobileShellMotionStyle`), like the queue drawer's panel.
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
        // 这一层裹着背景画布（下面的 :deep(.big-player-background)），保持单独的合成层。
        will-change: opacity;

        :deep(.big-player-background) {
          z-index: 0;
          will-change: transform, opacity;
        }
      }

      :deep(.mobile-thumb) {
        pointer-events: none;
      }

      // 封面的 40px 模糊阴影随尺寸每帧重绘；移动期间直接去掉，落定后由封面自己的
      // box-shadow 过渡淡入。
      &.interactive-transition :deep(.mobile-cover-frame) {
        box-shadow: none;
        transition: none;
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
      transition: opacity var(--duration-500) var(--ease-out);
      pointer-events: none;
    }

    :deep(.mobile-cover-layout) {
      pointer-events: none;
    }

    :deep(.mobile-lyric),
    :deep(.no-lyrics) {
      opacity: 0;
      transition: opacity var(--duration-500) var(--ease-out);
      pointer-events: none;
    }

    :deep(.mobile-big-controls) {
      opacity: 1;
    }

    &.mobile-visible {
      :deep(.mobile-thumb),
      :deep(.mobile-cover-frame),
      :deep(.mobile-cover-layout),
      :deep(.lyric-share) {
        pointer-events: auto;
      }
    }

    // Layer 2 激活 (= AMLL default)
    &.layer2-active {
      :deep(.mobile-small-controls) {
        opacity: 1;
        transition: opacity var(--duration-200) var(--ease-out) 0.25s;
      }

      :deep(.mobile-cover-layout) {
        pointer-events: none;
      }

      :deep(.mobile-lyric),
      :deep(.no-lyrics) {
        opacity: 1;
        transition: opacity var(--duration-500) var(--ease-out) 0.5s;
      }

      // Hidden controls sit a little low and rise into place, so they read as arriving.
      :deep(.mobile-big-controls) {
        opacity: 0;
        transform: translate3d(0, 12px, 0);
        pointer-events: none;
      }

      :deep(.mobile-big-controls.lyric-controls-visible) {
        opacity: 1;
        transform: none;
      }

      @media (prefers-reduced-motion: reduce) {
        :deep(.mobile-big-controls) {
          transform: none;
        }
      }
    }

    &.mobile-visible.layer2-active {
      :deep(.mobile-big-controls.lyric-controls-visible .mobile-controls-motion),
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
  transition: opacity var(--duration-300) cubic-bezier(0.34, 1.56, 0.64, 1);
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
