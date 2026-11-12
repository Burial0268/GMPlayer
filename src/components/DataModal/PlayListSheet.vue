<template>
  <div
    v-if="mounted"
    ref="layerRef"
    :class="['playlist-sheet-layer', { dark: isDark, closing: !music.showPlayList, suspended }]"
    :inert="!navigation.overlays.value.some((layer) => layer.presentation === 'queue-floating')"
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
      @touchcancel.passive="handleTouchCancel"
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
 * 层导航决定显隐，`music.showPlayList` 只是兼容投影；手势完成后请求退出队列层，
 * watch 负责视觉收尾。Escape 与系统返回统一由 useLayerPresentation 处理。
 */
import { animate, Motion, useMotionValue, useTransform, type MotionValue } from "motion-v";
import { musicStore, settingStore } from "@/store";
import QueuePanel from "@/components/QueuePanel/index.vue";
import { useLayerNavigation } from "@/utils/navigation";
import { layerMotion } from "@/utils/navigation/motion";
import { prefersReducedMotion } from "@/utils/reducedMotion";
import { useMotionInterruption } from "@/composables/useMotionInterruption";
import { useSheetDrag } from "@/composables/useSheetDrag";

type MotionStyleRecord = Record<string, string | number | MotionValue | undefined>;

const music = musicStore();
const navigation = useLayerNavigation();
const suspended = ref(false);
const hasFloatingQueue = () =>
  navigation.state.value.layers.some((layer) => layer.presentation === "queue-floating");
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
let settleGeneration = 0;

const clamp01 = (value: number) => Math.min(1, Math.max(0, value));
// 还没量到高度时用视口高度兜底：面板一定比视口短，所以初始位移仍在屏外，
// 不会在挂载的第一帧闪出一个已经就位的抽屉。
const sheetTravel = () => sheetHeightValue.get() || window.innerHeight || 1;

const sheetY = useTransform(() => (1 - clamp01(progress.get())) * sheetTravel());
const scrimOpacity = useTransform(() => clamp01(progress.get()));
const sheetStyle = computed<MotionStyleRecord>(() => ({ y: sheetY }));
const scrimStyle = computed<MotionStyleRecord>(() => ({ opacity: scrimOpacity }));

const stopSettle = () => {
  settleGeneration++;
  settleAnimation?.stop();
  settleAnimation = null;
};

const measureSheet = () => {
  const el = layerRef.value?.querySelector(".playlist-sheet");
  if (el instanceof HTMLElement && el.clientHeight > 0) sheetHeightValue.set(el.clientHeight);
};

const openSheet = () => {
  stopSettle();
  const generation = settleGeneration;
  if (suspended.value) {
    suspended.value = false;
    progress.set(1);
    nextTick(measureSheet);
    return;
  }
  if (!mounted.value) {
    progress.set(0);
    mounted.value = true;
  }
  nextTick(() => {
    if (generation !== settleGeneration || !music.showPlayList) return;
    measureSheet();
    if (prefersReducedMotion()) {
      progress.set(1);
      queuePanelRef.value?.scrollToCurrent();
      return;
    }
    settleAnimation = animate(progress, 1, {
      ...layerMotion.settle,
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
  drag.reset();
  stopSettle();
  if (hasFloatingQueue()) {
    suspended.value = true;
    progress.set(1);
    return;
  }
  if (prefersReducedMotion()) {
    progress.set(0);
    mounted.value = false;
    return;
  }
  // 卸载只发生在这段收起动画自然跑完之后；中途被重新展开打断时 stopSettle 已经把
  // settleAnimation 换成了新的一段，身份不符就什么都不做（stop 不会触发 onComplete，
  // 这里的比对是为了防住「关 → 开 → 关」里第一段的回调迟到）。
  const animation = animate(progress, 0, {
    ...layerMotion.settle,
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
  if (music.showPlayList) navigation.closeQueue();
  else closeSheet();
};

const settleOpen = () => {
  if (prefersReducedMotion()) progress.set(1);
  else settleAnimation = animate(progress, 1, layerMotion.settle);
};

const drag = useSheetDrag({
  progress,
  travel: sheetTravel,
  stop: stopSettle,
  settleOpen,
  dismiss: requestClose,
  scrollers: ".v-vl, .queue-scroll",
  grip: ".sheet-grip",
});
const { handleTouchStart, handleTouchMove, handleTouchEnd, handleTouchCancel, handleGripClick } =
  drag;

useMotionInterruption(() => {
  drag.reset();
  stopSettle();
  if (!mounted.value) return;
  measureSheet();
  suspended.value = !music.showPlayList && hasFloatingQueue();
  progress.set(music.showPlayList || suspended.value ? 1 : 0);
  mounted.value = music.showPlayList || suspended.value;
});

const removeBeforeNavigation = navigation.onBeforeNavigation(handleTouchCancel);
const removeCancelledNavigation = navigation.onNavigationCancelled(() => {
  if (music.showPlayList) openSheet();
});

watch(
  () => music.showPlayList,
  (show) => (show ? openSheet() : closeSheet()),
);

onMounted(() => {
  // 从平板宽度缩到手机宽度时抽屉可能已经是打开状态，这一挂载就要接手。
  if (music.showPlayList) openSheet();
});

onBeforeUnmount(() => {
  removeBeforeNavigation();
  removeCancelledNavigation();
  stopSettle();
});
</script>

<style lang="scss" scoped>
.playlist-sheet-layer {
  &.suspended {
    visibility: hidden;
    pointer-events: none;
  }
  position: fixed;
  inset: 0;
  // 与搜索浮层同层；详情交接时由导航宿主临时抬高内容层。
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
