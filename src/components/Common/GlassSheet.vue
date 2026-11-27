<template>
  <Teleport to="body">
    <div
      v-if="mounted"
      :class="['action-sheet', 'glass-sheet', layout, { closing: !show }]"
      :style="{ zIndex, '--sheet-width': width ? `${width}px` : undefined }"
    >
      <Motion class="glass-sheet-scrim" :style="scrimStyle" @click="dismiss" />
      <Motion
        ref="panelRef"
        class="glass-sheet-panel"
        role="dialog"
        aria-modal="true"
        :aria-label="label ?? title"
        :style="panelStyle"
        @touchstart.passive="onTouchStart"
        @touchmove="onTouchMove"
        @touchend.passive="onTouchEnd"
        @touchcancel.passive="onTouchCancel"
      >
        <button
          v-if="layout === 'bottom'"
          class="sheet-grip"
          type="button"
          :aria-label="$t('general.dialog.cancel')"
          @click="drag.handleGripClick"
        >
          <span class="sheet-grip-bar" />
        </button>
        <slot name="header">
          <div v-if="title" class="sheet-song">
            <div class="sheet-song-text">
              <div class="sheet-song-name">{{ title }}</div>
            </div>
          </div>
        </slot>
        <div class="sheet-scroll">
          <slot />
        </div>
        <div v-if="$slots.footer" class="sheet-footer">
          <slot name="footer" />
        </div>
      </Motion>
    </div>
  </Teleport>
</template>

<script lang="ts">
// Module scope, shared by every GlassSheet: the open sheets, oldest first.
const openSheets: symbol[] = [];
</script>

<script setup lang="ts">
/**
 * 玻璃面板：移动外壳（≤768px）下是可下拉关闭的底部抽屉，桌面外壳下是居中的同材质面板。
 *
 * 外观与手势和 PlayListSheet 同一套（玻璃、抓手、跟手下拉、遮罩随进度）。添加到歌单、
 * 大播放器的「更多」、歌曲列表的操作菜单共用它，行样式（`.sheet-group` / `.sheet-row` /
 * `.sheet-song`）也在这里，消费方只写自己特有的部分。
 *
 * teleport 到 body：调用方可能在带 transform 的页面层里，fixed 会相对它定位。颜色因此
 * 只读 :root 令牌（--text-primary 等），继承不到 naive 写在组件根上的 --n-text-color。
 * 根节点带 `action-sheet`：useLayerPresentation 据此把系统返回转成 Escape 交给这里。
 * PlayListSheet 不用它：那个抽屉的显隐属于层导航，挂载位置也是刻意的（见其注释）。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { animate, Motion, useMotionValue, useTransform } from "motion-v";
import { layerMotion } from "@/utils/navigation/motion";
import { prefersReducedMotion } from "@/utils/ui/reducedMotion";
import { PLAYLIST_SHEET_MEDIA_QUERY } from "@/utils/ui/playlistLayout";
import { useSheetDrag } from "@/composables/useSheetDrag";

const show = defineModel<boolean>("show", { required: true });
const props = withDefaults(
  defineProps<{
    label?: string;
    /** Header text when no `header` slot is given; also the default `label`. */
    title?: string;
    /** Centred panel width in px (default 440). */
    width?: number;
    /** Scrim, Escape and drag do not close it (e.g. an upload in progress). */
    persistent?: boolean;
    zIndex?: number;
  }>(),
  {
    label: undefined,
    title: undefined,
    width: undefined,
    persistent: false,
    // 与 PlayListSheet / 队列抽屉同层，盖过大播放器（2000）。
    zIndex: 2200,
  },
);

const sheetMedia =
  typeof window !== "undefined" ? window.matchMedia(PLAYLIST_SHEET_MEDIA_QUERY) : null;
const isBottom = ref(sheetMedia?.matches ?? true);
const syncLayout = () => (isBottom.value = sheetMedia?.matches ?? true);
onMounted(() => sheetMedia?.addEventListener("change", syncLayout));
const layout = computed(() => (isBottom.value ? "bottom" : "center"));

// ── 开合：progress 0 = 收起、1 = 展开；收起动画跑完才卸载 ──
const mounted = ref(false);
const panelRef = ref<{ $el: HTMLElement } | null>(null);
const progress = useMotionValue(0);
const panelHeight = useMotionValue(0);
const clamp01 = (value: number) => Math.min(1, Math.max(0, value));
const travel = () => panelHeight.get() || window.innerHeight || 1;
const sheetY = useTransform(() => (1 - clamp01(progress.get())) * travel());
const centerScale = useTransform(() => 0.96 + 0.04 * clamp01(progress.get()));
const opacity = useTransform(() => clamp01(progress.get()));
const scrimStyle = { opacity };
const panelStyle = computed(() =>
  isBottom.value ? { y: sheetY } : { scale: centerScale, opacity },
);
let settle: ReturnType<typeof animate> | null = null;

const stopSettle = () => {
  settle?.stop();
  settle = null;
};
const settleTo = (target: 0 | 1) => {
  stopSettle();
  if (prefersReducedMotion()) {
    progress.set(target);
    if (!target) mounted.value = false;
    return;
  }
  const animation = animate(progress, target, {
    ...layerMotion.settle,
    onComplete: () => {
      if (settle !== animation) return;
      settle = null;
      if (!target) mounted.value = false;
    },
  });
  settle = animation;
};

const drag = useSheetDrag({
  progress,
  travel,
  stop: stopSettle,
  settleOpen: () => settleTo(1),
  dismiss: () => (props.persistent ? settleTo(1) : (show.value = false)),
  scrollers: ".sheet-scroll",
  grip: ".sheet-grip",
});
// 居中面板没有下拉手势。
const onTouchStart = (e: TouchEvent) => isBottom.value && drag.handleTouchStart(e);
const onTouchMove = (e: TouchEvent) => isBottom.value && drag.handleTouchMove(e);
const onTouchEnd = (e: TouchEvent) => isBottom.value && drag.handleTouchEnd(e);
const onTouchCancel = () => isBottom.value && drag.handleTouchCancel();

const dismiss = () => {
  if (!props.persistent) show.value = false;
};

// Sheets stack (a confirmation over "manage sources", "new playlist" over "add to playlist"),
// and keydown listeners run in the order they were added: only the topmost may take Escape,
// or the first sheet opened closes underneath the one the user is looking at.
const self = Symbol("glass-sheet");

const onKeydown = (event: KeyboardEvent) => {
  if (openSheets[openSheets.length - 1] !== self) return;
  if (event.key !== "Escape" || !show.value || event.defaultPrevented || props.persistent) return;
  event.preventDefault();
  show.value = false;
};

watch(
  show,
  async (open) => {
    drag.reset();
    const index = openSheets.indexOf(self);
    if (index >= 0) openSheets.splice(index, 1);
    if (open) openSheets.push(self);
    if (open) document.addEventListener("keydown", onKeydown);
    else document.removeEventListener("keydown", onKeydown);
    if (!open) {
      if (mounted.value) settleTo(0);
      return;
    }
    if (!mounted.value) {
      progress.set(0);
      mounted.value = true;
    }
    await nextTick();
    const height = panelRef.value?.$el?.clientHeight;
    if (height) panelHeight.set(height);
    if (show.value) settleTo(1);
  },
  { immediate: true },
);

onBeforeUnmount(() => {
  const index = openSheets.indexOf(self);
  if (index >= 0) openSheets.splice(index, 1);
  stopSettle();
  sheetMedia?.removeEventListener("change", syncLayout);
  document.removeEventListener("keydown", onKeydown);
});
</script>

<style lang="scss">
.glass-sheet {
  position: fixed;
  inset: 0;
  pointer-events: auto;
  color: var(--text-primary);
  --sheet-fill: color-mix(in srgb, var(--text-primary) 6%, transparent);
  --sheet-separator: color-mix(in srgb, var(--text-primary) 10%, transparent);

  // 收起动画期间吞掉输入：此时 show 已经是 false，再关一次没有状态可翻。
  &.closing {
    pointer-events: none;
  }

  &.center {
    display: grid;
    place-items: center;
    padding: 24px;
  }
}

// naive's floating layers (select menus, colour pickers, tooltips) take z-indexes counting up
// from 2000, under a sheet at 2200 or more, so a select inside a sheet opened behind it.
body:has(> .glass-sheet) .v-binder-follower-container {
  z-index: 10100 !important;
}

.glass-sheet-scrim {
  position: absolute;
  inset: 0;
  background-color: rgb(0 0 0 / 42%);
  will-change: opacity;

  html[data-theme="dark"] & {
    background-color: rgb(0 0 0 / 56%);
  }
}

.glass-sheet-panel {
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background-color: rgba(var(--app-shell-rgb, 242, 242, 244), 0.86);
  // 前缀在前、标准在后（全项目同此约定，见 PlayListSheet）。
  -webkit-backdrop-filter: blur(26px) saturate(180%);
  backdrop-filter: blur(26px) saturate(180%);
  box-shadow:
    0 -18px 46px rgb(0 0 0 / 14%),
    inset 0 1px 0 rgb(255 255 255 / 40%);
  will-change: transform;

  html[data-theme="dark"] .bottom > & {
    box-shadow:
      0 -18px 46px rgb(0 0 0 / 34%),
      inset 0 1px 0 rgb(255 255 255 / 8%);
  }

  .bottom > & {
    position: absolute;
    right: 0;
    bottom: 0;
    left: 0;
    max-height: calc(100vh - 84px);
    border-radius: 16px 16px 0 0;
  }

  .center > & {
    position: relative;
    width: min(var(--sheet-width, 440px), 100%);
    max-height: min(640px, calc(100vh - 96px));
    padding-top: 16px;
    border-radius: var(--radius-lg, 16px);
    box-shadow:
      0 24px 64px rgb(0 0 0 / 22%),
      inset 0 1px 0 rgb(255 255 255 / 40%);
  }

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
    // 落在抓手上的手势一律交给我们，preventDefault 才始终有效。
    touch-action: none;

    &:focus-visible {
      outline: none;
      border-radius: var(--radius-pill);
      box-shadow: var(--focus-ring);
    }
  }

  .sheet-grip-bar {
    width: 36px;
    height: 4px;
    border-radius: var(--radius-pill);
    background-color: color-mix(in srgb, var(--text-primary) 24%, transparent);
  }

  .sheet-scroll {
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: 0 16px calc(16px + var(--app-safe-area-bottom, 0px));
  }

  // 操作按钮（下载、匹配）：固定在面板底部，不随列表滚走；底部安全区由它让出。
  .sheet-scroll:has(+ .sheet-footer) {
    padding-bottom: 8px;
  }

  .sheet-footer {
    flex: 0 0 auto;
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    padding: 12px 16px calc(14px + var(--app-safe-area-bottom, 0px));
    border-top: 1px solid var(--sheet-separator);

    .bottom > & .n-button {
      flex: 1;
    }
  }

  // 头部：封面 + 两行文字（歌曲操作）或标题行（添加到歌单）。
  .sheet-song {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 20px 14px;
  }

  .sheet-cover {
    width: 48px;
    height: 48px;
    flex-shrink: 0;
    border-radius: var(--radius-sm);
    object-fit: cover;
    background: var(--sheet-fill);
  }

  .sheet-song-text {
    flex: 1;
    min-width: 0;
  }

  .sheet-song-name {
    font-size: 1rem;
    font-weight: 600;
    line-height: 1.3;
  }

  .sheet-song-artists {
    margin-top: 2px;
    font-size: 0.82rem;
    color: var(--text-secondary);
  }

  // 分组卡片 + 行间细线，同一种行高与图标列，和设置页的分组列表同一语法。
  .sheet-group {
    overflow: hidden;
    border-radius: var(--radius-md);
    background: var(--sheet-fill);

    & + .sheet-group {
      margin-top: 12px;
    }
  }

  .sheet-row {
    appearance: none;
    position: relative;
    width: 100%;
    min-height: 52px;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 0 14px;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 0.95rem;
    text-align: left;
    box-sizing: border-box;

    // 细线从文字起点开始，不穿过图标列。
    & + .sheet-row::before {
      content: "";
      position: absolute;
      top: 0;
      right: 0;
      left: var(--sheet-row-inset, 50px);
      height: 1px;
      background: var(--sheet-separator);
    }

    &:is(button) {
      cursor: pointer;
      transition: background-color var(--duration-150) var(--ease-out);

      &:active {
        background: var(--sheet-fill);
      }

      @media (hover: hover) {
        &:hover {
          background: var(--sheet-fill);
        }
      }

      &:disabled {
        cursor: default;
        opacity: 0.6;
      }
    }

    &:focus-visible {
      outline: none;
      box-shadow: inset var(--focus-ring);
    }
  }

  .row-icon {
    flex-shrink: 0;
    color: var(--main-color);
  }

  // 破坏性操作（删除云盘歌曲）。面板在 body 下读不到 naive 的 --n-error-color，取它的默认值。
  .danger-row,
  .danger-row .row-icon {
    color: #d03050;
  }

  .row-label {
    flex: 1;
    min-width: 0;
  }

  .row-meta {
    flex-shrink: 0;
    font-size: 0.82rem;
    color: var(--text-secondary);
  }

  .row-chevron {
    flex-shrink: 0;
    color: var(--text-secondary);
    opacity: 0.6;
  }
}
</style>
