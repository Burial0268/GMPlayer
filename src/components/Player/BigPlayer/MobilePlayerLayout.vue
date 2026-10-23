<template>
  <div
    ref="pagesRef"
    :class="['mobile-pages', { 'queue-open': queueOpen }]"
    :style="{ '--mobile-lyric-controls-height': `${lyricControlsHeight}px` }"
  >
    <div class="mobile-player-page">
      <Motion class="mobile-player-content" :style="playerPageMotionStyle">
        <div class="mobile-full-ui">
          <!-- AMLL .thumb — 抽屉把手 -->
          <Motion
            class="mobile-thumb"
            :style="contentUiMotionStyle"
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
              <Motion class="mobile-small-controls-inner" :style="contentUiMotionStyle">
                <div class="mobile-song-info">
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
                  <div class="artists text-hidden" v-if="artistList.length">
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
                  <n-icon size="24" :component="QueueMusicRound" @click.stop="$emit('openQueue')" />
                  <n-icon size="24" :component="MoreVertRound" @click.stop="" />
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
              <Motion class="mobile-big-controls-inner" :style="contentUiMotionStyle">
                <!-- 歌曲信息（展开） -->
                <div
                  class="mobile-song-info-row"
                  :inert="activeMobileLayer === 2"
                  :aria-hidden="activeMobileLayer === 2"
                >
                  <div class="mobile-song-info">
                    <div class="name-wrapper" ref="nameWrapperRef">
                      <div class="name" ref="nameTextRef" :class="{ 'is-marquee': isNameOverflow }">
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
                    <div class="artists text-hidden" v-if="artistList.length">
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
                    <n-icon
                      size="24"
                      :component="QueueMusicRound"
                      @click.stop="$emit('openQueue')"
                    />
                    <n-icon size="24" :component="MoreVertRound" @click.stop="" />
                  </div>
                </div>
                <Motion as-child :style="controlsMotionStyle">
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
                        <span>-{{ remainingTime }}</span>
                      </div>
                    </div>
                    <!-- 控制按钮 + 音量 -->
                    <MobileControls @toComment="$emit('toComment')" />
                  </div>
                </Motion>
              </Motion>
            </div>
          </div>
        </div>

        <!-- Single visible album layer. The cover slots above are measurement anchors only. -->
        <MobileCoverFrame
          :visible="albumLayerVisible"
          :motionStyle="albumLayerStyle"
          :coverUrl="coverImageUrl500"
          :layoutTransition="layoutTransition"
          :layoutDependency="mobileLayer"
          :layoutEnabled="false"
          :layoutId="null"
          :borderRadius="12"
          interactive
          @click="handleCoverClick"
          @touchstart.passive="handlePlayerTouchStart"
          @touchmove.passive="handlePlayerTouchMove"
          @touchend.passive="handlePlayerTouchEnd"
          @touchcancel="handlePlayerTouchCancel"
        />
      </Motion>
    </div>

    <!-- 移动端待播清单 — 与主内容同级的分页：共用背景，由 pager 平移切换 -->
    <Motion
      class="mobile-queue-layout"
      data-navigation-layer="queue-player"
      :style="queuePageMotionStyle"
      @touchstart.passive="handleQueueTouchStart"
      @touchmove="handleQueueTouchMove"
      @touchend.passive="handleQueueTouchEnd"
      @touchcancel="handleQueueTouchCancel"
    >
      <div class="mobile-queue-content">
        <div class="mobile-queue-panel">
          <div class="mobile-queue-header">
            <div class="queue-title">
              <n-icon size="22" :component="QueueMusicRound" />
              <div class="queue-title-text">
                <span class="title">{{ $t("general.name.playlists") }}</span>
                <span class="count" v-if="music.getPlaylists.length">
                  {{ $t("general.name.songSize", { size: music.getPlaylists.length }) }}
                </span>
              </div>
            </div>
            <button
              v-if="music.getPlaylists.length"
              class="queue-clear"
              type="button"
              @click="music.clearPlaylists()"
            >
              {{ $t("player.queue.clear") }}
            </button>
          </div>

          <n-virtual-list
            v-if="music.getPlaylists.length"
            ref="queueListRef"
            class="mobile-queue-list"
            :items="queueRows"
            :item-size="56"
            :item-resizable="true"
            key-field="key"
            :show-scrollbar="false"
            @scroll="handleQueueScroll"
          >
            <template #default="{ item: row }">
              <div
                :id="`mobile-queue-${row.index}`"
                :class="[
                  'queue-song',
                  {
                    'is-current': row.index === music.persistData.playSongIndex,
                    'row-odd': row.index % 2 === 0,
                    'row-even': row.index % 2 === 1,
                    'row-first': row.index === 0,
                    'row-last': row.index === music.getPlaylists.length - 1,
                  },
                ]"
                role="button"
                tabindex="0"
                @click="changeQueueIndex(row.index)"
                @keydown.enter.prevent="changeQueueIndex(row.index)"
              >
                <div class="queue-index">
                  <span v-if="row.index !== music.persistData.playSongIndex">
                    {{ row.index + 1 }}
                  </span>
                  <div v-else class="playing-bars">
                    <span class="line"></span>
                    <span class="line"></span>
                    <span class="line"></span>
                  </div>
                </div>
                <img class="queue-cover" :src="getQueueCover(row.item)" alt="cover" />
                <div class="queue-info">
                  <div class="queue-name text-hidden">{{ row.item.name }}</div>
                  <div class="queue-artists text-hidden">{{ formatArtists(row.item.artist) }}</div>
                </div>
                <div class="queue-duration" v-if="row.item.time">{{ row.item.time }}</div>
                <button
                  class="queue-remove"
                  type="button"
                  @click.stop="music.removeSong(row.index)"
                >
                  <n-icon size="20" :component="DeleteRound" />
                </button>
              </div>
            </template>
          </n-virtual-list>
          <div class="queue-empty" v-else>
            {{ $t("other.playlistEmpty") }}
          </div>

          <!-- 与桌面大播放器同一个底部整宽定位按钮，网格第三行。 -->
          <button
            v-if="music.getPlaylists.length"
            class="queue-locate"
            type="button"
            @click="scrollCurrentQueueSong"
          >
            <n-icon size="18" :component="MyLocationRound" />
            <span>{{ $t("player.queue.locate") }}</span>
          </button>
        </div>
      </div>
    </Motion>
  </div>
</template>

<script setup lang="ts">
import {
  DeleteRound,
  MoreVertRound,
  MyLocationRound,
  QueueMusicRound,
  StarBorderRound,
  StarRound,
} from "@vicons/material";
import { NVirtualList } from "naive-ui";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { animate, Motion, useMotionValue, useTransform, type MotionValue } from "motion-v";
import { musicStore } from "@/store";
import { coverUrl } from "@/utils/coverUrl";
import { prefersReducedMotion } from "@/utils/reducedMotion";
import { locateVirtualRow, type LocateHandle, type VirtualListInst } from "@/utils/locateRow";
import { useLayerNavigation } from "@/utils/navigation";
import { layerMotion } from "@/utils/navigation/motion";
import { useMotionInterruption } from "@/composables/useMotionInterruption";
import RollingLyrics from "../RollingLyrics.vue";
import BouncingSlider from "../BouncingSlider.vue";
import MobileControls from "./MobileControls.vue";
import MobileCoverFrame from "./MobileCoverFrame.vue";
import LyricOffsetControl from "./LyricOffsetControl.vue";
import OverflowMarquee from "@/components/Common/OverflowMarquee.vue";

declare const $player: any;

type Artist = { name: string };
type QueueSong = {
  id: number;
  name: string;
  artist?: Artist[];
  album?: { picUrl?: string };
  time?: string;
};
type StaticStyleRecord = Record<string, string | number | undefined>;
type MotionStyleRecord = Record<string, string | number | MotionValue | undefined>;

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
  controlsMotionStyle: MotionStyleRecord;
  albumLayerStyle: MotionStyleRecord;
  albumLayerVisible: boolean;
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
const queueListRef = ref<VirtualListInst | null>(null);
let locateHandle: LocateHandle | null = null;
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

// ── 队列分页：主内容与 Playlist 同级、共用大播放器背景，作为连续条带垂直平移 ──
// 0 = 主内容，1 = Playlist。y 用像素数值而非百分比字符串：progress 归零时
// motion 才会输出 transform: none，主内容不残留 stacking context，
// 保住歌词层 plus-lighter 到背景画布的混合链。
const pagesRef = ref<HTMLElement | null>(null);
const pagerProgress = useMotionValue(props.queueOpen ? 1 : 0);
const pagerHeightValue = useMotionValue(0);
let pagerAnimation: ReturnType<typeof animate> | null = null;

const clamp01 = (value: number) => Math.min(1, Math.max(0, value));
const pagerHeight = () => pagerHeightValue.get() || window.innerHeight || 1;
const measurePagerHeight = () => {
  pagerHeightValue.set(pagesRef.value?.clientHeight || window.innerHeight || 1);
};

const playerPageY = useTransform(() => -clamp01(pagerProgress.get()) * pagerHeight());
const queuePageY = useTransform(() => (1 - clamp01(pagerProgress.get())) * pagerHeight());
const playerPageMotionStyle = computed<MotionStyleRecord>(() => ({ y: playerPageY }));
const queuePageMotionStyle = computed<MotionStyleRecord>(() => ({
  ...props.contentShellStyle,
  y: queuePageY,
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

const queueRows = computed(() =>
  music.getPlaylists.map((item: QueueSong, index: number) => ({
    item,
    index,
    key: `${item.id}-${index}`,
  })),
);

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
  if (event.target instanceof HTMLElement && event.target.closest("button")) return;
  const touch = event.changedTouches?.[0];
  if (!touch) return;
  queueTouch.value = {
    x: touch.clientX,
    y: touch.clientY,
    dragging: false,
    scrollTop: queueScrollTop.value,
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

const formatArtists = (artists: Artist[] = []) =>
  artists
    .filter(Boolean)
    .map((item) => item.name)
    .join(" / ");

const getQueueCover = (item: QueueSong) => coverUrl(item.album?.picUrl, 96);

const scrollCurrentQueueSong = () => {
  const index = music.persistData.playSongIndex;
  if (index < 0 || index >= music.getPlaylists.length) return;
  locateHandle?.cancel();
  locateHandle = locateVirtualRow(queueListRef.value, index, {
    rowSelector: `#mobile-queue-${index}`,
    align: "start",
    instant: prefersReducedMotion(),
  });
};

const changeQueueIndex = (index: number) => {
  music.selectPlaySongByIndex(index);
};

watch(
  () => props.queueOpen,
  (open) => {
    // 由父级状态（按钮/thumb/整体关闭）驱动的开合走同一 pager 动画；
    // 手势 settle 后 prop 翻转再次触发时，spring 从当前值与速度无缝续接。
    settlePager(open);
    if (open) nextTick(scrollCurrentQueueSong);
  },
);

watch(
  () => music.persistData.playSongIndex,
  () => {
    if (props.queueOpen) nextTick(scrollCurrentQueueSong);
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
  locateHandle?.cancel();
});

defineExpose({ phonyBigCoverRef, phonySmallCoverRef, nameWrapperRef, nameTextRef });
</script>

<style lang="scss" scoped>
.mobile-pages {
  --mobile-controls-bottom-padding: calc(var(--app-safe-area-bottom, 0px) + 4rem);
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

// .mobile-player-content / .mobile-full-ui 不得挂常驻 will-change（或其他产生
// stacking context 的属性）：它们位于 .mobile-lyric-layout 的 plus-lighter 到
// .mobile-player-shell (isolation: isolate) 之间，一旦成为 stacking context，
// 歌词混合就会被隔离、混不到背景画布上（取色模式下表现为歌词失去加亮）。
// 队列推入动画的 will-change 由 motion-v 在动画期间自行添加/移除。

.mobile-full-ui {
  position: absolute;
  inset: 0;
  display: grid;
  grid-template-rows: [thumb] calc(var(--app-safe-area-top, 0px) + 30px) [main-view] 1fr;
  grid-template-columns: 1fr;
  min-width: 0;
  min-height: 0;
}

// 与主内容同级的分页：共用大播放器背景，无独立底色/毛玻璃，由 pager 平移入场。
// 平移用 y（像素）驱动，静止在两端时 transform 归零，不残留 stacking context。
.mobile-queue-layout {
  display: grid;
  grid-template-rows: [thumb] calc(var(--app-safe-area-top, 0px) + 30px) [main-view] 1fr;
  grid-template-columns: 1fr;
  color: var(--main-cover-color);
  pointer-events: none;
  overflow: hidden;
}

.mobile-queue-content {
  grid-row: main-view;
  grid-column: 1 / 2;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.mobile-pages.queue-open {
  .mobile-player-page {
    pointer-events: none;
  }

  .mobile-queue-layout {
    pointer-events: auto;
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
  pointer-events: none;

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
  overflow-y: hidden;
  display: grid;
  // 独立留出封面与 metadata 的间距，矮视口缩小封面而不挤掉留白。
  grid-template-rows: 1em [cover-view] minmax(0, 1fr) 20px [controls-view] auto;
  grid-template-columns: 24px [main-view] 1fr 24px;
  pointer-events: none;

  &.lyric-layer {
    z-index: 3;

    .mobile-song-info-row {
      visibility: hidden;
      pointer-events: none;
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
        color: var(--main-cover-color);
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
      opacity: 0.7;
      color: var(--main-cover-color);
    }
  }

  .mobile-header-actions {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-left: 12px;
    flex-shrink: 0;

    .n-icon {
      color: var(--main-cover-color);
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
  transition: opacity var(--duration-500) var(--ease-out);
  opacity: 0;
  min-width: 0;
  z-index: 2;
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
          color: var(--main-cover-color);
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
        opacity: 0.7;
        color: var(--main-cover-color);
      }
    }

    .mobile-header-actions {
      display: flex;
      align-items: center;
      gap: 16px;
      margin-left: 12px;
      flex-shrink: 0;

      .n-icon {
        color: var(--main-cover-color);
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

    .time-display {
      display: flex;
      justify-content: space-between;
      margin-top: 8px;
      font-size: max(1.2vh, 0.7rem);
      opacity: 0.5;
      color: var(--main-cover-color);
    }
  }
}

.mobile-controls-motion {
  display: block;
  will-change: transform, opacity;
}

.mobile-queue-panel {
  height: 100%;
  box-sizing: border-box;
  display: grid;
  grid-template-rows: auto minmax(0, 1fr) auto;
  padding: 14px 16px calc(var(--app-safe-area-bottom, 0px) + 16px);
}

.mobile-queue-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  min-width: 0;
  margin-bottom: 8px;
}

.queue-clear {
  flex: 0 0 auto;
  padding: 6px 10px;
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: var(--radius-sm);
  color: inherit;
  background: rgba(255, 255, 255, 0.06);
  font: inherit;
  font-size: 0.76rem;
  cursor: pointer;
  opacity: 0.72;
  transition:
    opacity 0.16s ease,
    background-color 0.16s ease,
    border-color 0.16s ease;

  &:active,
  &:focus-visible {
    opacity: 1;
    background: rgba(255, 255, 255, 0.12);
    border-color: rgba(255, 255, 255, 0.2);
    outline: none;
  }
}

.queue-title {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 10px;

  > .n-icon {
    flex-shrink: 0;
    opacity: 0.9;
  }
}

.queue-title-text {
  min-width: 0;
  display: flex;
  flex-direction: column;

  .title {
    font-size: 1.05rem;
    font-weight: 700;
    line-height: 1.2;
  }

  .count {
    margin-top: 2px;
    font-size: 0.78rem;
    opacity: 0.62;
  }
}

.mobile-queue-list {
  min-height: 0;
  overflow-x: clip;
  overscroll-behavior: contain;
  padding: 4px 0 12px;
  contain: layout paint style;

  :deep(.v-vl) {
    overflow-x: hidden !important;
    scrollbar-width: none;
  }

  :deep(.v-vl::-webkit-scrollbar) {
    width: 0;
    height: 0;
  }
}

// 与 QueuePanel / DesktopQueuePanel 一致的斑马纹连续列表（封面自适应色）
.queue-song {
  min-height: 56px;
  display: grid;
  grid-template-columns: 26px 42px minmax(0, 1fr) auto 32px;
  align-items: center;
  gap: 10px;
  border-radius: 0;
  padding: 7px 8px;
  box-sizing: border-box;
  transition: background-color var(--duration-150) var(--ease-out);

  &.row-first {
    border-radius: var(--radius-md) var(--radius-md) 0 0;
  }

  &.row-last {
    border-radius: 0 0 var(--radius-md) var(--radius-md);
  }

  &.row-odd {
    background: color-mix(in srgb, var(--main-cover-color) 5%, transparent);
  }

  &.row-even {
    background: color-mix(in srgb, var(--main-cover-color) 9%, transparent);
  }

  &:active {
    background: color-mix(in srgb, var(--main-cover-color) 15%, transparent);
  }

  &.is-current {
    background: color-mix(in srgb, var(--main-cover-color) 22%, transparent);
  }
}

.queue-index {
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 0.78rem;
  opacity: 0.66;
  font-variant-numeric: tabular-nums;
}

.playing-bars {
  height: 18px;
  width: 18px;
  display: flex;
  align-items: flex-end;
  justify-content: center;
  gap: 3px;

  .line {
    width: 3px;
    min-height: 7px;
    border-radius: 3px;
    background: var(--main-cover-color);
    animation: queue-line-move 0.9s ease-in-out infinite;

    &:nth-child(2) {
      animation-delay: 0.12s;
    }

    &:nth-child(3) {
      animation-delay: 0.24s;
    }
  }
}

.queue-cover {
  width: 42px;
  height: 42px;
  border-radius: var(--radius-sm);
  object-fit: cover;
}

.queue-info {
  min-width: 0;

  .queue-name {
    font-weight: 650;
    font-size: 0.88rem;
    line-height: 1.25;
  }

  .queue-artists {
    margin-top: 2px;
    font-size: 0.75rem;
    opacity: 0.62;
  }
}

.queue-duration {
  font-size: 0.76rem;
  opacity: 0.54;
  font-variant-numeric: tabular-nums;
}

.queue-remove {
  appearance: none;
  border: none;
  background: transparent;
  color: var(--main-cover-color);
  width: 32px;
  height: 32px;
  border-radius: var(--radius-sm);
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0.56;
  cursor: pointer;

  &:active {
    opacity: 1;
    background: color-mix(in srgb, var(--main-cover-color) 14%, transparent);
  }
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

// 与 DesktopQueuePanel 的底部定位按钮同一套：叠在封面背景上，配色随 --main-cover-color。
.queue-locate {
  margin-top: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  min-height: var(--control-touch-size, 44px);
  padding: 0 16px;
  border: 1px solid color-mix(in srgb, var(--main-cover-color) 20%, transparent);
  border-radius: var(--radius-md);
  color: var(--main-cover-color);
  background: color-mix(in srgb, var(--main-cover-color) 12%, transparent);
  font: inherit;
  font-size: 0.88rem;
  font-weight: 640;
  cursor: pointer;
  transition:
    background-color 0.16s ease,
    transform 0.16s ease;

  .n-icon {
    color: var(--main-cover-color);
  }

  &:active {
    background: color-mix(in srgb, var(--main-cover-color) 20%, transparent);
    transform: scale(0.985);
  }
}

@media (max-width: 380px) {
  .queue-song {
    grid-template-columns: 24px 40px minmax(0, 1fr) 32px;
    gap: 8px;
  }

  .queue-cover {
    width: 40px;
    height: 40px;
  }

  .queue-duration {
    display: none;
  }
}

@keyframes queue-line-move {
  0%,
  100% {
    height: 8px;
  }

  50% {
    height: 18px;
  }
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
