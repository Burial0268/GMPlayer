<template>
  <div :class="['all', { noLrc: !showLyrics, immersive: isImmersive }]">
    <div class="tip" ref="tipRef" v-show="lrcMouseStatus">
      <n-text>{{ $t("other.lrcClicks") }}</n-text>
    </div>

    <ImmersivePlayerLayout
      v-if="isImmersive"
      :menuShow="menuShow"
      :showLyrics="showLyrics"
      :commentsOpen="commentsOpen"
      :handleProgressSeek="handleProgressSeek"
      @lrcMouseEnter="$emit('lrcMouseEnter')"
      @lrcAllLeave="$emit('lrcAllLeave')"
      @lrcTextClick="$emit('lrcTextClick', $event)"
      @openComments="$emit('openComments')"
      @closeComments="$emit('closeComments')"
    />

    <template v-else>
      <div ref="leftContentRef" class="left">
        <LayoutGroup id="desktop-player-content">
          <AnimatePresence :initial="false">
            <Motion
              v-if="commentsOpen"
              key="comments"
              class="left-stage comment-stage"
              :layout="true"
              :initial="{ opacity: 0, x: -22, y: 8, scale: 0.99 }"
              :animate="{ opacity: 1, x: 0, y: 0, scale: 1 }"
              :exit="{ opacity: 0, x: -14, y: 5, scale: 0.995 }"
              :transition="commentStageTransition"
            >
              <DesktopCommentPanel @close="$emit('closeComments')" />
            </Motion>
            <Motion
              v-else
              key="artwork"
              class="left-stage artwork-stage"
              :layout="true"
              :initial="{ opacity: 0, x: 22, y: 8, scale: 0.99 }"
              :animate="{ opacity: 1, x: 0, y: 0, scale: 1 }"
              :exit="{ opacity: 0, x: 14, y: 5, scale: 0.995 }"
              :transition="artworkStageTransition"
            >
              <PlayerCover
                v-if="setting.playerStyle === 'cover'"
                @openComments="$emit('openComments')"
              />
              <PlayerRecord
                v-else-if="setting.playerStyle === 'record'"
                @openComments="$emit('openComments')"
              />
            </Motion>
          </AnimatePresence>
        </LayoutGroup>
        <PlayerCloseHandle />
      </div>

      <div
        ref="rightContentRef"
        :class="['right', { 'lyrics-hidden': !showLyrics }]"
        :aria-hidden="!showLyrics"
      >
        <DesktopLyricsPanel
          :menuShow="menuShow"
          :handleProgressSeek="handleProgressSeek"
          @lrcMouseEnter="$emit('lrcMouseEnter')"
          @lrcAllLeave="$emit('lrcAllLeave')"
          @lrcTextClick="$emit('lrcTextClick', $event)"
        />
      </div>
    </template>

    <DesktopQueuePanel :show="queueOpen" />
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { musicStore, settingStore } from "@/store";
import { AnimatePresence, LayoutGroup, Motion } from "motion-v";
import PlayerCover from "../PlayerCover.vue";
import PlayerRecord from "../PlayerRecord.vue";
import DesktopLyricsPanel from "./DesktopLyricsPanel.vue";
import DesktopQueuePanel from "./DesktopQueuePanel.vue";
import DesktopCommentPanel from "./DesktopCommentPanel.vue";
import ImmersivePlayerLayout from "./ImmersivePlayerLayout.vue";
import PlayerCloseHandle from "../PlayerCloseHandle.vue";

const props = defineProps<{
  lrcMouseStatus: boolean;
  menuShow: boolean;
  hasLyrics: boolean;
  lyricsVisible: boolean;
  queueOpen: boolean;
  commentsOpen: boolean;
  handleProgressSeek: (val: number) => void;
}>();

defineEmits<{
  lrcMouseEnter: [];
  lrcAllLeave: [];
  lrcTextClick: [time: number];
  openComments: [];
  closeComments: [];
}>();

const music = musicStore();
const setting = settingStore();

const tipRef = ref<HTMLElement | null>(null);
const leftContentRef = ref<HTMLElement | null>(null);
const rightContentRef = ref<HTMLElement | null>(null);
const lyricsReady = computed(() => props.hasLyrics && !music.getLoadingState);
const showLyrics = computed(() => lyricsReady.value && props.lyricsVisible);
// 沉浸模式接管整块桌面布局（全幅封面 + 右侧歌词 + 浮动控制卡），
// 因此不渲染 .left / .right 两栏，也不需要下面的关闭手柄测量。
const isImmersive = computed(() => setting.playerStyle === "immersive");
// This is deliberately slower and more damped than the mobile artwork spring.
// The desktop hand-off travels farther, so the lower stiffness and heavier mass
// keep it from snapping into place while still preserving a spring settle.
const stageSpring = {
  type: "spring",
  stiffness: 180,
  damping: 42,
  mass: 1.35,
  restDelta: 0.001,
  restSpeed: 0.01,
} as const;
const stageOpacity = {
  type: "tween",
  duration: 0.62,
  ease: [0.16, 1, 0.3, 1],
} as const;
const commentStageTransition = {
  ...stageSpring,
  default: stageSpring,
  layout: stageSpring,
  opacity: { ...stageOpacity, delay: 0.1 },
} as const;
const artworkStageTransition = {
  ...stageSpring,
  default: stageSpring,
  layout: stageSpring,
  opacity: { ...stageOpacity, delay: 0.04 },
} as const;

// 关闭手柄跟随封面/唱片舞台：测量舞台在 .left 内的位置，取「舞台顶部上方 1.5rem」
// 那一点作为手柄中心（复刻封面/唱片视图里 bottom: calc(100% + 1.5rem) 的定位），
// 写入 --close-handle-x / --close-handle-y，从而切到评论面板手柄纹丝不动、全屏也精确对齐。
const setCloseHandleVars = (left: HTMLElement, anchor: DOMRect, leftRect: DOMRect) => {
  const x = anchor.left + anchor.width / 2 - leftRect.left;
  const y = anchor.top - leftRect.top - 24;
  left.style.setProperty("--close-handle-x", `${Math.round(x)}px`);
  left.style.setProperty("--close-handle-y", `${Math.round(y)}px`);
};

const applyCloseHandlePosition = () => {
  const left = leftContentRef.value;
  if (!left) return;
  const leftRect = left.getBoundingClientRect();
  // 优先：封面/唱片舞台顶部上方 1.5rem（复刻 --stage 定位）
  const stage = left.querySelector<HTMLElement>(".cover-stage, .record-stage");
  const stageRect = stage?.getBoundingClientRect();
  if (stageRect?.width && stageRect?.height) {
    setCloseHandleVars(left, stageRect, leftRect);
    return;
  }
  // 回退：评论视图没有舞台时，用评论容器自身几何计算（同样取其顶部上方 1.5rem、
  // 水平居中），这样评论面板打开期间缩放窗口手柄也随之更新，而不是冻在旧值。
  const panel = left.querySelector<HTMLElement>(".desktop-comment-panel");
  const panelRect = panel?.getBoundingClientRect();
  if (panelRect?.width && panelRect?.height) {
    setCloseHandleVars(left, panelRect, leftRect);
  }
};

let measureFrame: number | null = null;
const scheduleMeasure = () => {
  if (measureFrame !== null) cancelAnimationFrame(measureFrame);
  measureFrame = requestAnimationFrame(() => {
    measureFrame = null;
    applyCloseHandlePosition();
  });
};

let leftResizeObserver: ResizeObserver | null = null;
let settleTimer: number | null = null;

// .left 在切换播放器样式时会整块卸载重建，观察器必须跟着换目标，
// 否则从沉浸模式切回封面/唱片后手柄会冻在旧坐标。
const attachLeftObserver = () => {
  leftResizeObserver?.disconnect();
  leftResizeObserver = null;
  const left = leftContentRef.value;
  if (!left || typeof ResizeObserver === "undefined") return;
  leftResizeObserver = new ResizeObserver(() => scheduleMeasure());
  leftResizeObserver.observe(left);
};

const clearSettleTimer = () => {
  if (settleTimer !== null) {
    window.clearTimeout(settleTimer);
    settleTimer = null;
  }
};

onMounted(() => {
  scheduleMeasure();
  attachLeftObserver();
});

onBeforeUnmount(() => {
  if (measureFrame !== null) cancelAnimationFrame(measureFrame);
  clearSettleTimer();
  leftResizeObserver?.disconnect();
  leftResizeObserver = null;
});

// 返回封面/唱片视图时，等入场弹簧落定后重测（覆盖评论期间发生的窗口缩放）。
// 快速来回切换会不断重排这一次重测，而不是堆积多个定时器。
watch(
  () => props.commentsOpen,
  (open) => {
    clearSettleTimer();
    if (open) return;
    settleTimer = window.setTimeout(() => {
      settleTimer = null;
      scheduleMeasure();
    }, 750);
  },
);
watch(
  () => setting.playerStyle,
  () =>
    nextTick(() => {
      attachLeftObserver();
      scheduleMeasure();
    }),
);
watch(
  () => music.getPlaySongData,
  () => nextTick(scheduleMeasure),
);

defineExpose({ tipRef, leftContentRef, rightContentRef });
</script>

<style lang="scss" scoped>
.all {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: row;
  align-items: center;
  position: relative;

  // 沉浸模式内部全部绝对定位，两栏 flex 规则不参与布局
  &.immersive {
    display: block;
  }

  &.noLrc {
    justify-content: flex-start;

    .left {
      flex-basis: 100%;
      width: 100%;
      transform: translateX(0) scale(1);
    }

    .artwork-stage {
      padding-left: 0;
    }

    .comment-stage {
      padding-right: clamp(24px, 3vw, 48px);
    }
  }

  .tip {
    position: absolute;
    top: 24px;
    left: calc(50% - 150px);
    width: 300px;
    height: 40px;
    border-radius: 25px;
    background-color: #ffffff20;
    -webkit-backdrop-filter: blur(20px);
    backdrop-filter: blur(20px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 4;
    will-change: transform, opacity;

    span {
      color: #ffffffc7;
    }
  }

  .left {
    flex: 0 0 40%;
    width: 40%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    position: relative;
    box-sizing: border-box;
    transition:
      flex-basis 0.34s cubic-bezier(0.25, 1, 0.5, 1),
      width 0.34s cubic-bezier(0.25, 1, 0.5, 1),
      transform 0.34s cubic-bezier(0.25, 1, 0.5, 1),
      opacity 0.24s ease;
    // width 走布局，合成器接不了；留在 will-change 里只是白占一个常驻图层。
    will-change: transform, opacity;
  }

  .left-stage {
    position: absolute;
    inset: 0;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    will-change: transform, opacity;
  }

  .artwork-stage {
    align-items: center;
    justify-content: center;
    padding-left: 2rem;
  }

  .comment-stage {
    align-items: stretch;
    justify-content: stretch;
    padding: clamp(82px, 10vh, 112px) clamp(8px, 1vw, 18px) clamp(84px, 11vh, 112px)
      clamp(24px, 3vw, 48px);
  }

  .right {
    position: absolute;
    inset: 0 0 0 auto;
    width: 60%;
    min-width: 0;
    height: 100%;
    z-index: 1;
    box-sizing: border-box;
    mix-blend-mode: plus-lighter;
    padding-right: 1rem;
    opacity: 1;
    overflow: hidden;
    visibility: visible;
    transform: translate3d(0, 0, 0);
    transition:
      transform 0.34s var(--ease-out),
      opacity 0.18s var(--ease-out) 0.16s,
      visibility 0s linear;
    will-change: transform, opacity;

    &.lyrics-hidden {
      opacity: 0;
      visibility: hidden;
      transform: translate3d(24px, 0, 0);
      pointer-events: none;
      transition:
        opacity 0.22s var(--ease-out),
        transform 0.28s var(--ease-out),
        visibility 0s linear 0.28s;
    }
  }
}
</style>
