<template>
  <div ref="pagesRef" :class="['mobile-pages', { 'queue-open': queueOpen }]">
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
            @touchcancel="handlePlayerTouchEnd"
          >
            <div class="handle-bar"></div>
          </Motion>

          <!-- AMLL .lyricLayout — Layer 2: 紧凑封面信息 + 歌词 -->
          <div :class="['mobile-lyric-layout', { active: activeMobileLayer === 2 }]">
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
            <div class="mobile-lyric" v-if="hasLyrics">
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
            <div v-else class="no-lyrics">
              <Motion class="mobile-ui-empty" :style="contentUiMotionStyle">
                <span>¯\_(ツ)_/¯</span>
              </Motion>
            </div>
          </div>

          <!-- AMLL .noLyricLayout — Layer 1: 大封面 + 歌曲信息 + controls -->
          <div class="mobile-cover-layout">
            <div class="mobile-phony-big-cover mobile-cover-slot" ref="phonyBigCoverRef"></div>
            <div class="mobile-big-controls">
              <Motion class="mobile-big-controls-inner" :style="contentUiMotionStyle">
                <!-- 歌曲信息（展开） -->
                <div class="mobile-song-info-row">
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
                <Motion class="mobile-controls-motion" :style="controlsMotionStyle">
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
          @touchcancel="handlePlayerTouchEnd"
        />
      </Motion>
    </div>

    <!-- 移动端待播清单 — 与主内容同级的分页：共用背景，由 pager 平移切换 -->
    <Motion
      class="mobile-queue-layout"
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
          </div>

          <n-virtual-list
            v-if="music.getPlaylists.length"
            ref="queueListRef"
            class="mobile-queue-list"
            :items="queueRows"
            :item-size="73"
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
                  { 'is-current': row.index === music.persistData.playSongIndex },
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
        </div>
      </div>
    </Motion>
  </div>
</template>

<script setup lang="ts">
import {
  DeleteRound,
  MoreVertRound,
  QueueMusicRound,
  StarBorderRound,
  StarRound,
} from "@vicons/material";
import { NVirtualList } from "naive-ui";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { animate, Motion, useMotionValue, useTransform, type MotionValue } from "motion-v";
import { musicStore } from "@/store";
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
}>();

const music = musicStore();

// Expose phony refs and name refs for parent (cover frame composable + name overflow)
const phonyBigCoverRef = ref<HTMLElement | null>(null);
const phonySmallCoverRef = ref<HTMLElement | null>(null);
const nameWrapperRef = ref<HTMLElement | null>(null);
const nameTextRef = ref<HTMLElement | null>(null);
const queueListRef = ref<{
  scrollTo: (options: { index: number; behavior?: ScrollBehavior }) => void;
} | null>(null);
const queueScrollTop = ref(0);
const suppressCoverClick = ref(false);
const playerTouch = ref<{
  x: number;
  y: number;
  dragging: boolean;
  mode: "queue" | "close" | null;
} | null>(null);
const queueTouch = ref<{ x: number; y: number; dragging: boolean; scrollTop: number } | null>(null);

const activeMobileLayer = computed(() => (props.mobileLayer === 2 ? 2 : 1));
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

// 整屏平移的 settle 用近临界阻尼（ζ≈0.98）：sharedLayoutTransition 的 ζ≈0.55
// 在约一屏的行程上会过冲 ~12%，条带两端露出背景。
const pagerSettleTransition = {
  type: "spring",
  stiffness: 420,
  damping: 38,
  mass: 0.9,
  restDelta: 0.001,
  restSpeed: 0.02,
} as const;

const settlePager = (open: boolean) => {
  stopPagerAnimation();
  pagerAnimation = animate(pagerProgress, open ? 1 : 0, pagerSettleTransition);
  if (open !== props.queueOpen) emit(open ? "openQueue" : "closeQueue");
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
      stopPagerAnimation();
      suppressCoverClick.value = true;
    }
  }

  if (start.mode === "close") {
    emit("closeDragMove", Math.max(0, deltaY));
    return;
  }

  // queue 模式：跟手平移 pager
  pagerProgress.set(clamp01(-deltaY / pagerHeight()));
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
    stopPagerAnimation();
  }

  // 拖拽期间接管手势，阻止列表同时滚动（touchmove 未加 .passive 才能 preventDefault）
  if (event.cancelable) event.preventDefault();
  pagerProgress.set(1 - clamp01(deltaY / pagerHeight()));
};

const handleQueueTouchEnd = () => {
  if (queueTouch.value?.dragging) settlePagerFromGesture();
  resetQueueTouch();
};

const handleQueueTouchCancel = () => {
  handleQueueTouchEnd();
};

const formatArtists = (artists: Artist[] = []) =>
  artists
    .filter(Boolean)
    .map((item) => item.name)
    .join(" / ");

const getQueueCover = (item: QueueSong) => {
  const picUrl = item.album?.picUrl;
  return picUrl ? picUrl.replace(/^http:/, "https:") + "?param=96y96" : "/images/pic/default.png";
};

const scrollCurrentQueueSong = () => {
  queueListRef.value?.scrollTo({
    index: music.persistData.playSongIndex,
    behavior: "smooth",
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

onMounted(() => {
  measurePagerHeight();
  window.addEventListener("resize", measurePagerHeight);
});

onBeforeUnmount(() => {
  stopPagerAnimation();
  window.removeEventListener("resize", measurePagerHeight);
});

defineExpose({ phonyBigCoverRef, phonySmallCoverRef, nameWrapperRef, nameTextRef });
</script>

<style lang="scss" scoped>
.mobile-pages {
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
    transition: background 0.2s ease;
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
}

// ── AMLL .noLyricLayout — Layer 1: 大封面 + controls ──
.mobile-cover-layout {
  grid-row: main-view;
  grid-column: 1 / 2;
  position: relative;
  z-index: 1;
  overflow-y: hidden;
  display: grid;
  grid-template-rows: 1em [cover-view] 1fr [controls-view] 0fr;
  grid-template-columns: 24px [main-view] 1fr 24px;
  pointer-events: none;
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
  transition: opacity 0.25s 0.25s;
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
  transition: opacity 0.5s 0.5s;
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
    #000 calc(100% - 48px),
    transparent 100%
  );
  mask: linear-gradient(
    180deg,
    transparent 0,
    transparent 40px,
    rgba(0, 0, 0, 0.55) 80px,
    #000 116px,
    #000 calc(100% - 48px),
    transparent 100%
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
  transition: opacity 0.5s 0.5s;
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
  transition: opacity 0.5s;
  opacity: 0;
  min-width: 0;
  z-index: 2;
  text-shadow: 0 0 0.3em color-mix(in srgb, currentColor 15%, transparent);
  // --app-safe-area-bottom is env(safe-area-inset-bottom) on Tauri mobile,
  // 0px everywhere else — so this is a no-op on desktop / browser.
  padding-bottom: calc(var(--app-safe-area-bottom, 0px) + 4rem);

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
  grid-template-rows: auto minmax(0, 1fr);
  padding: 14px 16px calc(var(--app-safe-area-bottom, 0px) + 16px);
}

.mobile-queue-header {
  display: flex;
  align-items: center;
  min-width: 0;
  margin-bottom: 8px;
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

.queue-song {
  min-height: 64px;
  display: grid;
  grid-template-columns: 30px 48px minmax(0, 1fr) auto 36px;
  align-items: center;
  gap: 10px;
  border-radius: 8px;
  padding: 8px 8px 8px 4px;
  margin-bottom: 8px;
  box-sizing: border-box;
  background: rgba(255, 255, 255, 0.075);
  border: 1px solid rgba(255, 255, 255, 0.08);

  &:active {
    transform: scale(0.985);
  }

  &.is-current {
    background: color-mix(in srgb, var(--main-cover-color) 18%, transparent);
    border-color: color-mix(in srgb, var(--main-cover-color) 42%, transparent);
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
  width: 48px;
  height: 48px;
  border-radius: 7px;
  object-fit: cover;
  box-shadow: 0 8px 18px rgba(0, 0, 0, 0.24);
}

.queue-info {
  min-width: 0;

  .queue-name {
    font-weight: 650;
    font-size: 0.95rem;
    line-height: 1.2;
  }

  .queue-artists {
    margin-top: 5px;
    font-size: 0.78rem;
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
  width: 36px;
  height: 36px;
  border-radius: 50%;
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0.56;
  cursor: pointer;

  &:active {
    opacity: 1;
    background: rgba(255, 255, 255, 0.12);
    transform: scale(0.94);
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

@media (max-width: 380px) {
  .queue-song {
    grid-template-columns: 26px 44px minmax(0, 1fr) 34px;
    gap: 8px;
  }

  .queue-cover {
    width: 44px;
    height: 44px;
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
  transition: opacity 0.5s;
  pointer-events: none;
}

.mobile-cover-layout {
  pointer-events: auto;
}

.mobile-lyric,
.no-lyrics {
  opacity: 0;
  transition: opacity 0.5s;
  pointer-events: none;
}

.mobile-big-controls {
  opacity: 1;
}
</style>
