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
      closable
    >
      <template #header>
        <n-text class="playlist-title">{{ $t("general.name.playlists") }}</n-text>
      </template>
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
 *   遮罩、不锁滚动 —— 队列在旁边开着，主内容照样能用。
 * - ≤768px：`PlayListSheet` 底部抽屉。移动外壳下右侧抽屉是桌面习惯：整屏从右侧推入、
 *   只能靠右上角那颗 × 关闭，而那颗 × 在无刘海留白的全高面板里正好压在状态栏下面。
 * - ≥1041px 由 App.vue 的内联队列列接管，这里什么都不渲染。
 */
import { musicStore } from "@/store";
import { useLayerNavigation } from "@/utils/navigation";
import { PLAYLIST_DRAWER_MEDIA_QUERY, PLAYLIST_SHEET_MEDIA_QUERY } from "@/utils/playlistLayout";
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
.playlist-drawer {
  width: 400px !important;
  border-radius: 0;
  transition: width var(--duration-300) var(--ease-out);

  .n-drawer-header {
    height: 60px;
    box-sizing: border-box;
  }

  // QueuePanel 自管滚动与内边距，抽屉主体不再包滚动容器
  .n-drawer-body-content-wrapper {
    padding: 0 !important;
    height: 100%;
  }
}
</style>

<style lang="scss" scoped>
.playlist-title {
  font-size: 15px;
  font-weight: 700;
}
</style>
