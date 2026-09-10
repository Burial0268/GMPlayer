<template>
  <div :class="['local-playlist', { 'is-dark': setting.getSiteTheme === 'dark' }]">
    <div class="left">
      <RouteArtwork v-slot="{ imageProps }" class="cover" data-navigation-cover="page">
        <n-image
          show-toolbar-tooltip
          class="coverImg"
          :img-props="imageProps"
          :src="coverUrl"
          :previewed-img-props="{ style: { borderRadius: 'var(--radius-md)' } }"
          :preview-src="coverUrl"
          fallback-src="/images/pic/default.png"
        />
        <RouteShadow class="shadow" :src="coverUrl" />
      </RouteArtwork>
      <div class="meta">
        <div class="title">
          <span v-content-intro class="detail-kind">{{ typeLabel }}</span>
          <n-text class="name text-hidden" data-navigation-title="page">{{
            meta.name || $t("local.unknown")
          }}</n-text>
          <n-text v-content-intro class="creator">{{
            meta.description || $t("sidebar.localMusic")
          }}</n-text>
        </div>
        <div v-content-intro class="detail-stats">
          <div class="num">
            <n-icon :depth="3" :component="MusicList" />
            <n-text>{{ $t("local.trackCount", { count: meta.trackCount }) }}</n-text>
          </div>
          <div class="num" v-if="meta.totalDurationMs > 0">
            <n-icon :depth="3" :component="Time" />
            <n-text>{{ totalDuration }}</n-text>
          </div>
        </div>
        <n-space v-content-intro class="control">
          <n-button
            strong
            secondary
            round
            type="primary"
            :disabled="!songs.length"
            @click="playAll"
          >
            <template #icon>
              <n-icon :component="PlayOne" />
            </template>
            {{ $t("general.name.playAll") }}
          </n-button>
          <n-button v-if="canExport" strong secondary round @click="exportPlaylist">
            <template #icon>
              <n-icon :component="Download" />
            </template>
            {{ $t("local.exportM3u") }}
          </n-button>
        </n-space>
      </div>
    </div>
    <div v-content-intro class="right">
      <div class="meta">
        <n-text class="name">{{ meta.name || $t("local.unknown") }}</n-text>
        <n-text class="creator">
          <n-icon :depth="3" :component="MusicList" />
          {{ $t("local.trackCount", { count: meta.trackCount }) }}
        </n-text>
      </div>
      <div class="list-toolbar">
        <n-input
          class="list-search"
          :class="{ 'has-value': !!searchKeyword }"
          v-model:value="searchKeyword"
          clearable
          size="small"
          :placeholder="$t('general.name.filterInList')"
        >
          <!-- 搜索时后台还在补齐未加载的页，前缀图标就地变成 spinner —— 与歌单页同一处理。 -->
          <template #prefix>
            <n-spin v-if="isSearching && loading" :size="13" />
            <n-icon v-else :component="Filter" />
          </template>
        </n-input>
      </div>
      <DataLists
        :listData="displayData"
        :loading="loading"
        :capabilities="capabilities"
        :empty-text="isSearching ? $t('general.name.noSearchResult') : $t('local.noMatches')"
        show-header
        page-window
        :virtual-item-size="54"
        :virtual-threshold="40"
        :total-rows="displayTotalRows"
        @reach-end="loadMore"
        @remove-track="removeTrack"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { convertFileSrc } from "@tauri-apps/api/core";
import { Download, Filter, MusicList, PlayOne, Time } from "@icon-park/vue-next";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { musicStore, settingStore, useLocalLibraryStore } from "@/store";
import { fuzzyFilterSongs } from "@/utils/fuzzySearch";
import { DEFAULT_COVER } from "@/utils/coverUrl";
import DataLists from "@/components/DataList/DataLists.vue";
import RouteShadow from "@/components/Navigation/RouteShadow.vue";
import RouteArtwork from "@/components/Navigation/RouteArtwork.vue";
import { useContentIntro } from "@/composables/useContentIntro";
import { localPlaylistExportM3u, localPlaylistRemoveTracks } from "@/utils/localLibrary";
import { capabilitiesFor, queryForRef, refFromQuery } from "@/utils/playlistSource";
import { isMobile } from "@/utils/tauri/platform/mobile";
import type { SongData } from "@/store/musicTypes";

/**
 * The detail view for every local collection.
 *
 * A separate view rather than a generalised `PlayListView.vue` on purpose. That
 * file is ~1600 lines, and a large share of it is compensation for Netease being
 * read-after-write inconsistent: a pending delta laid over the server's answer,
 * `MAX_RECONCILE_ATTEMPTS`, whole-entry replacement because rows are `markRaw`'d.
 * A local write is synchronous and authoritative, so none of that applies and
 * adopting it would introduce problems that do not exist here. What the two
 * genuinely share is `DataLists.vue`, and that is the expensive part.
 */
const PAGE_SIZE = 200;

const { t } = useI18n();
const route = useRoute();
const music = musicStore();
const setting = settingStore();
const local = useLocalLibraryStore();
const { vContentIntro } = useContentIntro();

const songs = ref<SongData[]>([]);
const loading = ref(false);
const meta = ref({ name: "", description: "", trackCount: 0, totalDurationMs: 0 });

// ── 列表内筛选 ──────────────────────────────────────────────
//
// 与歌单页/专辑页同一个交互：只在已加载的行里过滤，并在有关键词时把剩下的页也补齐。
// 本地库补页不花网络，只花一次 IPC，所以这里不需要歌单页那套防抖 + 让位的机制。
const searchKeyword = ref("");
const normalizedKeyword = computed(() => searchKeyword.value.trim());
const isSearching = computed(() => !!normalizedKeyword.value);

const displayData = computed(() =>
  isSearching.value ? fuzzyFilterSongs(songs.value, normalizedKeyword.value) : songs.value,
);

/** 搜索时总高按**过滤结果**算，否则按总行数——否则搜索态下会留一大截空白。 */
const displayTotalRows = computed(() =>
  isSearching.value ? displayData.value.length : meta.value.trackCount,
);

const playlistRef = computed(() => refFromQuery(route.query as Record<string, unknown>));
const capabilities = computed(() => capabilitiesFor(playlistRef.value));

const typeLabel = computed(() => {
  switch (playlistRef.value.kind) {
    case "local-album":
      return t("local.tab.albums");
    case "local-artist":
      return t("local.tab.artists");
    case "local-folder":
      return t("local.tab.folders");
    case "local-favourites":
      return t("local.favourites");
    case "local-playlist":
      return t("local.tab.playlists");
    default:
      return t("local.tab.songs");
  }
});

const coverUrl = computed(() => {
  const path = songs.value.find((song) => song.local?.coverPath)?.local?.coverPath;
  // Never the empty string — see the same computed in `LocalSongView.vue` for
  // why that draws the WebView's broken-image glyph rather than the fallback.
  return path ? convertFileSrc(path) : DEFAULT_COVER;
});

/**
 * `h:mm:ss` / `mm:ss` for a whole list.
 *
 * Not `getSongTime`, which formats a single track with `date-fns` and wraps past
 * one hour — an album is routinely longer than that, and `getLongTime` is a
 * *date* formatter despite the name.
 */
const totalDuration = computed(() => {
  const seconds = Math.round(meta.value.totalDurationMs / 1000);
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const rest = seconds % 60;
  const pad = (value: number) => String(value).padStart(2, "0");
  return hours > 0 ? `${hours}:${pad(minutes)}:${pad(rest)}` : `${minutes}:${pad(rest)}`;
});

/**
 * Export writes a file through the native save dialog, which mobile does not
 * have — writing a document through SAF would need `ACTION_CREATE_DOCUMENT`, and
 * this library only ever reads. Hidden rather than left to fail.
 */
const isMobilePlatform = ref(false);
const canExport = computed(() => capabilities.value.reorder && !isMobilePlatform.value);

let requestToken = 0;

const load = async (append = false) => {
  const query = queryForRef(playlistRef.value);
  if (!query) return;
  const token = ++requestToken;
  loading.value = true;
  try {
    const page = await local.page({
      ...query,
      offset: append ? songs.value.length : 0,
      limit: PAGE_SIZE,
    });
    if (token !== requestToken) return;
    songs.value = append ? songs.value.concat(page.songs) : page.songs;
    meta.value = {
      name: headerName(),
      description: headerDescription(),
      trackCount: page.total,
      // Only what has been loaded can be summed; the total row count is
      // authoritative but the durations of unfetched rows are not known. Shown
      // once everything is in, so it is either right or absent.
      totalDurationMs:
        songs.value.length >= page.total
          ? songs.value.reduce((sum, song) => sum + Number(song.dt ?? 0), 0)
          : 0,
    };
  } finally {
    if (token === requestToken) loading.value = false;
  }
};

const headerName = (): string => {
  const value = playlistRef.value;
  switch (value.kind) {
    case "local-album":
      return value.album;
    case "local-artist":
      return value.artist;
    case "local-folder":
      return value.folder || sourceName(value.sourceId);
    case "local-favourites":
      return t("local.favourites");
    case "local-playlist":
      return local.playlists.find((playlist) => playlist.id === value.id)?.name ?? "";
    default:
      return t("local.tab.songs");
  }
};

const headerDescription = (): string => {
  const value = playlistRef.value;
  if (value.kind === "local-playlist") {
    return local.playlists.find((playlist) => playlist.id === value.id)?.description ?? "";
  }
  if (value.kind === "local-folder") return sourceName(value.sourceId);
  return "";
};

const sourceName = (sourceId: string): string =>
  local.sources.find((source) => source.id === sourceId)?.displayName ?? "";

const loadMore = () => {
  if (loading.value || songs.value.length >= meta.value.trackCount) return;
  load(true);
};

/**
 * Typing in the filter pulls the rest of the list in.
 *
 * The filter runs over loaded rows only, so without this a search on a
 * partially-scrolled list would silently miss everything past the last page. One
 * `local_library_list` call per page and no network, so it can simply loop —
 * unlike the Netease path, which has to yield between chunks.
 */
watch(isSearching, async (searching) => {
  if (!searching) return;
  while (isSearching.value && songs.value.length < meta.value.trackCount) {
    const before = songs.value.length;
    await load(true);
    // A page that added nothing means the list is as complete as it will get;
    // continuing would spin forever on a stale total.
    if (songs.value.length <= before) break;
  }
});

// Each keyword reorders the results without changing the route.
watch(normalizedKeyword, (keyword, prev) => {
  if (keyword !== prev && typeof $scrollToTop !== "undefined") $scrollToTop();
});

const playAll = () => {
  if (!songs.value.length) return;
  music.setPlaylists(songs.value);
  music.addSongToPlaylists(songs.value[0], true);
};

/**
 * Take a row out of this playlist.
 *
 * Patched locally *and* written through, in that order — the write is
 * synchronous and authoritative, so unlike the Netease path there is nothing to
 * reconcile and no pending delta to carry. The row is dropped from the local
 * array rather than the page refetched, because a refetch at this offset would
 * pull in a row from the next page and shift everything the user is looking at.
 */
const removeTrack = async (song: SongData) => {
  const value = playlistRef.value;
  const key = song?.local?.uri;
  if (value.kind !== "local-playlist" || !key) return;
  try {
    const removed = await localPlaylistRemoveTracks(value.id, [key]);
    if (!removed) return;
    songs.value = songs.value.filter((row) => row.local?.uri !== key);
    meta.value = { ...meta.value, trackCount: Math.max(0, meta.value.trackCount - removed) };
    await local.refreshPlaylists();
  } catch (err) {
    $message.error(String(err));
  }
};

const exportPlaylist = async () => {
  const value = playlistRef.value;
  if (value.kind !== "local-playlist") return;
  try {
    const count = await localPlaylistExportM3u(value.id);
    // 0 means the save dialog was dismissed, which is not worth a toast.
    if (count > 0) $message.success(t("local.exported", { count }));
  } catch (err) {
    $message.error(String(err));
  }
};

watch(
  () => route.fullPath,
  () => {
    if (route.path === "/local/playlist") load();
  },
);
// The playlist name comes from the store, which may hydrate after the first
// paint; re-derive the header without refetching the rows.
watch(
  () => local.playlists,
  () => {
    meta.value = { ...meta.value, name: headerName(), description: headerDescription() };
  },
);
// An edit made on a track's detail page. This view is kept alive and loads from
// `onMounted`, so nothing else would ever re-read the corrected rows.
//
// Recorded rather than dropped when it lands while the page is hidden — which is
// the normal case, since the edit is made on `/local/song` — and paid on the next
// activation. Guarding on the route and returning would lose the change outright:
// `onMounted` does not run again for a cached instance.
let loadedRevision = local.revision;
let active = true;

watch(
  () => local.revision,
  () => {
    if (!active) return;
    loadedRevision = local.revision;
    void load();
  },
);

onActivated(() => {
  active = true;
  if (local.revision === loadedRevision) return;
  loadedRevision = local.revision;
  void load();
});
onDeactivated(() => {
  active = false;
});

onMounted(async () => {
  isMobilePlatform.value = await isMobile();
  await local.hydrate();
  await load();
  loadedRevision = local.revision;
  if (active) $setSiteTitle(`${meta.value.name} - ${t("sidebar.localMusic")}`);
});
</script>

<style lang="scss" scoped>
// 与 `views/PlayList/PlayListView.vue` / `views/Album/AlbumView.vue` 同一份布局。
// 本仓的详情页各带一份这套 `.left` / `.right` 样式（歌单、专辑、单曲现在都是），
// 所以这里照抄而不是抽公共 mixin；改其中一页的头部时其余几页要一起看。
.local-playlist {
  // 悬浮搜索控件的玻璃参数。抄自 Nav 的悬浮按钮，但 `--floating-control-bg` 定义在
  // `.nav` 内部拿不到，所以这里重新声明一份。
  //
  // 填充掺封面强调色、也比 Nav 那份更实：这一处的 `backdrop-filter` 静止时无事可做
  // （背后是不透明近白的面板底），层次只能由填充自己给。完整理由见
  // `views/PlayList/PlayListView.vue` 同名 token 块。本页没有走
  // `useContentPanelAccent`，所以 `--content-panel-accent-rgb` 会回退成中性白——只有
  // 「更实一点」这一半生效，等接上取色再自动跟着封面走。
  --list-search-tint: 12%;
  --list-search-base: rgba(255, 255, 255, 0.72);
  --list-search-bg: color-mix(
    in srgb,
    rgb(var(--content-panel-accent-rgb, 255, 255, 255)) var(--list-search-tint),
    var(--list-search-base)
  );
  --list-search-border: rgba(0, 0, 0, 0.06);

  &.is-dark {
    --list-search-base: rgba(24, 24, 24, 0.62);
    --list-search-border: rgba(255, 255, 255, 0.11);
  }

  @media (max-width: 768px) {
    --list-search-base: rgba(255, 255, 255, 0.8);
    --list-search-border: rgba(0, 0, 0, 0.07);

    &.is-dark {
      --list-search-base: rgba(32, 32, 38, 0.72);
      --list-search-border: rgba(255, 255, 255, 0.11);
    }
  }

  display: flex;
  flex-direction: column;
  gap: 22px;
  padding: 10px clamp(16px, 3vw, 36px) 36px;
  .left {
    width: 100%;
    min-height: 0;
    position: relative;
    display: grid;
    grid-template-columns: minmax(176px, 278px) minmax(0, 1fr);
    align-items: center;
    gap: clamp(22px, 4vw, 38px);
    // 底部收窄：头部块和列表之间原本叠了 gap + padding + 工具栏 margin，加起来是一条
    // 明显的死区。工具栏本身已经提供了呼吸。
    padding: 18px 2px 10px;

    .cover {
      position: relative;
      display: flex;
      align-items: center;
      justify-content: flex-start;
      width: 100%;
      aspect-ratio: 1 / 1;
      border-radius: var(--radius-md);

      &:active {
        transform: scale(0.95);
      }

      .coverImg {
        border-radius: var(--radius-md);
        width: 100%;
        height: 100%;
        overflow: hidden;
        z-index: 1;

        :deep(img) {
          width: 100%;
          height: 100%;
          object-fit: cover;
        }
      }

      .shadow {
        position: absolute;
        inset: 10px 0 0;
        height: 100%;
        width: 100%;
        filter: blur(18px) opacity(0.28);
        transform: scale(0.92, 0.94);
        z-index: 0;
        background-size: cover;
        aspect-ratio: 1/1;
      }
    }
    .meta {
      width: 100%;
      display: flex;
      flex-direction: column;
      justify-content: flex-end;
      min-width: 0;

      .n-text {
        color: inherit;
      }

      .title {
        display: flex;
        flex-direction: column;
        min-width: 0;
        margin-top: 0;

        .detail-kind {
          margin-bottom: 7px;
          font-size: 11px;
          font-weight: 700;
          line-height: 1;
          text-transform: uppercase;
          color: rgb(var(--content-panel-accent-rgb, 128, 128, 128));
        }

        .name {
          display: -webkit-box;
          max-width: min(780px, 100%);
          overflow: hidden;
          font-size: clamp(32px, 5vw, 56px);
          font-weight: 800;
          line-height: 1.06;
          overflow-wrap: anywhere;
          -webkit-box-orient: vertical;
          -webkit-line-clamp: 2;
          line-clamp: 2;
        }

        .creator {
          width: fit-content;
          margin-top: 10px;
          font-size: 15px;
          font-weight: 700;
          color: var(--n-text-color-2);
        }
      }

      .detail-stats {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px 14px;
        margin-top: 13px;
        color: var(--n-text-color-3);

        .num {
          display: flex;
          align-items: center;
          min-width: 0;
          font-size: 13px;

          .n-icon {
            flex: 0 0 auto;
            margin-right: 5px;
          }
        }
      }
      .control {
        margin-top: 16px;

        :deep(.n-button) {
          --n-color: rgba(var(--content-panel-button-rgb, 226, 154, 128), 0.86);
          --n-color-hover: rgb(var(--content-panel-button-rgb, 226, 154, 128));
          --n-color-pressed: rgba(var(--content-panel-button-rgb, 226, 154, 128), 0.74);
          --n-color-focus: rgba(var(--content-panel-button-rgb, 226, 154, 128), 0.92);
          --n-text-color: rgb(var(--content-panel-on-button-rgb, 18, 18, 22));
          --n-text-color-hover: rgb(var(--content-panel-on-button-rgb, 18, 18, 22));
          --n-text-color-pressed: rgb(var(--content-panel-on-button-rgb, 18, 18, 22));
          --n-text-color-focus: rgb(var(--content-panel-on-button-rgb, 18, 18, 22));
          --n-border: 1px solid rgba(var(--content-panel-button-rgb, 226, 154, 128), 0.24);
          --n-border-hover: 1px solid rgba(var(--content-panel-button-rgb, 226, 154, 128), 0.36);
          --n-border-pressed: 1px solid rgba(var(--content-panel-button-rgb, 226, 154, 128), 0.24);
          --n-border-focus: 1px solid rgba(var(--content-panel-button-rgb, 226, 154, 128), 0.38);

          min-width: 112px;
          height: 34px;
          border: 1px solid rgba(var(--content-panel-on-button-rgb, 18, 18, 22), 0.16);
          box-shadow:
            inset 0 1px 0 rgba(255, 255, 255, 0.26),
            inset 0 0 0 1px rgba(var(--content-panel-button-rgb, 226, 154, 128), 0.2),
            0 8px 18px rgba(var(--content-panel-accent-rgb, 0, 0, 0), 0.12);
          font-weight: 700;
        }

        :deep(.n-button .n-button__border),
        :deep(.n-button .n-button__state-border) {
          border-color: transparent !important;
        }
      }
    }
  }
  .right {
    width: 100%;
    min-width: 0;

    // 桌面端标题已经在封面右边了，这一份是移动端用的。
    .meta {
      display: none;
    }

    /*
      行样式与 `PlayListView.vue` 的 `.right` 逐条一致。

      用 `:deep(.datalists)` 而不是给 `<DataLists>` 挂 class：那个组件的根是
      `<Transition>`，class 透传到不了里面的 `.datalists`，挂 class 是静默失效的。
    */
    :deep(.datalists) {
      // 列宽的唯一出处：行和列头读同一组变量，不会各自漂移。
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

    // 只用 `song-row-*` 类，不用 `:nth-child`：窗口模式下行前面有占位块，
    // `:nth-child` 的奇偶会整体错位，而这些类是按绝对下标打的。
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

    // 搜索框锚在列表的**右**边缘（和时长列同一条竖线）。左边已经有封面和列表两条
    // 对齐线了，再在中间放一个小控件只会把断层放大。
    .list-toolbar {
      position: sticky;
      // 钉在 Nav 下沿，由 shell 提供（`App.vue` 的 `--content-sticky-top`）：钉 0 就是
      // 钉到 Nav 背后、`clip-path` 切口以外，移动端还会被顶部玻璃带洗白。
      top: var(--content-sticky-top, 0px);
      z-index: 3;
      display: flex;
      align-items: center;
      justify-content: flex-end;
      margin: 2px 0 10px;
      // 一条横跨整宽的透明 sticky 元素会把下面每一行的点击都吃掉。
      pointer-events: none;

      > * {
        pointer-events: auto;
      }
    }

    // 药丸形悬浮控件。常态收窄，聚焦或有内容时展开。
    .list-search {
      width: 170px;
      transition: width var(--duration-300) var(--ease-out);

      // 直接写在 `.list-search` 上而不是 `:deep(&.n-input)`：原因见
      // `views/PlayList/PlayListView.vue` 同名规则，后者编译出来是顶层带 `&` 的选择器，
      // 浏览器整条丢弃，naive 的白底 3px 圆角就从药丸描边的四角漏出来。
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

      &:focus-within,
      &.has-value {
        width: min(300px, 100%);
      }
    }
  }
  @media (max-width: 768px) {
    gap: 14px;
    padding: 8px 14px 28px;

    .left {
      min-height: 0;
      grid-template-columns: 1fr;
      align-items: start;
      gap: 16px;
      padding: 12px 0 18px;

      .cover {
        justify-self: center;
        width: min(58vw, 260px);
      }

      .meta {
        color: var(--n-text-color);

        .n-text {
          color: inherit;
        }

        .title {
          color: var(--n-text-color);

          .name {
            font-size: clamp(25px, 8vw, 36px);
            line-height: 1.12;
          }

          .creator {
            font-size: 15px;
            margin-top: 9px;
          }
        }

        .detail-stats {
          margin-top: 12px;
        }

        .control {
          margin-top: 16px;

          :deep(.n-button) {
            height: 38px;
          }
        }
      }
    }

    .right {
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

      // 窄屏上「靠右的窄输入框 + 左边一大片空」很怪，手指还要伸到角上。
      // 只改药丸宽度，不动 `justify-content`：这里原先还写了一条
      // `justify-content: stretch`，而 flex 容器里 `stretch` 按规范退化成
      // `flex-start`，既和基础规则的 `flex-end` 相矛盾又什么都没做——药丸已经是
      // `width: 100%`，主轴上没有余量可分。歌单页同名断点就只有这一条 margin。
      .list-toolbar {
        margin: 0 0 8px;
      }

      .list-search {
        width: 100%;

        &:focus-within,
        &.has-value {
          width: 100%;
        }
      }
    }
  }
}
</style>
