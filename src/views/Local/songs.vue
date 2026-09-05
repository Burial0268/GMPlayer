<template>
  <div class="local-songs">
    <div v-if="!local.hasLibrary" class="empty">
      <n-empty :description="$t('local.noSources')" size="large">
        <template #extra>
          <n-space :size="10" justify="center">
            <n-button strong secondary round type="primary" @click="importDirectory">
              {{ $t("local.addFolder") }}
            </n-button>
            <!-- Both offered from the start: wanting one song is as ordinary as
                 wanting a folder. -->
            <n-button strong secondary round @click="importFiles">
              {{ $t("local.addFiles") }}
            </n-button>
          </n-space>
        </template>
      </n-empty>
    </div>

    <template v-else>
      <div class="list-toolbar">
        <n-input
          class="list-search"
          :class="{ 'has-value': !!keyword }"
          v-model:value="keyword"
          clearable
          size="small"
          :placeholder="$t('local.filterPlaceholder')"
        >
          <template #prefix>
            <n-icon :component="Filter" />
          </template>
        </n-input>
        <n-select
          class="list-sort"
          v-model:value="sort"
          size="small"
          :options="sortOptions"
          :consistent-menu-width="false"
        />
        <n-button
          class="list-order"
          size="small"
          circle
          quaternary
          :title="$t('local.toggleOrder')"
          @click="descending = !descending"
        >
          <template #icon>
            <n-icon :component="descending ? SortAmountDown : SortAmountUp" />
          </template>
        </n-button>
      </div>

      <DataLists
        :listData="songs"
        :loading="loading"
        :capabilities="capabilities"
        :empty-text="$t('local.noMatches')"
        show-header
        page-window
        :virtual-item-size="54"
        :virtual-threshold="40"
        :total-rows="total"
        @reach-end="loadMore"
      />
    </template>
  </div>
</template>

<script setup lang="ts">
import { Filter, SortAmountDown, SortAmountUp } from "@icon-park/vue-next";
import { useI18n } from "vue-i18n";
import { useLocalLibraryStore } from "@/store";
import DataLists from "@/components/DataList/DataLists.vue";
import { LOCAL_AUTO_CAPABILITIES } from "@/utils/playlistSource";
import type { SongData } from "@/store/musicTypes";

const { t } = useI18n();
const local = useLocalLibraryStore();

/**
 * One page's worth. Chosen to be comfortably more than a screen so the first
 * paint is complete, and small enough that a 20k-track library does not cross
 * the IPC boundary at once.
 */
const PAGE_SIZE = 200;

const songs = ref<SongData[]>([]);
const total = ref(0);
const loading = ref(false);
const keyword = ref("");
const sort = ref<"title" | "artist" | "album" | "added" | "duration">("title");
const descending = ref(false);

const capabilities = LOCAL_AUTO_CAPABILITIES;

const sortOptions = computed(() => [
  { label: t("local.sort.title"), value: "title" },
  { label: t("local.sort.artist"), value: "artist" },
  { label: t("local.sort.album"), value: "album" },
  { label: t("local.sort.added"), value: "added" },
  { label: t("local.sort.duration"), value: "duration" },
]);

/**
 * Reload from the first page.
 *
 * Guarded by a token rather than by a boolean: typing in the filter fires this
 * on every keystroke, and without the token a slow early request could land
 * after a fast later one and show results for a query the user has moved on
 * from.
 */
let requestToken = 0;

const reload = async () => {
  const token = ++requestToken;
  loading.value = true;
  try {
    const page = await local.page({
      keyword: keyword.value.trim() || undefined,
      sort: sort.value,
      descending: descending.value,
      offset: 0,
      limit: PAGE_SIZE,
    });
    if (token !== requestToken) return;
    songs.value = page.songs;
    total.value = page.total;
  } finally {
    if (token === requestToken) loading.value = false;
  }
};

const loadMore = async () => {
  if (loading.value || songs.value.length >= total.value) return;
  const token = ++requestToken;
  loading.value = true;
  try {
    const page = await local.page({
      keyword: keyword.value.trim() || undefined,
      sort: sort.value,
      descending: descending.value,
      offset: songs.value.length,
      limit: PAGE_SIZE,
    });
    if (token !== requestToken) return;
    songs.value = songs.value.concat(page.songs);
    total.value = page.total;
  } finally {
    if (token === requestToken) loading.value = false;
  }
};

const importDirectory = async () => {
  await local.importDirectory();
  await reload();
};

const importFiles = async () => {
  await local.importFiles();
  await reload();
};

// Debounced so a filter keystroke does not become an IPC round trip. Deliberately
// a local timer: `utils/debounce.ts` shares one module-level handle app-wide, so
// using it here would cancel whatever else happened to be pending.
let filterTimer: ReturnType<typeof setTimeout> | undefined;
watch(keyword, () => {
  clearTimeout(filterTimer);
  filterTimer = setTimeout(reload, 220);
});
watch([sort, descending], reload);
// `trackCount` covers a scan; `revision` covers an edit made on a detail page,
// which leaves the count alone. This view is kept alive, so without the second
// one navigating back shows the cached rows forever.
watch([() => local.trackCount, () => local.revision], reload);

onMounted(() => {
  $setSiteTitle(t("sidebar.localMusic") + " - " + t("local.tab.songs"));
  reload();
});

onBeforeUnmount(() => clearTimeout(filterTimer));
</script>

<style lang="scss" scoped>
/*
  与 `views/PlayList/PlayListView.vue` 的 `.right` 逐条一致。

  用 `:deep(.datalists)` 而不是给 `<DataLists>` 挂一个全局 class：那个组件的根是
  `<Transition>`，class 透传落在 Transition 组件上、**到不了**里面的 `.datalists`，
  所以挂 class 那条路是静默失效的 —— 行会保持默认的卡片样式（带间距、大圆角）。
  歌单页/专辑页一直用的就是 `:deep`，这里照抄。
*/
.local-songs {
  :deep(.datalists) {
    // 列宽的唯一出处：`.songs` 的行和 `.song-list-head` 的列头都读这组变量，
    // 所以不可能出现「改了行没改列头」的错位。
    --song-lead-size: 38px;
    --song-lead-gap: 14px;
    --song-action-width: 76px;
    --song-time-width: 46px;
    --song-name-flex: 1.6;
    --song-album-flex: 1;
    --song-row-padding-x: 12px;

    --detail-song-list-radius: var(--radius-md);

    margin-top: 2px;
  }

  :deep(.datalists .songs) {
    --n-color: transparent;
    --n-border-color: transparent;

    margin-bottom: 0;
    border: 0;
    border-radius: 0;
    background-color: transparent;
    box-shadow: none;
  }

  // 只用 `song-row-*` 类，不用 `:nth-child`：窗口模式下行被包在
  // `.song-plain-list` 里且前面有一个占位块，`:nth-child` 的奇偶会整体错位，
  // 而这些类是按**绝对下标**打的，滚到哪里都对。
  :deep(.datalists .songs.song-row-odd) {
    background-color: color-mix(in srgb, var(--n-text-color) 3%, transparent);
  }

  :deep(.datalists .songs.song-row-even) {
    background-color: color-mix(in srgb, var(--n-text-color) 6%, transparent);
  }

  :deep(.datalists .songs.song-row-first) {
    border-radius: var(--detail-song-list-radius) var(--detail-song-list-radius) 0 0;
  }

  :deep(.datalists .songs.song-row-last) {
    border-radius: 0 0 var(--detail-song-list-radius) var(--detail-song-list-radius);
  }

  :deep(.datalists .songs.song-row-single) {
    border-radius: var(--detail-song-list-radius);
  }

  :deep(.datalists .songs:hover) {
    background-color: color-mix(in srgb, var(--n-text-color) 10%, transparent);
    box-shadow: none;
  }

  :deep(.datalists .songs.play) {
    background-color: color-mix(in srgb, var(--main-color) 13%, transparent);
  }

  :deep(.datalists .songs .n-card__content) {
    min-height: 52px;
    padding: 8px var(--song-row-padding-x) !important;
  }

  :deep(.datalists .songs .pic),
  :deep(.datalists .songs .num) {
    border-radius: var(--radius-sm);
    font-size: 13px;
  }

  :deep(.datalists .songs .name .title) {
    font-size: 14px;
  }

  :deep(.datalists .songs .name .meta) {
    font-size: 12px;
  }

  :deep(.datalists .songs .album) {
    font-size: 13px;
    opacity: 0.72;
  }

  :deep(.datalists .songs .time) {
    font-size: 12px;
    opacity: 0.64;
  }

  // 悬浮工具栏，与歌单页同一套：整条透明、只有控件有玻璃底，空白处放行点击。
  .list-toolbar {
    position: sticky;
    // 钉在 Nav 下沿，由 shell 提供（`App.vue` 的 `--content-sticky-top`）。
    top: var(--content-sticky-top, 0px);
    z-index: 3;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    margin: 2px 0 10px;
    pointer-events: none;

    > * {
      pointer-events: auto;
    }
  }

  .list-search,
  .list-sort,
  .list-order {
    // 填充掺封面强调色、也比 Nav 那份更实：这一处的 `backdrop-filter` 静止时无事可做
    // （背后是不透明近白的面板底），层次只能由填充自己给。完整理由见
    // `views/PlayList/PlayListView.vue` 同名 token 块。本页不走 `useContentPanelAccent`，
    // 所以强调色会回退成中性白，只有「更实一点」这一半生效。
    --list-search-tint: 12%;
    --list-search-base: rgba(255, 255, 255, 0.72);
    --list-search-bg: color-mix(
      in srgb,
      rgb(var(--content-panel-accent-rgb, 255, 255, 255)) var(--list-search-tint),
      var(--list-search-base)
    );
    --list-search-border: rgba(0, 0, 0, 0.06);

    border-radius: var(--radius-pill);
    background-color: var(--list-search-bg);
    box-shadow:
      0 8px 22px rgb(0 0 0 / 10%),
      inset 0 1px 0 rgb(255 255 255 / 24%);
    -webkit-backdrop-filter: blur(18px) saturate(160%);
    backdrop-filter: blur(18px) saturate(160%);

    :deep(.n-input__border),
    :deep(.n-input__state-border) {
      border: 1px solid var(--list-search-border);
      border-radius: var(--radius-pill);
    }
  }

  .list-search {
    width: 170px;
    transition: width var(--duration-300) var(--ease-out);

    &:focus-within,
    &.has-value {
      width: min(300px, 100%);
    }
  }

  .list-sort {
    width: 116px;
    flex-shrink: 0;

    // n-select 的可见填充不在根上：`.n-base-selection-label` 自己画 naive 的
    // `--n-color`（亮色主题是纯白），圆角从 `.n-base-selection` 继承的 3px。上面那层
    // 玻璃因此被整块盖住，而白底又会从被改成药丸的描边四角漏出来——和 n-input 之前那处
    // 是同一个症状。naive 的 `--n-*` 是内联写在组件根元素上的、样式表覆盖不掉，所以只能
    // 覆盖读它们的属性本身。`--n-color-active`（聚焦/展开时的填充）也是同一个白，这里的
    // 选择器压得过 `.n-base-selection--active` 那条，一并盖掉。
    :deep(.n-base-selection),
    :deep(.n-base-selection-label) {
      background-color: transparent;
      border-radius: var(--radius-pill);
    }

    // 描边比 n-input 那份多带一层 `.n-base-selection`：naive 的 hover 规则
    // （`.n-base-selection:not(--disabled):hover .n-base-selection__state-border`）
    // 和四个类的写法同权重，同权重就靠注入顺序决胜，而 naive 是运行时挂载样式的、永远在
    // 打包 CSS 之后——不提权的话一 hover 药丸描边就跳回 naive 的默认色。
    :deep(.n-base-selection .n-base-selection__border),
    :deep(.n-base-selection .n-base-selection__state-border) {
      border: 1px solid var(--list-search-border);
      border-radius: var(--radius-pill);
    }
  }

  .list-order {
    flex-shrink: 0;
  }

  .empty {
    display: flex;
    justify-content: center;
    padding: 60px 0;
  }

  @media (max-width: 768px) {
    .list-toolbar {
      margin: 0 0 8px;
    }

    .list-search {
      flex: 1;
      width: auto;

      &:focus-within,
      &.has-value {
        width: auto;
      }
    }

    :deep(.datalists) {
      --song-lead-size: 42px;
      --song-lead-gap: 11px;
      --song-row-padding-x: 6px;
    }

    :deep(.datalists .songs) {
      margin-bottom: 0;
      border-radius: 0;
    }

    :deep(.datalists .songs .n-card__content) {
      min-height: 58px;
      padding: 9px var(--song-row-padding-x) !important;
    }

    :deep(.datalists .songs .name) {
      padding-right: 8px;
    }

    :deep(.datalists .songs .album),
    :deep(.datalists .songs .time) {
      display: none;
    }
  }
}

html[data-theme="dark"] .local-songs {
  .list-search,
  .list-sort,
  .list-order {
    --list-search-base: rgba(24, 24, 24, 0.62);
    --list-search-border: rgba(255, 255, 255, 0.11);
  }
}
</style>
