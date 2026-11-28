<template>
  <n-popover
    v-if="useDrawerLayout"
    class="queue-popover"
    trigger="manual"
    placement="top"
    :show="playListShow"
    :x="anchorPoint.x"
    :y="anchorPoint.y"
    to="body"
    @clickoutside="close"
  >
    <QueuePanel ref="queuePanelRef" />
  </n-popover>
  <PlayListSheet v-else-if="useSheetLayout" />
</template>

<script setup>
/**
 * 播放队列的浮层宿主：按外壳形态挑一种呈现，两者互斥。
 *
 * - 769–1040px：从播放条队列按钮弹出的气泡（n-popover，箭头指着按钮）。仍是桌面外壳
 *   （有 Sidebar、没有 TabBar），不带遮罩、不锁滚动；点外面、再点按钮或 Esc 收起。
 *   贴边整高的侧栏、悬浮在内容右侧的卡片都试过：前者和圆角内容面板割裂，后者盖住了
 *   右上角的按钮，又看不出它是哪颗按钮打开的。
 * - ≤768px：`PlayListSheet` 底部抽屉。移动外壳下右侧抽屉是桌面习惯：整屏从右侧推入、
 *   只能靠右上角那颗 × 关闭，而那颗 × 在无刘海留白的全高面板里正好压在状态栏下面。
 * - ≥1041px 由 App.vue 的内联队列列接管，这里什么都不渲染。
 */
import { musicStore } from "@/store";
import { useLayerNavigation } from "@/utils/navigation";
import { PLAYLIST_DRAWER_MEDIA_QUERY, PLAYLIST_SHEET_MEDIA_QUERY } from "@/utils/ui/playlistLayout";
import QueuePanel from "@/components/QueuePanel/index.vue";
import PlayListSheet from "@/components/DataModal/PlayListSheet.vue";

const props = defineProps({
  /** The player bar's queue button: the popover points at it. */
  anchor: { type: Object, default: null },
});

const music = musicStore();
const navigation = useLayerNavigation();

// Top centre of the queue button, read when the popover opens and on resize while open.
const anchorPoint = reactive({ x: 0, y: 0 });
const measureAnchor = () => {
  const rect = props.anchor?.getBoundingClientRect();
  if (!rect) return;
  anchorPoint.x = rect.left + rect.width / 2;
  anchorPoint.y = rect.top - 4;
};

const close = () => navigation.closeQueue();
const onKeydown = (event) => {
  if (event.key === "Escape" && !event.defaultPrevented) close();
};

// 播放列表显隐
const useDrawerLayout = ref(false);
const useSheetLayout = ref(false);
let drawerMediaQuery = null;
let sheetMediaQuery = null;
const playListShow = ref(false);
const queuePanelRef = ref(null);

watch(playListShow, (open) => {
  if (open) {
    measureAnchor();
    window.addEventListener("resize", measureAnchor);
    document.addEventListener("keydown", onKeydown);
  } else {
    window.removeEventListener("resize", measureAnchor);
    document.removeEventListener("keydown", onKeydown);
  }
});

// 打开时滚动到当前播放曲目
const scrollToCurrentSong = () => {
  nextTick().then(() => {
    if (playListShow.value) queuePanelRef.value?.scrollToCurrent();
  });
};

const syncDrawerLayout = (event) => {
  useDrawerLayout.value = event?.matches ?? drawerMediaQuery?.matches ?? true;
};

const syncSheetLayout = (event) => {
  useSheetLayout.value = event?.matches ?? sheetMediaQuery?.matches ?? false;
};

watch(
  () => music.showPlayList,
  (show) => {
    if (useDrawerLayout.value) {
      playListShow.value = show;
    } else {
      playListShow.value = false;
    }
    scrollToCurrentSong();
  },
);

watch(
  () => useDrawerLayout.value,
  (isDrawerLayout) => {
    playListShow.value = isDrawerLayout ? music.showPlayList : false;
    scrollToCurrentSong();
  },
);

onMounted(() => {
  if (typeof window !== "undefined") {
    drawerMediaQuery = window.matchMedia(PLAYLIST_DRAWER_MEDIA_QUERY);
    sheetMediaQuery = window.matchMedia(PLAYLIST_SHEET_MEDIA_QUERY);
    syncDrawerLayout();
    syncSheetLayout();
    drawerMediaQuery.addEventListener("change", syncDrawerLayout);
    sheetMediaQuery.addEventListener("change", syncSheetLayout);
  } else {
    useDrawerLayout.value = true;
  }
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", measureAnchor);
  document.removeEventListener("keydown", onKeydown);
  drawerMediaQuery?.removeEventListener("change", syncDrawerLayout);
  sheetMediaQuery?.removeEventListener("change", syncSheetLayout);
});
</script>

<style lang="scss">
// Teleported to body, so a global rule. The glass is GlassSheet's material; the arrow
// paints with --n-color, so it takes the same tint as the body it hangs from.
.n-popover.queue-popover {
  --n-color: rgba(var(--app-shell-rgb, 242, 242, 244), 0.92) !important;
  --n-padding: 0 !important;
  // QueuePanel paints its own opaque surface unless told otherwise; the glass is this layer.
  --queue-surface-bg: transparent;
  // Floating queue: rounded rows, every other one tinted (same as the mobile sheet). The
  // docked column at >=1041px keeps the flush striped stack.
  --queue-row-radius: var(--radius-md);
  --queue-row-odd-bg: transparent;
  width: 340px;
  border-radius: var(--radius-panel, 16px);

  // The height goes on the panel: its `height: 100%` does not resolve inside the popover, and
  // the body cannot clip (overflow: hidden would cut off the arrow hanging below it).
  .queue-panel {
    height: min(480px, calc(100vh - var(--app-player-bar-height, 70px) - 32px));
    border-radius: inherit;
  }
  -webkit-backdrop-filter: blur(26px) saturate(180%);
  backdrop-filter: blur(26px) saturate(180%);
  box-shadow:
    0 18px 48px rgb(0 0 0 / 18%),
    inset 0 1px 0 rgb(255 255 255 / 40%);

  html[data-theme="dark"] & {
    box-shadow:
      0 18px 48px rgb(0 0 0 / 40%),
      inset 0 1px 0 rgb(255 255 255 / 8%);
  }
}
</style>
