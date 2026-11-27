<template>
  <n-drawer
    v-if="useDrawerLayout"
    class="playlist-drawer"
    :show="playListShow"
    :z-index="2200"
    :width="400"
    :show-mask="false"
    :trap-focus="false"
    :block-scroll="false"
    placement="right"
    to="body"
    @update:show="handleDrawerShowUpdate"
  >
    <n-drawer-content
      class="playlist-drawer-content"
      :native-scrollbar="true"
      :body-content-style="{ padding: 0, height: '100%' }"
    >
      <QueuePanel ref="queuePanelRef" />
    </n-drawer-content>
  </n-drawer>
  <PlayListSheet v-else-if="useSheetLayout" />
</template>

<script setup>
/**
 * 播放队列的浮层宿主：按外壳形态挑一种呈现，两者互斥。
 *
 * - 769–1040px：右侧 n-drawer。仍是桌面外壳（有 Sidebar、没有 TabBar），且刻意不带
 *   遮罩、不锁滚动 —— 队列在旁边开着，主内容照样能用。它画成悬浮在内容面板上的玻璃卡片
 *   （离窗口边与播放条各留一道缝），不是贴边整高的侧栏：那样和 ≥1041px 的内联队列列、
 *   和圆角的内容面板都对不上。没有自己的标题栏和 ×，同内联列：用播放条按钮或 Esc 收起。
 * - ≤768px：`PlayListSheet` 底部抽屉。移动外壳下右侧抽屉是桌面习惯：整屏从右侧推入、
 *   只能靠右上角那颗 × 关闭，而那颗 × 在无刘海留白的全高面板里正好压在状态栏下面。
 * - ≥1041px 由 App.vue 的内联队列列接管，这里什么都不渲染。
 */
import { musicStore } from "@/store";
import { useLayerNavigation } from "@/utils/navigation";
import { PLAYLIST_DRAWER_MEDIA_QUERY, PLAYLIST_SHEET_MEDIA_QUERY } from "@/utils/ui/playlistLayout";
import QueuePanel from "@/components/QueuePanel/index.vue";
import PlayListSheet from "@/components/DataModal/PlayListSheet.vue";

const music = musicStore();
const navigation = useLayerNavigation();

// 播放列表显隐
const useDrawerLayout = ref(false);
const useSheetLayout = ref(false);
let drawerMediaQuery = null;
let sheetMediaQuery = null;
const playListShow = ref(false);
const queuePanelRef = ref(null);

const handleDrawerShowUpdate = (show) => {
  if (!show) {
    navigation.closeQueue();
    return;
  }
  if (useDrawerLayout.value && music.showPlayList) {
    playListShow.value = true;
  }
};

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
  drawerMediaQuery?.removeEventListener("change", syncDrawerLayout);
  sheetMediaQuery?.removeEventListener("change", syncSheetLayout);
});
</script>

<style lang="scss">
.playlist-drawer.n-drawer {
  top: var(--app-shell-top-gap, 6px);
  right: 8px;
  bottom: calc(var(--app-bottom-chrome-with-player) + 8px);
  width: min(360px, calc(100vw - 16px)) !important;
  height: auto;
  overflow: hidden;
  border-radius: var(--radius-panel, 16px);
  // GlassSheet's material, so the queue reads as the same kind of surface as every other sheet.
  background-color: rgba(var(--app-shell-rgb, 242, 242, 244), 0.86);
  -webkit-backdrop-filter: blur(26px) saturate(180%);
  backdrop-filter: blur(26px) saturate(180%);
  box-shadow:
    0 18px 48px rgb(0 0 0 / 18%),
    inset 0 1px 0 rgb(255 255 255 / 40%);
  // QueuePanel paints its own opaque surface unless told otherwise; the glass is this layer.
  --queue-surface-bg: transparent;

  html[data-theme="dark"] & {
    box-shadow:
      0 18px 48px rgb(0 0 0 / 40%),
      inset 0 1px 0 rgb(255 255 255 / 8%);
  }

  .n-drawer-content,
  .n-drawer-body,
  .n-drawer-body-content-wrapper {
    background: transparent;
  }

  // QueuePanel 自管滚动与内边距，抽屉主体不再包滚动容器
  .n-drawer-body-content-wrapper {
    padding: 0 !important;
    height: 100%;
  }
}
</style>
