<template>
  <div
    v-if="mounted"
    ref="layerRef"
    :class="['playlist-sheet-layer', { dark: isDark, closing: !music.showPlayList }]"
  >
    <Motion class="playlist-sheet-scrim" :style="scrimStyle" @click="requestClose" />
    <Motion
      class="playlist-sheet"
      role="dialog"
      aria-modal="true"
      :aria-label="$t('general.name.playlists')"
      :style="sheetStyle"
      @touchstart.passive="handleTouchStart"
      @touchmove="handleTouchMove"
      @touchend.passive="handleTouchEnd"
      @touchcancel.passive="handleTouchEnd"
    >
      <button
        class="sheet-grip"
        type="button"
        :aria-label="$t('player.queue.collapse')"
        @click="handleGripClick"
      >
        <span class="sheet-grip-bar" />
      </button>
      <QueuePanel ref="queuePanelRef" class="sheet-queue" />
    </Motion>
  </div>
</template>

<script setup lang="ts">
/**
 * 移动端播放队列的底部抽屉。
 *
 * 只在移动外壳（≤768px，见 `utils/playlistLayout`）里由 PlayListDrawer 挂载，
 * 769–1040px 仍是右侧 n-drawer。刻意没有复用 n-drawer：遮罩浓度要跟着拖拽进度走，
 * 而 naive 的 mask 只有开 / 关两态；面板的 transform 也由它的过渡类接管，跟手拖拽
 * 必然和过渡类抢同一个属性 —— 内联 transform 赢，于是退场动画反而失效。
 *
 * 也**没有** teleport 到 body，尽管旁边那个抽屉是。QueuePanel 通篇在读 `--n-text-color`
 * （文字色，以及 color-mix 出来的行底色），而这个变量是 naive 组件写在自己根节点上的
 * 内联变量：右侧抽屉靠 `.n-drawer-content` 提供，内联队列列靠 `.n-layout-content`。
 * 挂到 body 下就谁也继承不到，`color-mix()` 里的无效 var 会让整条声明失效 —— 行底色
 * 直接没了，而不是退成别的颜色。留在原地（DOM 上就是 `.n-layout` 的子节点，和
 * BigPlayer、迷你条同一层）变量自然继承，主题切换也跟着走。
 *
 * 唯一状态源仍是 `music.showPlayList`：手势判定为关闭时只写 store，由下面的 watch
 * 驱动同一段收起动画，而不是自己偷偷卸载。迷你条的按钮、SearchInp、BigPlayer 都在
 * 读写这个字段，多一个写者就会出现「按钮亮着但抽屉没了」。
 */
import { animate, Motion, useMotionValue, useTransform, type MotionValue } from "motion-v";
import { musicStore, settingStore } from "@/store";
import QueuePanel from "@/components/QueuePanel/index.vue";

type MotionStyleRecord = Record<string, string | number | MotionValue | undefined>;

const music = musicStore();
const setting = settingStore();

const layerRef = ref<HTMLElement | null>(null);
const queuePanelRef = ref<{ scrollToCurrent: () => void } | null>(null);
const mounted = ref(false);
const isDark = computed(() => setting.getSiteTheme === "dark");

// 0 = 完全收起，1 = 完全展开。位移取像素而不是百分比：手势要按屏幕上的实际距离判定，
// 而 76vh 的面板高度只有量过才知道。
const progress = useMotionValue(0);
const sheetHeightValue = useMotionValue(0);
let settleAnimation: ReturnType<typeof animate> | null = null;

const clamp01 = (value: number) => Math.min(1, Math.max(0, value));
// 还没量到高度时用视口高度兜底：面板一定比视口短，所以初始位移仍在屏外，
// 不会在挂载的第一帧闪出一个已经就位的抽屉。
const sheetTravel = () => sheetHeightValue.get() || window.innerHeight || 1;

const sheetY = useTransform(() => (1 - clamp01(progress.get())) * sheetTravel());
const scrimOpacity = useTransform(() => clamp01(progress.get()));
const sheetStyle = computed<MotionStyleRecord>(() => ({ y: sheetY }));
const scrimStyle = computed<MotionStyleRecord>(() => ({ opacity: scrimOpacity }));

// 近临界阻尼（ζ≈0.98）：行程有大半屏，欠阻尼的回弹会让面板顶边过冲、露出遮罩后的
// 页面。与 MobilePlayerLayout 的队列分页同一组参数。
const settleTransition = {
  type: "spring",
  stiffness: 420,
  damping: 38,
  mass: 0.9,
  restDelta: 0.001,
  restSpeed: 0.02,
} as const;

const DRAG_THRESHOLD_PX = 8;
const DISMISS_PROGRESS = 0.65;
const DISMISS_VELOCITY = 0.6;

const stopSettle = () => {
  settleAnimation?.stop();
  settleAnimation = null;
};

const measureSheet = () => {
  const el = layerRef.value?.querySelector(".playlist-sheet");
  if (el instanceof HTMLElement && el.clientHeight > 0) sheetHeightValue.set(el.clientHeight);
};

const openSheet = () => {
  stopSettle();
  if (!mounted.value) {
    progress.set(0);
    mounted.value = true;
  }
  nextTick(() => {
    measureSheet();
    settleAnimation = animate(progress, 1, {
      ...settleTransition,
      // 面板是每次展开重新挂载的，第一帧虚拟列表往往还没量到视口高度，scrollTo 会被
      // 夹到 0。展开落定后再补一次；命中同一位置时没有副作用（scrollTo 不带
      // behavior，不会有可见跳动）。
      onComplete: () => queuePanelRef.value?.scrollToCurrent(),
    });
    queuePanelRef.value?.scrollToCurrent();
  });
};

const closeSheet = () => {
  if (!mounted.value) return;
  stopSettle();
  // 卸载只发生在这段收起动画自然跑完之后；中途被重新展开打断时 stopSettle 已经把
  // settleAnimation 换成了新的一段，身份不符就什么都不做（stop 不会触发 onComplete，
  // 这里的比对是为了防住「关 → 开 → 关」里第一段的回调迟到）。
  const animation = animate(progress, 0, {
    ...settleTransition,
    onComplete: () => {
      if (settleAnimation !== animation) return;
      settleAnimation = null;
      mounted.value = false;
    },
  });
  settleAnimation = animation;
};

const requestClose = () => {
  // store 已经是 false 说明收起动画正在跑，直接续上，不然这一下点击就没有状态可翻。
  if (music.showPlayList) music.showPlayList = false;
  else closeSheet();
};

type SheetTouch = {
  x: number;
  y: number;
  dragging: boolean;
  fromGrip: boolean;
  scrollTop: number;
};

let touchState: SheetTouch | null = null;
let suppressGripClick = false;

/** 手指落点所在的滚动容器已经滚了多远；抓手不在列表里，closest 自然返回 null。 */
const scrollTopAt = (target: EventTarget | null) => {
  if (!(target instanceof Element)) return 0;
  const scroller = target.closest(".v-vl, .queue-scroll");
  return scroller instanceof HTMLElement ? scroller.scrollTop : 0;
};

const handleTouchStart = (event: TouchEvent) => {
  const touch = event.changedTouches?.[0];
  if (!touch) return;
  // 每一次新手势都从「不吞点击」重新开始：拖拽结束后若把标记留着，下一次真正的
  // 轻点就会被白白吃掉一次。
  suppressGripClick = false;
  touchState = {
    x: touch.clientX,
    y: touch.clientY,
    dragging: false,
    fromGrip: event.target instanceof Element && Boolean(event.target.closest(".sheet-grip")),
    scrollTop: scrollTopAt(event.target),
  };
};

const handleTouchMove = (event: TouchEvent) => {
  const start = touchState;
  const touch = event.changedTouches?.[0];
  if (!start || !touch) return;

  const deltaX = touch.clientX - start.x;
  const deltaY = touch.clientY - start.y;
  if (!start.dragging) {
    if (Math.abs(deltaY) < DRAG_THRESHOLD_PX) return;
    const vertical = Math.abs(deltaY) > Math.abs(deltaX) * 1.15;
    // 只有「列表已经在顶部」或「手指落在抓手上」时才把下拉解释为关闭手势，否则把
    // 手势还给列表自己滚 —— 和 MobilePlayerLayout 队列页的判据一致。
    if (!vertical || deltaY <= 0 || (!start.fromGrip && start.scrollTop > 4)) {
      touchState = null;
      return;
    }
    start.dragging = true;
    suppressGripClick = start.fromGrip;
    stopSettle();
  }

  // 拖拽期间接管手势，阻止列表同时滚动（touchmove 不能加 .passive，否则拦不住）
  if (event.cancelable) event.preventDefault();
  progress.set(1 - clamp01(deltaY / sheetTravel()));
};

const handleTouchEnd = () => {
  const start = touchState;
  touchState = null;
  if (!start?.dragging) return;
  const velocity = progress.getVelocity();
  // 甩动优先于位置：慢慢拖到一半再松手应该弹回，快速下滑一点点就该关。
  const dismiss =
    velocity < -DISMISS_VELOCITY
      ? true
      : velocity > DISMISS_VELOCITY
        ? false
        : progress.get() < DISMISS_PROGRESS;
  if (dismiss) {
    requestClose();
    return;
  }
  settleAnimation = animate(progress, 1, settleTransition);
};

const handleGripClick = () => {
  // 拖拽时 preventDefault 过，多数引擎不会再合成 click；这一层是兜底，
  // 免得「从抓手拖一半又弹回」顺带被判成一次关闭。
  if (suppressGripClick) {
    suppressGripClick = false;
    return;
  }
  requestClose();
};

const handleKeydown = (event: KeyboardEvent) => {
  // ≤768px 也可能是被拖窄的桌面窗口，那里没有下拉手势可用。
  if (event.key !== "Escape" || !music.showPlayList) return;
  requestClose();
};

watch(
  () => music.showPlayList,
  (show) => (show ? openSheet() : closeSheet()),
);

onMounted(() => {
  // 从平板宽度缩到手机宽度时抽屉可能已经是打开状态，这一挂载就要接手。
  if (music.showPlayList) openSheet();
  window.addEventListener("keydown", handleKeydown);
  // 76vh 会随旋转 / 分屏改变，行程必须重量，否则收起后底边仍会露出一条。
  window.addEventListener("resize", measureSheet);
});

onBeforeUnmount(() => {
  stopSettle();
  window.removeEventListener("keydown", handleKeydown);
  window.removeEventListener("resize", measureSheet);
});
</script>

<style lang="scss" scoped>
.playlist-sheet-layer {
  position: fixed;
  inset: 0;
  // 与旧的右侧抽屉同层。搜索浮层（--z-search-overlay: 2200）与它互斥（SearchInp
  // 打开时会关掉 showPlayList），移动端 BigPlayer 是 2100，打开时同样会关掉它。
  z-index: 2200;

  // 收起动画期间吞掉输入：此时 store 已经是 false，若还能抓住退场中的面板，
  // 「再关一次」就没有状态可翻，面板会停在半空。
  &.closing {
    pointer-events: none;
  }
}

.playlist-sheet-scrim {
  position: absolute;
  inset: 0;
  background-color: rgb(0 0 0 / 42%);
  will-change: opacity;

  .dark & {
    background-color: rgb(0 0 0 / 56%);
  }
}

.playlist-sheet {
  position: absolute;
  right: 0;
  bottom: 0;
  left: 0;
  // 顶部必须留一条变暗的页面，抽屉才读得出「盖在内容上」而不是「换了一页」。
  // clamp 下限是矮屏兜底（横屏 / 分屏），上限保证那条缝不会被吃掉。
  height: clamp(280px, 76vh, calc(100vh - 84px));
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border-radius: 16px 16px 0 0;
  // 面板贴着屏幕底边（否则玻璃下方会露出一条外壳色），home indicator 的余量由
  // 列表自己的内边距让出。
  --queue-pad-bottom: calc(12px + var(--app-safe-area-bottom, 0px));
  // 玻璃在这一层，所以 QueuePanel 的实色外壳底要让开，不然模糊是白做的。
  --queue-surface-bg: transparent;
  background-color: rgba(var(--app-shell-rgb, 242, 242, 244), 0.86);
  // 前缀在前、标准在后：压缩器把两者视作同一属性的重复声明、只留最后一条，
  // 写反顺序会让不认前缀的引擎彻底丢掉模糊（全项目同此约定）。
  -webkit-backdrop-filter: blur(26px) saturate(180%);
  backdrop-filter: blur(26px) saturate(180%);
  box-shadow:
    0 -18px 46px rgb(0 0 0 / 14%),
    inset 0 1px 0 rgb(255 255 255 / 40%);
  will-change: transform;

  .dark & {
    box-shadow:
      0 -18px 46px rgb(0 0 0 / 34%),
      inset 0 1px 0 rgb(255 255 255 / 8%);
  }
}

// 抓手既是「可以下拉」的视觉提示，也是关闭手势唯一不看列表滚动位置的抓取区，
// 因此整条 26px 都可拖、可点，而不只是那 36px 的横杠。
.sheet-grip {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  justify-content: center;
  height: 26px;
  padding: 0;
  border: 0;
  background: transparent;
  cursor: pointer;
  // 落在抓手上的手势一律交给我们，不去找可滚动祖先，preventDefault 才始终有效。
  touch-action: none;

  &:focus-visible {
    outline: none;
    border-radius: var(--radius-pill);
    box-shadow: var(--focus-ring);
  }

  &:active .sheet-grip-bar {
    background-color: color-mix(in srgb, var(--n-text-color) 42%, transparent);
  }
}

.sheet-grip-bar {
  width: 36px;
  height: 4px;
  border-radius: var(--radius-pill);
  background-color: color-mix(in srgb, var(--n-text-color) 24%, transparent);
  transition: background-color var(--duration-150) var(--ease-out);
}

.sheet-queue {
  flex: 1;
  min-height: 0;
}
</style>
