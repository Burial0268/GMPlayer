<template>
  <div
    ref="pagesRef"
    :class="['mobile-pages', { 'queue-open': queueOpen, tinted }]"
    :style="{
      '--mobile-lyric-controls-height': `${lyricControlsHeight}px`,
      '--pager-shift': pagerShiftCss,
      '--queue-offset': queueOffsetCss,
    }"
  >
    <div class="mobile-player-page">
      <div class="mobile-player-content">
        <div class="mobile-full-ui">
          <!-- AMLL .thumb — 抽屉把手 -->
          <Motion
            class="mobile-thumb"
            :style="handleUiMotionStyle"
            @click="handleThumbClick"
            @touchstart.passive="handlePlayerTouchStart"
            @touchmove.passive="handlePlayerTouchMove"
            @touchend.passive="handlePlayerTouchEnd"
            @touchcancel="handlePlayerTouchCancel"
          >
            <div class="handle-bar"></div>
          </Motion>

          <!-- AMLL .lyricLayout — Layer 2: 紧凑封面信息 + 歌词 -->
          <div
            :class="[
              'mobile-lyric-layout',
              { active: activeMobileLayer === 2, 'controls-visible': lyricControlsVisible },
            ]"
          >
            <div class="mobile-phony-small-cover mobile-cover-slot" ref="phonySmallCoverRef"></div>
            <div class="mobile-small-controls">
              <Motion class="mobile-small-controls-inner" :style="smallControlsMotionStyle">
                <div ref="smallSongInfoRef" class="mobile-song-info">
                  <Motion as-child :style="titleRealStyle">
                    <div class="name-wrapper">
                      <div class="name" :class="{ 'is-marquee': isNameOverflow }">
                        <span class="name-inner name-measure">{{
                          songName || $t("other.noSong")
                        }}</span>
                        <OverflowMarquee
                          v-if="isNameOverflow"
                          class="mobile-name-marquee"
                          :speed="36"
                        >
                          <span class="mobile-name-marquee-content">
                            {{ songName || $t("other.noSong") }}
                          </span>
                        </OverflowMarquee>
                      </div>
                    </div>
                  </Motion>
                  <div
                    :class="['artists', 'text-hidden', { 'is-shared': artistsMetrics }]"
                    v-if="artistList.length"
                  >
                    <span v-for="(item, index) in artistList" :key="'s' + index">
                      {{ item.name }}<span v-if="index != artistList.length - 1"> / </span>
                    </span>
                  </div>
                </div>
                <div class="mobile-header-actions">
                  <n-icon
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
                  <n-icon size="24" :component="MoreVertRound" @click.stop="$emit('openMenu')" />
                </div>
              </Motion>
            </div>
            <div
              v-if="hasLyrics"
              class="mobile-lyric"
              @touchmove.capture.passive="revealLyricControls"
              @wheel.capture.passive="handleLyricWheel"
            >
              <Motion class="mobile-lyric-inner" :style="contentUiMotionStyle">
                <RollingLyrics
                  @mouseenter="$emit('lrcMouseEnter')"
                  @mouseleave="$emit('lrcAllLeave')"
                  @lrcTextClick="$emit('lrcTextClick', $event)"
                  class="mobile-lyrics"
                />
                <LyricOffsetControl class="mobile-lyric-offset" />
              </Motion>
            </div>
            <div
              v-else
              class="no-lyrics"
              @touchmove.capture.passive="revealLyricControls"
              @wheel.capture.passive="handleLyricWheel"
            >
              <Motion class="mobile-ui-empty" :style="contentUiMotionStyle">
                <span>¯\_(ツ)_/¯</span>
              </Motion>
            </div>
          </div>

          <!-- AMLL .noLyricLayout — Layer 1: 大封面 + 歌曲信息 + controls -->
          <div :class="['mobile-cover-layout', { 'lyric-layer': activeMobileLayer === 2 }]">
            <div class="mobile-phony-big-cover mobile-cover-slot" ref="phonyBigCoverRef"></div>
            <div
              :class="['mobile-big-controls', { 'lyric-controls-visible': lyricControlsVisible }]"
              :inert="activeMobileLayer === 2 && !lyricControlsVisible"
              :aria-hidden="activeMobileLayer === 2 && !lyricControlsVisible"
            >
              <div class="mobile-big-controls-inner" :style="contentShellStyle">
                <!-- 歌曲信息（展开） -->
                <div
                  class="mobile-song-info-row"
                  :inert="activeMobileLayer === 2"
                  :aria-hidden="activeMobileLayer === 2"
                >
                  <div ref="bigSongInfoRef" class="mobile-song-info">
                    <Motion as-child :style="titleRealStyle">
                      <div class="name-wrapper" ref="nameWrapperRef">
                        <div
                          class="name"
                          ref="nameTextRef"
                          :class="{ 'is-marquee': isNameOverflow }"
                        >
                          <span class="name-inner name-measure">{{
                            songName || $t("other.noSong")
                          }}</span>
                          <OverflowMarquee
                            v-if="isNameOverflow"
                            class="mobile-name-marquee"
                            :speed="36"
                          >
                            <span class="mobile-name-marquee-content">
                              {{ songName || $t("other.noSong") }}
                            </span>
                          </OverflowMarquee>
                        </div>
                      </div>
                    </Motion>
                    <div
                      :class="['artists', 'text-hidden', { 'is-shared': artistsMetrics }]"
                      v-if="artistList.length"
                    >
                      <span v-for="(item, index) in artistList" :key="'b' + index">
                        {{ item.name }}<span v-if="index != artistList.length - 1"> / </span>
                      </span>
                    </div>
                  </div>
                  <div class="mobile-header-actions">
                    <n-icon
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
                    <n-icon size="24" :component="MoreVertRound" @click.stop="$emit('openMenu')" />
                  </div>
                </div>
                <div
                  ref="playbackControlsRef"
                  class="mobile-controls-motion"
                  @pointerdown.capture="holdLyricControls"
                  @click.capture="scheduleLyricControlsHide"
                  @focusin="clearLyricControlsTimer"
                  @focusout="scheduleLyricControlsHide"
                >
                  <!-- 进度条 -->
                  <div class="mobile-progress">
                    <BouncingSlider
                      :value="music.getPlaySongTime.currentTime || 0"
                      :min="0"
                      :max="music.getPlaySongTime.duration || 1"
                      :is-playing="music.getPlayState"
                      @update:value="handleProgressSeek"
                    />
                    <div class="time-display">
                      <span>{{ music.getPlaySongTime.songTimePlayed }}</span>
                      <span v-if="qualityText" class="quality-badge">
                        <n-icon :component="IconLossless" />
                        <span class="quality-label">{{ qualityText }}</span>
                      </span>
                      <span>-{{ remainingTime }}</span>
                    </div>
                  </div>
                  <!-- 控制按钮 + 音量 -->
                  <MobileControls @toComment="$emit('toComment')" />
                  <!-- Apple Music's bottom bar: lyrics toggles the two layers, queue pages over. -->
                  <div class="mobile-bottom-bar">
                    <button
                      type="button"
                      :aria-pressed="activeMobileLayer === 2"
                      :aria-label="$t('player.controls.lyrics')"
                      @click.stop="$emit('switchLayer')"
                    >
                      <component :is="activeMobileLayer === 2 ? IconLyricsOn : IconLyricsOff" />
                    </button>
                    <OutputDeviceButton v-if="nativeOutput" />
                    <button
                      type="button"
                      :aria-label="$t('general.name.playlists')"
                      @click.stop="$emit('openQueue')"
                    >
                      <IconPlaylistOff />
                    </button>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Single visible album layer. The cover slots above are measurement anchors only. -->
        <MobileCoverFrame
          :visible="albumLayerVisible"
          :motionStyle="coverFrameStyle"
          :coverUrl="coverImageUrl500"
          :layoutTransition="layoutTransition"
          :layoutDependency="mobileLayer"
          :layoutEnabled="false"
          :layoutId="null"
          :borderRadius="12"
          :shrunk="coverShrunk"
          :dimmed="coverShrunk && music.getLoadingState"
          interactive
          @click="handleCoverClick"
          @touchstart.passive="handlePlayerTouchStart"
          @touchmove.passive="handlePlayerTouchMove"
          @touchend.passive="handlePlayerTouchEnd"
          @touchcancel="handlePlayerTouchCancel"
        />

        <!-- 唯一可见的歌名：和封面一样不受两层布局的裁剪，从 mini 条的歌名位置缩放到各层的
             歌名位置并留在那里；两层里真实的歌名只用来量位置，不显示。 -->
        <Motion
          v-if="titleMetrics"
          class="mobile-title-frame"
          aria-hidden="true"
          :style="titleFrameStyle"
        >
          <div class="title-name">
            <OverflowMarquee
              v-if="isNameOverflow && !textTravelling"
              class="title-marquee"
              :speed="36"
            >
              <span class="title-marquee-content">{{ songName || $t("other.noSong") }}</span>
            </OverflowMarquee>
            <template v-else>{{ songName || $t("other.noSong") }}</template>
          </div>
        </Motion>

        <!-- 歌手行同理，只在 mini 条显示的是歌手（不是歌词）时共享。 -->
        <Motion
          v-if="artistsMetrics"
          class="mobile-artists-frame"
          aria-hidden="true"
          :style="artistsFrameStyle"
        >
          <div class="artists-line">{{ artistNames }}</div>
        </Motion>
      </div>
    </div>

    <!-- 移动端待播清单 — 与主内容同级的分页：共用背景，由 pager 平移切换 -->
    <div
      class="mobile-queue-layout"
      data-navigation-layer="queue-player"
      :style="contentShellStyle"
      @touchstart.passive="handleQueueTouchStart"
      @touchmove="handleQueueTouchMove"
      @touchend.passive="handleQueueTouchEnd"
      @touchcancel="handleQueueTouchCancel"
    >
      <div class="mobile-thumb" @click="$emit('closeQueue')">
        <div class="handle-bar"></div>
      </div>
      <div class="mobile-queue-content">
        <div class="mobile-queue-panel">
          <PlayerQueueList
            v-if="music.getPlaylists.length"
            ref="queueListRef"
            class="mobile-queue-list"
            row-prefix="mobile-queue"
            @scroll="handleQueueScroll"
          />
          <div class="queue-empty" v-else>
            {{ $t("other.playlistEmpty") }}
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { MoreVertRound, StarBorderRound, StarRound } from "@vicons/material";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { animate, Motion, useMotionValue, useTransform, type MotionValue } from "motion-v";
import { musicStore } from "@/store";
import { prefersReducedMotion } from "@/utils/reducedMotion";
import { useLayerNavigation } from "@/utils/navigation";
import { layerMotion } from "@/utils/navigation/motion";
import { useMotionInterruption } from "@/composables/useMotionInterruption";
import RollingLyrics from "../RollingLyrics.vue";
import BouncingSlider from "../BouncingSlider.vue";
import MobileControls from "./MobileControls.vue";
import MobileCoverFrame from "./MobileCoverFrame.vue";
import LyricOffsetControl from "./LyricOffsetControl.vue";
import IconLyricsOff from "../icons/IconLyricsOff.vue";
import IconLyricsOn from "../icons/IconLyricsOn.vue";
import IconPlaylistOff from "../icons/IconPlaylistOff.vue";
import IconLossless from "../icons/IconLossless.vue";
import OutputDeviceButton from "./OutputDeviceButton.vue";
import PlayerQueueList from "./PlayerQueueList.vue";
import { isTauri } from "@/utils/tauri/core/runtime";
import { useQualityText } from "@/composables/useQualityText";
import OverflowMarquee from "@/components/Common/OverflowMarquee.vue";

declare const $player: any;

type Artist = { name: string };
type StaticStyleRecord = Record<string, string | number | undefined>;
type MotionStyleRecord = Record<string, string | number | MotionValue | undefined>;
// Typography and slot of a real text (the song name, the artists) in the active layer, which its
// floating copy mirrors.
// What a float is laid out at; its glyph size and window on screen are motion values.
type TextMetrics = {
  size: number;
  height: number;
};

const props = defineProps<{
  songName: string;
  artistList: Artist[];
  isNameOverflow: boolean;
  hasLyrics: boolean;
  remainingTime: string;
  coverImageUrl500: string;
  handleProgressSeek: (val: number) => void;
  queueOpen: boolean;
  mobileLayer: number;
  layoutTransition: Record<string, unknown>;
  contentShellStyle: StaticStyleRecord;
  fullUiMotionStyle: MotionStyleRecord;
  headerMotionStyle: MotionStyleRecord;
  handleMotionStyle: MotionStyleRecord;
  titleMotionStyle: MotionStyleRecord;
  titleRealStyle: MotionStyleRecord;
  titleMetrics: TextMetrics | null;
  // The floats are on their way between the bar and a slot.
  textTravelling: boolean;
  artistsMotionStyle: MotionStyleRecord;
  artistsMetrics: TextMetrics | null;
  albumLayerStyle: MotionStyleRecord;
  albumLayerVisible: boolean;
  tinted: boolean;
}>();

const emit = defineEmits<{
  close: [];
  openQueue: [];
  closeQueue: [];
  switchLayer: [];
  lrcMouseEnter: [];
  lrcAllLeave: [];
  lrcTextClick: [time: number];
  toComment: [];
  openMenu: [];
  closeDragStart: [];
  closeDragMove: [distance: number];
  closeDragEnd: [];
  closeDragCancel: [];
}>();

const music = musicStore();
const navigation = useLayerNavigation();

// Expose phony refs and name refs for parent (cover frame composable + name overflow)
const phonyBigCoverRef = ref<HTMLElement | null>(null);
const phonySmallCoverRef = ref<HTMLElement | null>(null);
const nameWrapperRef = ref<HTMLElement | null>(null);
const nameTextRef = ref<HTMLElement | null>(null);
// The real title blocks of each layer: their natural slots are what the floating copy lands on.
const bigSongInfoRef = ref<HTMLElement | null>(null);
const smallSongInfoRef = ref<HTMLElement | null>(null);
const queueListRef = ref<InstanceType<typeof PlayerQueueList> | null>(null);
const queueScrollTop = ref(0);
const suppressCoverClick = ref(false);
const playerTouch = ref<{
  x: number;
  y: number;
  dragging: boolean;
  mode: "queue" | "close" | null;
  progress: number;
} | null>(null);
const queueTouch = ref<{
  x: number;
  y: number;
  dragging: boolean;
  scrollTop: number;
  progress: number;
} | null>(null);

const activeMobileLayer = computed(() => (props.mobileLayer === 2 ? 2 : 1));
const qualityText = useQualityText();
// The output menu drives the native backend; the web player has no device list to offer.
const nativeOutput = isTauri();
const playbackControlsRef = ref<HTMLElement | null>(null);
const lyricControlsHeight = ref(0);
const lyricControlsVisible = ref(false);
const lyricControlsActive = computed(
  () => activeMobileLayer.value === 2 && music.showBigPlayer && !props.queueOpen,
);
const LYRIC_CONTROLS_IDLE_MS = 3000;
let lyricControlsTimer: ReturnType<typeof setTimeout> | null = null;
let controlsPointerId: number | null = null;
let controlsResizeObserver: ResizeObserver | null = null;

const clearLyricControlsTimer = () => {
  if (lyricControlsTimer !== null) clearTimeout(lyricControlsTimer);
  lyricControlsTimer = null;
};

const scheduleLyricControlsHide = () => {
  clearLyricControlsTimer();
  if (!lyricControlsActive.value || !lyricControlsVisible.value || controlsPointerId !== null)
    return;
  lyricControlsTimer = setTimeout(() => {
    lyricControlsTimer = null;
    if (playbackControlsRef.value?.contains(document.activeElement)) return;
    lyricControlsVisible.value = false;
  }, LYRIC_CONTROLS_IDLE_MS);
};

// 只监听手指/滚轮输入；AMLL 的播放跟随滚动不能唤出控件。
const revealLyricControls = () => {
  if (!lyricControlsActive.value) return;
  lyricControlsVisible.value = true;
  scheduleLyricControlsHide();
};

const handleLyricWheel = (event: WheelEvent) => {
  if (event.deltaY !== 0) revealLyricControls();
};

const holdLyricControls = (event: PointerEvent) => {
  if (!lyricControlsActive.value || !lyricControlsVisible.value) return;
  controlsPointerId = event.pointerId;
  clearLyricControlsTimer();
};

const releaseLyricControls = (event: PointerEvent) => {
  if (event.pointerId !== controlsPointerId) return;
  controlsPointerId = null;
  scheduleLyricControlsHide();
};

watch(
  lyricControlsActive,
  (active) => {
    clearLyricControlsTimer();
    controlsPointerId = null;
    lyricControlsVisible.value = false;
    if (active) revealLyricControls();
  },
  { immediate: true },
);

const contentUiMotionStyle = computed<MotionStyleRecord>(() => ({
  ...props.contentShellStyle,
  ...props.fullUiMotionStyle,
}));
const smallControlsMotionStyle = computed<MotionStyleRecord>(() => ({
  ...props.contentShellStyle,
  ...props.headerMotionStyle,
}));
const textFrameStyle = (motion: MotionStyleRecord, metrics: TextMetrics | null) =>
  metrics
    ? {
        ...motion,
        "--text-size": `${metrics.size}px`,
        "--text-height": `${metrics.height}px`,
      }
    : motion;
// Same text the real line renders: names joined by " / ".
const artistNames = computed(() => props.artistList.map((item) => item.name).join(" / "));

// ── 队列分页：主内容与 Playlist 同级、共用大播放器背景，作为连续条带垂直平移 ──
// 0 = 主内容，1 = Playlist。位移不能写在 .mobile-player-content 上：它是歌词和
// Layer 1 的祖先，滑动期间的 transform 会把 plus-lighter 关进空组，落定才重新加亮。
// 和舞台切换一样，只发布 CSS 变量，由已经在混合的元素自己套用；静止时是 none。
const pagesRef = ref<HTMLElement | null>(null);
const pagerProgress = useMotionValue(props.queueOpen ? 1 : 0);
const pagerHeightValue = useMotionValue(window.innerHeight || 1);
let pagerAnimation: ReturnType<typeof animate> | null = null;

const clamp01 = (value: number) => Math.min(1, Math.max(0, value));
const pagerHeight = () => pagerHeightValue.get() || window.innerHeight || 1;
const measurePagerHeight = () => {
  pagerHeightValue.set(pagesRef.value?.clientHeight || window.innerHeight || 1);
};
const shiftOf = (px: number) => (px ? `translateY(${px}px)` : "none");
const playerShift = useTransform(() => shiftOf(-clamp01(pagerProgress.get()) * pagerHeight()));
const queueOffset = useTransform(() => `${(1 - clamp01(pagerProgress.get())) * pagerHeight()}px`);
const pagerShiftCss = ref(String(playerShift.get()));
const queueOffsetCss = ref(String(queueOffset.get()));
const publishPagerShift = () => {
  pagerShiftCss.value = String(playerShift.get());
  queueOffsetCss.value = String(queueOffset.get());
};
const stopPlayerShift = playerShift.on("change", publishPagerShift);
const stopQueueOffset = queueOffset.on("change", publishPagerShift);

const motionOffset = (value: MotionStyleRecord[string]) => {
  if (typeof value === "number") return value;
  if (typeof value === "string") return Number(value) || 0;
  if (value && typeof value.get === "function") return Number(value.get()) || 0;
  return 0;
};
const pagerOffset = useTransform(() => clamp01(pagerProgress.get()) * pagerHeight());
const titleY = props.titleMotionStyle.y;
const artistsY = props.artistsMotionStyle.y;
const shiftedTitleY = useTransform(() => motionOffset(titleY) - pagerOffset.get());
const shiftedArtistsY = useTransform(() => motionOffset(artistsY) - pagerOffset.get());
const titleFrameStyle = computed(() => ({
  ...textFrameStyle(props.titleMotionStyle, props.titleMetrics),
  y: shiftedTitleY,
}));
const artistsFrameStyle = computed(() => ({
  ...textFrameStyle(props.artistsMotionStyle, props.artistsMetrics),
  y: shiftedArtistsY,
}));
const handleUiMotionStyle = computed<MotionStyleRecord>(() => ({
  ...props.contentShellStyle,
  ...props.handleMotionStyle,
  transform: playerShift,
}));
// Layer 1 only, where the artwork is the page; and not while it travels to or from the mini bar,
// where a shrunk frame would land off its slot.
const coverShrunk = computed(
  () =>
    activeMobileLayer.value === 1 &&
    !props.textTravelling &&
    (!music.getPlayState || music.getLoadingState),
);

const coverFrameStyle = computed<MotionStyleRecord>(() => ({
  ...props.albumLayerStyle,
  transform: playerShift,
}));

const stopPagerAnimation = () => {
  pagerAnimation?.stop();
  pagerAnimation = null;
};

const grabPager = () => {
  const progress = clamp01(pagerProgress.get());
  stopPagerAnimation();
  pagerProgress.jump(progress);
  return progress;
};

const settlePager = (open: boolean) => {
  stopPagerAnimation();
  if (prefersReducedMotion()) pagerProgress.set(open ? 1 : 0);
  else pagerAnimation = animate(pagerProgress, open ? 1 : 0, layerMotion.settle);
  if (open !== props.queueOpen) {
    if (open) emit("openQueue");
    else emit("closeQueue");
  }
};

const settlePagerFromGesture = () => {
  const velocity = pagerProgress.getVelocity();
  if (Math.abs(velocity) > 0.6) {
    settlePager(velocity > 0);
    return;
  }
  // 位置阈值带方向偏置：从任一端出发都需拖过 35% 行程才切页
  settlePager(pagerProgress.get() > (props.queueOpen ? 0.65 : 0.35));
};

const shouldIgnorePlayerSwipe = (target: EventTarget | null) => {
  if (!(target instanceof Element)) return false;
  return Boolean(
    target.closest(
      ".mobile-lyrics, .lyric-player-wrapper, .amll-lyric-player, .mobile-lyric-offset, .mobile-progress, .mobile-control-buttons, .mobile-volume, .mobile-header-actions, button, .n-button, [role='slider']",
    ),
  );
};

const handleThumbClick = () => {
  if (props.queueOpen) {
    emit("closeQueue");
    return;
  }
  emit("close");
};

const resetPlayerTouch = () => {
  playerTouch.value = null;
};

const handlePlayerTouchStart = (event: TouchEvent) => {
  if (props.queueOpen || shouldIgnorePlayerSwipe(event.target)) return;
  const touch = event.changedTouches?.[0];
  if (!touch) return;
  playerTouch.value = {
    x: touch.clientX,
    y: touch.clientY,
    dragging: false,
    mode: null,
    progress: 0,
  };
};

const handlePlayerTouchMove = (event: TouchEvent) => {
  const start = playerTouch.value;
  const touch = event.changedTouches?.[0];
  if (!start || !touch || props.queueOpen) return;

  const deltaX = touch.clientX - start.x;
  const deltaY = touch.clientY - start.y;
  if (!start.dragging) {
    if (Math.abs(deltaY) < 8) return;
    if (Math.abs(deltaY) < Math.abs(deltaX) * 1.15) {
      resetPlayerTouch();
      return;
    }
    start.dragging = true;
    start.mode = deltaY > 0 ? "close" : "queue";
    if (start.mode === "close") {
      emit("closeDragStart");
    } else {
      start.progress = grabPager();
      suppressCoverClick.value = true;
    }
  }

  if (start.mode === "close") {
    emit("closeDragMove", Math.max(0, deltaY));
    return;
  }

  // queue 模式：跟手平移 pager
  pagerProgress.set(clamp01(start.progress - deltaY / pagerHeight()));
};

const handlePlayerTouchEnd = () => {
  const start = playerTouch.value;
  if (start?.dragging) {
    if (start.mode === "close") emit("closeDragEnd");
    if (start.mode === "queue") {
      settlePagerFromGesture();
      window.setTimeout(() => {
        suppressCoverClick.value = false;
      }, 180);
    }
  }
  resetPlayerTouch();
};

const handlePlayerTouchCancel = () => {
  if (playerTouch.value?.dragging) {
    if (playerTouch.value.mode === "close") emit("closeDragCancel");
    else settlePager(props.queueOpen);
  }
  resetPlayerTouch();
  suppressCoverClick.value = false;
};

const handleCoverClick = () => {
  if (suppressCoverClick.value) {
    suppressCoverClick.value = false;
    return;
  }
  emit("switchLayer");
};

const resetQueueTouch = () => {
  queueTouch.value = null;
};

const handleQueueTouchStart = (event: TouchEvent) => {
  // `Element`, not `HTMLElement`: a touch on a button's icon targets the SVG path, and missing it
  // let a drag on the queue's reorder handle pull the whole page down with it.
  if (event.target instanceof Element && event.target.closest("button")) return;
  const touch = event.changedTouches?.[0];
  if (!touch) return;
  // The scrolling list only: the pinned now-playing card above it pulls the page like the header.
  const inList = event.target instanceof Element && event.target.closest(".player-queue-scroll");
  queueTouch.value = {
    x: touch.clientX,
    y: touch.clientY,
    dragging: false,
    // Only the list scrolls; a pull from the handle or header pages back from anywhere in it.
    scrollTop: inList ? queueScrollTop.value : 0,
    progress: 0,
  };
};

const handleQueueScroll = (event: Event) => {
  if (event.target instanceof HTMLElement) {
    queueScrollTop.value = event.target.scrollTop;
  }
};

const handleQueueTouchMove = (event: TouchEvent) => {
  const start = queueTouch.value;
  const touch = event.changedTouches?.[0];
  if (!start || !touch) return;

  const deltaX = touch.clientX - start.x;
  const deltaY = touch.clientY - start.y;
  if (!start.dragging) {
    if (Math.abs(deltaY) < 8) return;
    const vertical = Math.abs(deltaY) > Math.abs(deltaX) * 1.15;
    if (!vertical || start.scrollTop > 4 || deltaY <= 0) {
      resetQueueTouch();
      return;
    }
    start.dragging = true;
    start.progress = grabPager();
  }

  // 拖拽期间接管手势，阻止列表同时滚动（touchmove 未加 .passive 才能 preventDefault）
  if (event.cancelable) event.preventDefault();
  pagerProgress.set(clamp01(start.progress - deltaY / pagerHeight()));
};

const handleQueueTouchEnd = () => {
  if (queueTouch.value?.dragging) settlePagerFromGesture();
  resetQueueTouch();
};

const handleQueueTouchCancel = () => {
  if (queueTouch.value?.dragging) settlePager(props.queueOpen);
  resetQueueTouch();
};

const scrollCurrentQueueSong = (smooth = false) => queueListRef.value?.scrollToCurrent(smooth);

watch(
  () => props.queueOpen,
  (open) => {
    // 由父级状态（按钮/thumb/整体关闭）驱动的开合走同一 pager 动画；
    // 手势 settle 后 prop 翻转再次触发时，spring 从当前值与速度无缝续接。
    settlePager(open);
    if (open) nextTick(() => scrollCurrentQueueSong());
  },
);

watch(
  () => music.persistData.playSongIndex,
  () => {
    if (props.queueOpen) nextTick(() => scrollCurrentQueueSong(true));
  },
);

useMotionInterruption(() => {
  controlsPointerId = null;
  scheduleLyricControlsHide();
  stopPagerAnimation();
  resetPlayerTouch();
  resetQueueTouch();
  suppressCoverClick.value = false;
  measurePagerHeight();
  pagerProgress.set(props.queueOpen ? 1 : 0);
});

const removeBeforeNavigation = navigation.onBeforeNavigation(() => {
  handlePlayerTouchCancel();
  handleQueueTouchCancel();
});
const removeCancelledNavigation = navigation.onNavigationCancelled(() => {
  settlePager(props.queueOpen);
});

onMounted(() => {
  measurePagerHeight();
  // 只调整遮罩，不缩放歌词视口，避免显隐控件打断正在进行的歌词滚动。
  controlsResizeObserver = new ResizeObserver(([entry]) => {
    lyricControlsHeight.value = entry.contentRect.height;
  });
  if (playbackControlsRef.value) controlsResizeObserver.observe(playbackControlsRef.value);
  window.addEventListener("pointerup", releaseLyricControls, true);
  window.addEventListener("pointercancel", releaseLyricControls, true);
});

onBeforeUnmount(() => {
  clearLyricControlsTimer();
  controlsResizeObserver?.disconnect();
  window.removeEventListener("pointerup", releaseLyricControls, true);
  window.removeEventListener("pointercancel", releaseLyricControls, true);
  removeBeforeNavigation();
  removeCancelledNavigation();
  stopPagerAnimation();
  stopPlayerShift();
  stopQueueOffset();
});

defineExpose({
  phonyBigCoverRef,
  phonySmallCoverRef,
  nameWrapperRef,
  nameTextRef,
  bigSongInfoRef,
  smallSongInfoRef,
});
</script>

<style lang="scss" scoped>
// The app draws edge to edge over the system bar, so the bottom bar must be lifted clear of the
// screen edge, never closer than Apple Music sits: Android's gesture inset (~16-24px) is far
// smaller than the iPhone's 34pt home-indicator area, and "inset + a little" crowded the edge.
// A browser keeps its own toolbar below the page, so the web build needs no lift.
// The whole selector inside `:global()`: Vue drops anything after a `:global(...)` part, which
// would set this on <html>, where `.mobile-pages`' own default overrides it. `html` outranks
// the scoped `.mobile-pages[data-v]` default whatever the source order.
:global(html.tauri-app .mobile-pages) {
  --mobile-controls-bottom-padding: max(calc(var(--app-safe-area-bottom, 0px) + 1rem), 2.75rem);
}

.mobile-pages {
  --mobile-title-opacity: 1;
  --mobile-artists-opacity: 0.7;
  --mobile-time-opacity: 0.5;
  --mobile-controls-bottom-padding: calc(var(--app-safe-area-bottom, 0px) + 0.75rem);

  &.tinted {
    --mobile-title-opacity: 0.85;
    --mobile-artists-opacity: 0.6;
    --mobile-time-opacity: 0.6;
  }
  grid-row: 1 / -1;
  grid-column: 1 / 2;
  position: relative;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.mobile-player-page,
.mobile-queue-layout,
.mobile-player-content {
  position: absolute;
  inset: 0;
  min-width: 0;
  min-height: 0;
}

.mobile-player-page {
  transform-origin: center 22%;
}

// The song name and the artists line, shared with the mini bar like the artwork: the one visible
// copy of each, placed by transform only (x, y, scale from the top-left corner), so it moves
// without layout. The colour is a motion value while it travels and the page's own afterwards. The
// sizes mirror the real text of the active layer; see `titleMetrics` / `artistsMetrics`.
.mobile-title-frame,
.mobile-artists-frame {
  position: absolute;
  left: 0;
  top: 0;
  z-index: 6;
  overflow: hidden;
  white-space: nowrap;
  pointer-events: none;
  transform-origin: 0 0;
  will-change: transform;

  mix-blend-mode: plus-lighter;
}

.mobile-title-frame {
  color: color-mix(
    in srgb,
    var(--main-cover-mix-color, rgb(239, 239, 239)) calc(var(--mobile-title-opacity) * 100%),
    transparent
  );

  .title-name {
    height: var(--text-height);
    font-size: var(--text-size);
    font-weight: 600;
    line-height: var(--text-height);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .title-marquee {
    width: 100%;
    min-width: 0;
    height: 1.25em;
    line-height: 1.25;
    color: inherit;

    :deep(.overflow-marquee__group) {
      align-items: center;
      height: 1.25em;
      line-height: 1.25;
      min-width: max-content;
      white-space: nowrap;
    }
  }

  .title-marquee-content {
    display: inline-block;
    padding-right: 2em;
    white-space: nowrap;
  }
}

// Mirror the real line's opacity in the fallback colour; motion carries it while travelling.
.mobile-artists-frame {
  color: color-mix(
    in srgb,
    var(--main-cover-mix-color, rgb(239, 239, 239)) calc(var(--mobile-artists-opacity) * 100%),
    transparent
  );

  .artists-line {
    height: var(--text-height);
    font-size: var(--text-size);
    line-height: var(--text-height);
    overflow: hidden;
    text-overflow: ellipsis;
  }
}

.mobile-song-info .artists.is-shared {
  visibility: hidden;
}

// .mobile-player-content / .mobile-full-ui 不得挂 transform、will-change 或其他
// stacking context：它们位于 .mobile-lyric-layout 的 plus-lighter 到
// .mobile-player-shell (isolation: isolate) 之间。分页位移在混合元素自己身上。

.mobile-full-ui {
  position: absolute;
  inset: 0;
  display: grid;
  grid-template-rows: [thumb] calc(var(--app-safe-area-top, 0px) + 30px) [main-view] 1fr;
  grid-template-columns: 1fr;
  min-width: 0;
  min-height: 0;
}

// 与主内容同级的分页：共用大播放器背景，无独立底色/毛玻璃。
// 布局本身不平移（overflow 只负责裁切）。位移用 `top` 而不是 transform：动画中的
// transform 会把内容提成合成层，行里的 plus-lighter 就被关进空组，落定才重新加亮。
// 行和图标各自混合，不能像主内容那样把位移挂到每个混合元素上（行底色会跟着关进去）。
.mobile-queue-layout {
  display: grid;
  grid-template-rows: [thumb] calc(var(--app-safe-area-top, 0px) + 30px) [main-view] 1fr;
  grid-template-columns: 1fr;
  pointer-events: none;
  overflow: hidden;

  > .mobile-thumb {
    position: relative;
    top: var(--queue-offset, 0px);
    pointer-events: auto;
  }
}

.mobile-queue-content {
  grid-row: main-view;
  grid-column: 1 / 2;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  color: var(--main-cover-mix-color, rgb(239, 239, 239));
  position: relative;
  top: var(--queue-offset, 0px);
  pointer-events: auto;
  // 混合不挂在这块上：封面是它的子节点，会跟着被加亮。
  // The list blends its own boxes (PlayerQueueList); a blanket svg/path rule here nested a blend
  // inside each of them, an extra offscreen pass per icon per frame for nothing.
  .queue-empty {
    mix-blend-mode: plus-lighter;
  }
}

.mobile-pages.queue-open {
  .mobile-player-page {
    pointer-events: none;
  }
}

// ── AMLL .thumb ──
.mobile-thumb {
  grid-row: thumb;
  justify-self: center;
  align-self: end;
  z-index: 80;
  cursor: pointer;
  width: 60px;
  height: 20px;
  display: flex;
  align-items: center;
  justify-content: center;

  .handle-bar {
    width: 36px;
    height: 5px;
    background: rgba(255, 255, 255, 0.3);
    border-radius: 3px;
    transition: background var(--duration-200) var(--ease-out);
  }

  &:active .handle-bar {
    background: rgba(255, 255, 255, 0.5);
  }
}

// Registered so the lyric mask's bottom edge can transition: it used to jump the instant the
// controls were asked for, while the controls themselves took half a second to fade, so the last
// lines vanished (or reappeared on top of the fading controls) before anything else moved.
@property --mobile-lyric-bottom-inset {
  syntax: "<length>";
  inherits: true;
  initial-value: 0px;
}

// ── AMLL .lyricLayout — Layer 2: 紧凑封面/信息 + 歌词 ──
.mobile-lyric-layout {
  grid-row: main-view;
  grid-column: 1 / 2;
  position: relative;
  z-index: 2;
  display: grid;
  grid-template-rows: 8px [controls] 56px [lyric-view] minmax(0, 1fr);
  grid-template-columns: 16px [cover-side] 56px [info-side] minmax(0, 1fr) 16px;
  mix-blend-mode: plus-lighter;
  // 分页位移在混合元素自己身上。祖先一带 transform，滑动中 plus-lighter 就断了。
  transform: var(--pager-shift, none);
  pointer-events: none;
  // Same duration and curve as the controls (`.mobile-big-controls`), so the lyrics make room as
  // they arrive and give the space back as they leave.
  transition: --mobile-lyric-bottom-inset var(--duration-500) var(--ease-out);

  &.active {
    pointer-events: auto;
  }

  &.controls-visible {
    --mobile-lyric-bottom-inset: calc(
      var(--mobile-lyric-controls-height) + var(--mobile-controls-bottom-padding)
    );
  }
}

// ── AMLL .noLyricLayout — Layer 1: 大封面 + controls ──
.mobile-cover-layout {
  grid-row: main-view;
  grid-column: 1 / 2;
  position: relative;
  z-index: 1;
  // The actual artwork is a sibling; this group contains only slots and controls.
  mix-blend-mode: plus-lighter;
  transform: var(--pager-shift, none);
  overflow-y: hidden;
  display: grid;
  // 独立留出封面与 metadata 的间距，矮视口缩小封面而不挤掉留白。
  grid-template-rows: 1em [cover-view] minmax(0, 1fr) 20px [controls-view] auto;
  grid-template-columns: 24px [main-view] 1fr 24px;
  pointer-events: none;

  &.lyric-layer {
    z-index: 3;

    // Fades out before it is hidden; coming back to layer 1 it is visible at once and fades in.
    .mobile-song-info-row {
      opacity: 0;
      visibility: hidden;
      pointer-events: none;
      transition:
        opacity var(--duration-200) var(--ease-out),
        visibility 0s linear var(--duration-200);
    }
  }
}

// ── AMLL .phonySmallCover ──
.mobile-cover-slot {
  position: relative;
  min-width: 0;
  min-height: 0;
  pointer-events: none;
}

.mobile-phony-small-cover {
  grid-row: controls;
  grid-column: cover-side;
  justify-self: start;
  align-self: center;
  aspect-ratio: 1 / 1;
  width: 56px;
  height: 56px;
  z-index: 5;
}

// ── AMLL .smallControls — 紧凑歌曲信息 ──
.mobile-small-controls {
  grid-row: controls;
  grid-column: info-side;
  align-self: center;
  transition: opacity var(--duration-200) var(--ease-out) 0.25s;
  padding-left: 12px;
  min-width: 0;
  overflow: visible;
  height: fit-content;
  z-index: 3;
  display: flex;
  align-items: center;
  justify-content: space-between;

  .mobile-small-controls-inner {
    width: 100%;
    min-width: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    will-change: transform, opacity;
  }

  .mobile-song-info {
    flex: 1;
    min-width: 0;
    overflow: hidden;

    .name-wrapper {
      overflow: hidden;
      width: 100%;

      .name {
        display: flex;
        position: relative;
        font-weight: 600;
        font-size: 0.95rem;
        color: var(--main-cover-mix-color, rgb(239, 239, 239));
        opacity: var(--mobile-title-opacity);
        mix-blend-mode: plus-lighter;
        margin-bottom: 2px;
        white-space: nowrap;
        overflow: hidden;

        .name-inner {
          flex-shrink: 0;
        }

        &.is-marquee {
          .name-measure {
            position: absolute;
            visibility: hidden;
            pointer-events: none;
          }
        }

        .mobile-name-marquee {
          width: 100%;
          min-width: 0;
          height: 1.25em;
          line-height: 1.25;
          color: inherit;

          :deep(.overflow-marquee__group) {
            align-items: center;
            height: 1.25em;
            line-height: 1.25;
            min-width: max-content;
            white-space: nowrap;
          }
        }

        .mobile-name-marquee-content {
          display: inline-block;
          padding-right: 2em;
          white-space: nowrap;
        }
      }
    }

    .artists {
      font-size: 0.75rem;
      color: var(--main-cover-mix-color, rgb(239, 239, 239));
      opacity: var(--mobile-artists-opacity);
      mix-blend-mode: plus-lighter;
    }
  }

  .mobile-header-actions {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-left: 12px;
    flex-shrink: 0;

    .n-icon {
      color: var(--main-cover-mix-color, rgb(239, 239, 239));
      mix-blend-mode: plus-lighter;
      opacity: 0.8;
      cursor: pointer;

      &:active {
        opacity: 0.5;
      }
    }
  }
}

// ── AMLL .lyric — 歌词区域 ──
// 上探进 header 行（小封面/标题下方），歌词行从其下穿过时靠 mask 渐隐 + AMLL
// 逐行距离模糊构成 Apple 式过渡；否则会在 lyric-view 行顶被硬切出可见边界。
// （RollingLyrics 的 .lyric-am 桌面 mask 在移动端被显式关闭，移动端遮罩由这里负责）
.mobile-lyric {
  grid-row: controls / -1;
  grid-column: 1 / -1;
  transition: opacity var(--duration-500) var(--ease-out) 0.5s;
  opacity: 1;
  min-height: 0;
  position: relative;
  z-index: 1;
  -webkit-mask: linear-gradient(
    180deg,
    transparent 0,
    transparent 40px,
    rgba(0, 0, 0, 0.55) 80px,
    #000 116px,
    #000 calc(100% - var(--mobile-lyric-bottom-inset, 0px) - 48px),
    transparent calc(100% - var(--mobile-lyric-bottom-inset, 0px))
  );
  mask: linear-gradient(
    180deg,
    transparent 0,
    transparent 40px,
    rgba(0, 0, 0, 0.55) 80px,
    #000 116px,
    #000 calc(100% - var(--mobile-lyric-bottom-inset, 0px) - 48px),
    transparent calc(100% - var(--mobile-lyric-bottom-inset, 0px))
  );

  .mobile-lyric-inner {
    position: relative;
    height: 100%;
    min-height: 0;
    will-change: transform, opacity;
  }

  .mobile-lyric-offset {
    position: absolute;
    right: 4px;
    top: 50%;
    transform: translateY(-50%);
  }

  .mobile-lyrics {
    height: 100%;
    overflow-y: auto;
    padding: 0;
    -ms-overflow-style: none;
    scrollbar-width: none;

    &::-webkit-scrollbar {
      display: none;
    }
  }
}

.no-lyrics {
  grid-row: lyric-view;
  grid-column: 1 / -1;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: opacity var(--duration-500) var(--ease-out) 0.5s;
  opacity: 1;

  .mobile-ui-empty {
    will-change: transform, opacity;
  }

  span {
    font-size: 1rem;
    color: var(--main-cover-color);
    opacity: 0.5;
  }
}

// ── AMLL .phonyBigCover ──
.mobile-phony-big-cover {
  grid-row: cover-view;
  grid-column: 2 / 3;
  justify-self: center;
  align-self: center;
  aspect-ratio: 1 / 1;
  width: 100%;
  max-height: 100%;
  z-index: 3;
}

// ── AMLL .bigControls — 完整 controls ──
.mobile-big-controls {
  grid-row: controls-view;
  grid-column: 2 / 3;
  transition:
    opacity var(--duration-500) var(--ease-out),
    transform var(--duration-500) var(--ease-out);
  opacity: 0;
  min-width: 0;
  z-index: 2;
  color: var(--main-cover-mix-color, rgb(239, 239, 239));
  mix-blend-mode: plus-lighter;
  text-shadow: 0 0 0.3em color-mix(in srgb, currentColor 15%, transparent);
  // --app-safe-area-bottom is env(safe-area-inset-bottom) on Tauri mobile,
  // 0px everywhere else — so this is a no-op on desktop / browser.
  padding-bottom: var(--mobile-controls-bottom-padding);

  .mobile-big-controls-inner {
    display: block;
    min-width: 0;
    will-change: transform, opacity;
  }

  // 歌曲信息（展开）
  .mobile-song-info-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 20px;
    transition: opacity var(--duration-200) var(--ease-out);

    .mobile-song-info {
      flex: 1;
      min-width: 0;
      overflow: hidden;

      .name-wrapper {
        overflow: hidden;
        width: 100%;

        .name {
          display: flex;
          position: relative;
          font-weight: 600;
          font-size: 1.2rem;
          color: var(--main-cover-mix-color, rgb(239, 239, 239));
          opacity: var(--mobile-title-opacity);
          mix-blend-mode: plus-lighter;
          margin-bottom: 4px;
          white-space: nowrap;
          overflow: hidden;

          .name-inner {
            flex-shrink: 0;
          }

          &.is-marquee {
            .name-measure {
              position: absolute;
              visibility: hidden;
              pointer-events: none;
            }
          }

          .mobile-name-marquee {
            width: 100%;
            min-width: 0;
            height: 1.25em;
            line-height: 1.25;
            color: inherit;

            :deep(.overflow-marquee__group) {
              align-items: center;
              height: 1.25em;
              line-height: 1.25;
              min-width: max-content;
              white-space: nowrap;
            }
          }

          .mobile-name-marquee-content {
            display: inline-block;
            padding-right: 2em;
            white-space: nowrap;
          }
        }
      }

      .artists {
        font-size: 0.9rem;
        color: var(--main-cover-mix-color, rgb(239, 239, 239));
        opacity: var(--mobile-artists-opacity);
        mix-blend-mode: plus-lighter;
      }
    }

    .mobile-header-actions {
      display: flex;
      align-items: center;
      gap: 16px;
      margin-left: 12px;
      flex-shrink: 0;

      .n-icon {
        color: var(--main-cover-mix-color, rgb(239, 239, 239));
        mix-blend-mode: plus-lighter;
        opacity: 0.8;
        cursor: pointer;

        &:active {
          opacity: 0.5;
        }
      }
    }
  }

  .mobile-progress {
    width: 100%;
    margin-bottom: 16px;

    :deep(.bouncing-slider) {
      mix-blend-mode: plus-lighter;
    }

    // Three columns so the quality badge stays centred whatever width the two times have.
    .time-display {
      display: grid;
      grid-template-columns: 1fr auto 1fr;
      align-items: center;
      gap: 8px;
      margin-top: 8px;
      font-size: max(1.2vh, 0.7rem);
      font-variant-numeric: tabular-nums;
      opacity: var(--mobile-time-opacity);
      color: var(--main-cover-mix-color, rgb(239, 239, 239));
      mix-blend-mode: plus-lighter;

      > :last-child {
        grid-column: 3;
        justify-self: end;
      }
    }

    // Same badge as the desktop cover layout (PlayerCover's `.quality-badge`).
    .quality-badge {
      display: flex;
      align-items: center;
      gap: 4px;
      min-width: 0;
      padding: 2px 8px;
      border-radius: var(--radius-xs);
      background-color: rgba(255, 255, 255, 0.1);
      white-space: nowrap;

      .quality-label {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
      }
    }
  }
}

.mobile-controls-motion {
  display: block;
  will-change: transform, opacity;
}

// Same five columns as the transport row (MobileControls): lyrics, output and queue sit under
// its first, middle and last buttons. Adjacent columns crowded the three together.
.mobile-bottom-bar {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  justify-items: center;
  margin-top: 12px;

  > :first-child {
    grid-column: 1;
  }

  > :last-child {
    grid-column: 5;
  }

  :deep(.output-device-button) {
    grid-column: 3;
  }

  // The output button renders inside n-popselect, out of reach of the scoped attribute.
  button,
  :deep(.output-device-button) {
    appearance: none;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    padding: 0;
    border: none;
    background: transparent;
    color: var(--main-cover-mix-color, rgb(239, 239, 239));
    mix-blend-mode: plus-lighter;
    opacity: 0.6;
    cursor: pointer;
    transition:
      opacity var(--duration-200) var(--ease-out),
      transform var(--duration-150) var(--ease-out);

    &[aria-pressed="true"] {
      opacity: 1;
    }

    &:active {
      transform: scale(0.92);
    }
  }

  // Flat, not nested in the list above: a `:deep()` nested inside `:deep()` compiles to an
  // invalid selector, which would drop the whole rule.
  button :deep(svg),
  :deep(.output-device-button svg) {
    width: 34px;
    height: 34px;
  }
}

.mobile-queue-panel {
  height: 100%;
  box-sizing: border-box;
  display: grid;
  grid-template-rows: minmax(0, 1fr);
  padding: 14px 16px calc(var(--app-safe-area-bottom, 0px) + 16px);
}

.mobile-queue-list {
  min-height: 0;
  overflow-x: clip;
  overscroll-behavior: contain;
  padding: 4px 0 12px;
  // No `contain: paint`: it makes the list an isolated group, and the rows' plus-lighter would
  // blend with its transparent box instead of the background. Shows only with cover colour on.
}

.queue-empty {
  height: 40vh;
  display: flex;
  align-items: center;
  justify-content: center;
  text-align: center;
  opacity: 0.56;
  font-size: 0.95rem;
}

// ═══ 状态切换 ═══
// These are controlled by the parent's .layer2-active class on .bplayer
// The parent sets opacity/pointer-events via its own scoped CSS
// But since these elements are NOW in this child component,
// we need to handle the default state here.
// The parent will use :deep() or we handle via props if needed.

// Default state (Layer 1 visible):
.mobile-small-controls {
  opacity: 0;
  transition: opacity var(--duration-500) var(--ease-out);
  pointer-events: none;
}

.mobile-cover-layout {
  pointer-events: auto;
}

.mobile-lyric,
.no-lyrics {
  opacity: 0;
  transition: opacity var(--duration-500) var(--ease-out);
  pointer-events: none;
}

.mobile-big-controls {
  opacity: 1;
}
</style>
