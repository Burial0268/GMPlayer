<template>
  <div :class="['playlist', { 'is-dark': setting.getSiteTheme === 'dark' }]" v-if="playListDetail">
    <div class="left">
      <RouteArtwork v-slot="{ imageProps }" class="cover" data-navigation-cover="page">
        <n-image
          show-toolbar-tooltip
          class="coverImg"
          :img-props="imageProps"
          :src="getCoverUrl(playListDetail.coverImgUrl, 1024)"
          :previewed-img-props="{ style: { borderRadius: 'var(--radius-md)' } }"
          :preview-src="getCoverUrl(playListDetail.coverImgUrl, 512)"
          fallback-src="/images/pic/default.png"
        />
        <RouteShadow class="shadow" :src="getCoverUrl(playListDetail.coverImgUrl, 1024)" />
      </RouteArtwork>
      <div class="meta">
        <div class="title">
          <span v-content-intro class="detail-kind">{{ t("general.name.playlist") }}</span>
          <n-text class="name text-hidden" data-navigation-title="page">{{
            playListDetail!.name
          }}</n-text>
          <n-text v-content-intro class="creator">{{ playListDetail!.creator.nickname }}</n-text>
        </div>
        <div v-content-intro class="detail-stats">
          <div class="num" v-if="playListDetail && playListDetail.createTime">
            <n-icon :depth="3" :component="Newlybuild" />
            <n-text v-html="getLongTime(playListDetail.createTime)" />
          </div>
          <div class="num" v-if="playListDetail && playListDetail.updateTime">
            <n-icon :depth="3" :component="Write" />
            <n-text v-html="getLongTime(playListDetail.updateTime)" />
          </div>
          <div class="num" v-if="totalCount">
            <n-icon :depth="3" :component="MusicList" />
            <n-text>{{ t("general.name.songSize", { size: totalCount }) }}</n-text>
          </div>
        </div>
        <div v-content-intro class="intr">
          <span class="name">{{
            t("general.name.desc", { name: t("general.name.playlist") })
          }}</span>
          <span class="desc text-hidden">
            {{
              playListDetail && playListDetail.description
                ? playListDetail.description
                : t("other.noDesc")
            }}
          </span>
          <n-button
            class="all-desc"
            block
            strong
            secondary
            v-if="
              playListDetail && playListDetail.description && playListDetail.description.length > 70
            "
            @click="playListDescShow = true"
          >
            {{ t("general.name.allDesc") }}
          </n-button>
        </div>
        <n-space v-content-intro class="tag" v-if="playListDetail && playListDetail.tags">
          <n-tag
            class="tags"
            round
            :bordered="false"
            v-for="item in playListDetail!.tags"
            :key="item"
            @click="router.push(`/discover/playlists?cat=${item}&page=1`)"
          >
            {{ item }}
          </n-tag>
        </n-space>
        <n-space v-content-intro class="control">
          <n-button strong secondary round type="primary" @click="playAllSong">
            <template #icon>
              <n-icon :component="MusicList" />
            </template>
            {{ t("general.name.play") }}
          </n-button>
          <n-dropdown
            placement="right-start"
            trigger="click"
            :show-arrow="true"
            :options="dropdownOptions"
          >
            <n-button strong secondary circle>
              <template #icon>
                <n-icon :component="More" />
              </template>
            </n-button>
          </n-dropdown>
        </n-space>
      </div>
    </div>
    <div v-content-intro class="right">
      <div class="meta">
        <n-text class="name">{{ playListDetail!.name }}</n-text>
        <n-text class="creator">
          <n-icon :depth="3" :component="People" />
          {{ playListDetail!.creator.nickname }}
        </n-text>
        <n-space class="time">
          <div class="num">
            <n-icon :depth="3" :component="Newlybuild" />
            <n-text
              v-if="playListDetail && playListDetail.createTime"
              v-html="getLongTime(playListDetail.createTime)"
            />
          </div>
          <div class="num">
            <n-icon :depth="3" :component="Write" />
            <n-text
              v-if="playListDetail && playListDetail.updateTime"
              v-html="getLongTime(playListDetail.updateTime)"
            />
          </div>
        </n-space>
      </div>
      <div class="list-toolbar">
        <n-input
          class="list-search"
          :class="{ 'has-value': !!searchKeyword }"
          v-model:value="searchKeyword"
          clearable
          size="small"
          :placeholder="t('general.name.filterInList')"
        >
          <!--
            搜索时后台还在补齐未 hydrate 的块，前缀图标就地变成 spinner——用户的视线
            本来就在这儿，比在列表下方另起一行进度文字更省地方也更直接。
          -->
          <template #prefix>
            <n-spin v-if="isSearching && isHydrating" :size="13" />
            <n-icon v-else :component="Filter" />
          </template>
        </n-input>
      </div>
      <DataLists
        :listData="displayData"
        page-window
        :total-rows="displayTotalRows"
        :virtual-item-size="54"
        :virtual-threshold="40"
        show-header
        :loading="isHydrating"
        :empty-text="isSearching ? t('general.name.noSearchResult') : ''"
        @reach-end="hydrateMore"
      />
      <!-- 歌单简介 -->
      <n-modal
        class="s-modal"
        v-model:show="playListDescShow"
        preset="card"
        :title="t('general.name.desc', { name: t('general.name.playlist') })"
        :bordered="false"
      >
        <n-scrollbar v-if="hasPlaylistDescription">
          <n-text v-html="playlistDescriptionHtml" />
        </n-scrollbar>
      </n-modal>
    </div>
  </div>
  <div class="title" v-else-if="!playListId">
    <span class="key">{{ t("general.name.noKeywords") }}</span>
    <br />
    <n-button strong secondary @click="navigation.closeTop()" style="margin-top: 20px">
      {{ t("general.name.goBack") }}
    </n-button>
  </div>
  <PageLoadState v-else-if="!loadingState" error @retry="retryPlaylist" />
  <DetailPageSkeleton v-else kind="playlist" />
</template>

<script setup lang="ts">
import DetailPageSkeleton from "@/components/Navigation/DetailPageSkeleton.vue";
import RouteArtwork from "@/components/Navigation/RouteArtwork.vue";
import PageLoadState from "@/components/Navigation/PageLoadState.vue";
import RouteShadow from "@/components/Navigation/RouteShadow.vue";
import { useContentIntro } from "@/composables/useContentIntro";
import { useLayerNavigation } from "@/utils/navigation";
import type { DropdownMixedOption } from "naive-ui/es/dropdown/src/interface";
import { NIcon, NText } from "naive-ui";
import { delPlayList, likePlaylist } from "@/api/playlist";
import { fetchPlaylistDetail, fetchSongDetail } from "@/utils/ncm/projectedRequest";
import { useRouter, useRoute } from "vue-router";
import { userStore, musicStore, settingStore } from "@/store";
import { getLongTime } from "@/utils/timeTools";
import { transformSongData } from "@/utils/ncm/transformSongData";
import { asRawEntry } from "@/utils/rawEntry";
import { onPlaylistChanged, type PlaylistChange } from "@/utils/playlistMutations";
import { fuzzyFilterSongs } from "@/utils/fuzzySearch";
import { renderIcon } from "@/utils/ui/renderIcon";
import { buildLikeMessage } from "@/utils/ui/buildLikeMessage";
import { usePlayAllSong } from "@/composables/usePlayAllSong";
import { useDownloadSongs } from "@/composables/useDownloadSongs";
import { useContentPanelAccent } from "@/composables/useContentPanelAccent";
import {
  MusicList,
  LinkTwo,
  More,
  DeleteFour,
  DownloadFour,
  Like,
  Unlike,
  Newlybuild,
  Write,
  People,
  Filter,
} from "@icon-park/vue-next";
import { useI18n } from "vue-i18n";
import DataLists from "@/components/DataList/DataLists.vue";
import getCoverUrl from "@/utils/ncm/getCoverUrl";

const { t } = useI18n();
const router = useRouter();
const navigation = useLayerNavigation();
const { vContentIntro } = useContentIntro();
const route = useRoute();
const user = userStore();
const music = musicStore();
const setting = settingStore();
const { playAllSong: playAll } = usePlayAllSong();
const { enqueue: downloadSongs } = useDownloadSongs();
const { applyContentPanelAccent } = useContentPanelAccent();

// 歌单数据
const playListId = ref<string | number | string[] | undefined>(
  route.query.id as string | number | string[] | undefined,
);

interface PlaylistCreator {
  nickname: string;
}

interface PlaylistDetail {
  id: number;
  name: string;
  coverImgUrl: string;
  creator: PlaylistCreator;
  description?: string;
  tags?: string[];
  createTime?: number;
  updateTime?: number;
}

const playListDetail = ref<PlaylistDetail | null>(null);
/**
 * 已 hydrate 的行。
 *
 * `shallowRef` 而非 `ref`：写入永远是**整体替换**（追加一块、重排 num、采用服务端
 * manifest），从来不是就地改某一项，所以深层响应式在这里没有任何用处，只有代价。
 * 而代价是按元素数量算的：`ref` 会把数组包成 reactive proxy，此后每一次索引读取
 * 都过一遍 handler——万首歌单遍历一遍就是 2.2 ms（shallowRef 0.3 ms）。这条路径
 * 每次搜索过滤、每次窗口重算都要走，`DataLists` 里的 `slice`/`map` 也一样。
 *
 * 条目本身已经是 `markRaw` 的（见 `utils/rawEntry`），所以这只是把「数组这一层」
 * 也从代理里摘出来，语义上和条目那一层是同一个决定。
 */
const playListData = shallowRef<unknown[]>([]);
const playListDescShow = ref(false);
const loadingState = ref(true);
const totalCount = ref(0);

// ── 长流：manifest + 分块 hydrate ────────────────────────────
//
// `/playlist/detail` 返回的 `trackIds` 是**完整**的，而 `tracks` 只有前一段（服务
// 端截到 1000，见 `seedFromDetail`），上游文档给的做法就是「拿全部 trackIds 请求
// song/detail」。所以这里把列表拆成两层：manifest 是有序 id 全集，一次取到；行数据
// 按可视区间分块 hydrate，密集且连续地追加。
//
// 这样做同时更省：旧的分页走 `/playlist/track/all`，而它内部**每翻一页都重新拉
// 一次完整 manifest**（`n: 1e5`，实测 247 KB）只为切出 30 个 id。现在 manifest
// 只取一次，之后每块只花一次 `song_detail` —— 而 song_detail 会被 ncm-core 的
// batch 层按 id 合并、TTL 30 分钟、持久化 7 天 stale。
/**
 * 有序 id 全集。同样用 `shallowRef`：这是一个上万元素的数字数组，只整体替换。
 *
 * `ref` 包出来的代理会让 `slice`（每次补块）和 `filter`（每次打补丁）逐元素过
 * handler，而里面一个数字都不需要被单独追踪。
 */
const manifestIds = shallowRef<number[]>([]);
const isHydrating = ref(false);

/**
 * 一块的大小。
 *
 * `song/detail` 一次就吃得下 1000 个 id。实测过整张 10379 首的歌单：11 次请求全部
 * 取回，每一块返回的条数、顺序都和请求的 id 列表逐条一致，一首不少。原来按 100
 * 分块等于把 round trip 数乘了 10——一个万首歌单要 104 次**串行**请求才补得齐，
 * 搜索时的整表补齐因此慢到不可用，滚动加载也永远追不上手指。AGENTS.md 的结论在这
 * 里照样成立：一次调用的成本就是它那个 round trip，唯一值得优化的是请求**次数**。
 *
 * 1000 不是拍脑袋的上限：ncm-core 的磁盘缓存单条上限是 4 MB（`disk.rs`
 * `MAX_ENTRY_BYTES`），而 1000 首的 `song_detail` 响应实测约 2.06 MB，仍在里面。
 * 注意缓存存的是**完整**信封，所以这个上限跟投影无关——`fetchSongDetail` 只是把
 * 过 IPC 的那一份裁到 0.36 MB。
 */
const HYDRATE_CHUNK = 1000;

/**
 * 换歌单/重载时自增，作废在途的 hydrate。
 *
 * 分块加载是多次异步追加，而 `playListData` 是共享的——没有这个令牌，上一个歌单
 * 迟到的那一块会被追加到新歌单的列表尾巴上。
 */
let loadToken = 0;

/**
 * 已经向 manifest **请求**到哪了。
 *
 * 必须和 `playListData.length`（已渲染行数）分开：`song_detail` 不会返回下架/不可用
 * 的曲目，两者一旦分叉，用行数当下标就会反复重取同一段。
 */
const hydrateCursor = ref(0);

const hasPlaylistDescription = computed(
  () => !!playListDetail.value && !!playListDetail.value.description,
);

const playlistDescriptionHtml = computed(
  () => playListDetail.value?.description?.replace(/\n/g, "<br>") ?? "",
);

const normalizePlaylistId = (id: string | number | string[]) =>
  Number(Array.isArray(id) ? id[0] : id);

// 判断收藏还是取消
const isLikeOrDislike = (id: string | number | string[]) => {
  return !user.getLikedPlayListIds.has(Number(id));
};

// 判断是否可删除
const isCanDelete = (id: string | number | string[]) => {
  return user.getOwnPlayListIds.has(Number(id));
};

// 歌单下拉菜单数据
const dropdownOptions = ref<DropdownMixedOption[]>([]);

// 更改歌单下拉菜单数据
const setDropdownOptions = () => {
  dropdownOptions.value = [
    {
      key: "copy",
      label: t("menu.copy", {
        name: t("general.name.playlist"),
        other: t("general.name.link"),
      }),
      props: {
        onClick: () => {
          if (navigator.clipboard) {
            try {
              navigator.clipboard.writeText(
                `https://music.163.com/#/playlist?id=${playListId.value}`,
              );
              $message.success(t("general.message.copySuccess"));
            } catch (err) {
              console.error(t("general.message.copyFailure"), err);
              $message.error(t("general.message.copyFailure"));
            }
          } else {
            $message.error(t("general.message.notSupported"));
          }
        },
      },
      icon: renderIcon(h(LinkTwo) as any),
    },
    {
      key: "downloadAll",
      // 「已加载的行」而不是整张歌单：一个上万首的歌单没有「全部下载」这种意思，而且
      // 分块 hydrate 还没走到的行这里根本没有 id。
      //
      // 因此 `show` 只看登录态：`setDropdownOptions` 在 onMounted 就跑了，那时第一块
      // 还没落地，按行数判断等于永远隐藏。行数在点击那一刻才读，空列表由
      // `useDownloadSongs` 自己说明。
      label: t("download.downloadAll"),
      show: user.userLogin,
      props: {
        onClick: () => {
          void downloadSongs(playListData.value as any[]);
        },
      },
      icon: renderIcon(h(DownloadFour) as any),
    },
    {
      key: "del",
      label: t("menu.del"),
      show: user.userLogin && isCanDelete(playListId.value),
      props: {
        onClick: () => {
          toDelPlayList(playListDetail.value);
        },
      },
      icon: renderIcon(h(DeleteFour) as any),
    },
    {
      key: "like",
      label: isLikeOrDislike(playListId.value)
        ? t("menu.collection", { name: t("general.name.playlist") })
        : t("menu.cancelCollection", { name: t("general.name.playlist") }),
      show: user.userLogin && !isCanDelete(playListId.value),
      props: {
        onClick: () => {
          toChangeLike(playListId.value);
        },
      },
      icon: renderIcon(h(isLikeOrDislike(playListId.value) ? Like : Unlike) as any),
    },
  ];
};

/**
 * 取歌单详情 + manifest，并 hydrate 第一块。
 *
 * 详情和 manifest 来自**同一个** `/playlist/detail`。旧实现在 onMounted 里并发
 * 调 `getPlayListDetailData` 和 `getAllPlayListData`，而后者内部又拉一次同参数的
 * `/api/v6/playlist/detail` —— 两者在 ncm-core 里是不同 endpoint 名、不同缓存
 * 键，`inflight` 合并不了，所以每次开歌单页都把那份 247 KB 下载了两遍。
 *
 * 走 `fetchPlaylistDetail` / `fetchSongDetail` 而不是 `getPlayListDetail` /
 * `getMusicDetail`：同一个端点、同一个缓存条目，只是让 Rust 先把响应裁到这一页真正
 * 会读的字段再过 IPC。原始信封实测 2.4 MB，其中 79% 没有任何人读，而 `JSON.parse`
 * 是这条链上**唯一**跑在绘制线程上的一步。
 */
const loadPlaylist = (id: string | number | string[], minRows = 0) => {
  const sourceId = normalizePlaylistId(id);
  const token = ++loadToken;
  loadingState.value = true;
  // 这是「明确要一份权威数据」的路径（进页面、换歌单），本地那些还没对完账的
  // 增删到此为止：留着只会被套用到另一份列表上。
  resetPendingDelta();
  manifestIds.value = [];
  playListData.value = [];
  hydrateCursor.value = 0;
  // 在**发出请求时**记账，不等它回来：这个时间戳的语义是「问到哪一刻为止」，而
  // `onActivated` 首次挂载就会跑一次，那时首屏请求还在路上——用响应时间会让它当场
  // 判定为过期，再白发一次。
  lastSyncedAt = Date.now();
  fetchPlaylistDetail(sourceId)
    .then((res) => {
      if (token !== loadToken) return;
      const pl = res?.playlist;
      if (!pl) {
        loadingState.value = false;
        if (isActive) $message.error(t("general.message.acquisitionFailed"));
        return;
      }
      totalCount.value = pl.trackCount;
      playListDetail.value = pl;
      manifestIds.value = extractManifestIds(pl);
      applyContentPanelAccent(getCoverUrl(pl.coverImgUrl, 256));
      if (isActive) $setSiteTitle(pl.name + " - " + t("general.name.playlist"));
      seedFromDetail(pl, sourceId);
      void hydrateUntil(Math.max(minRows, HYDRATE_CHUNK));
    })
    .catch((err) => {
      if (token !== loadToken) return;
      loadingState.value = false;
      console.error(t("general.message.acquisitionFailed"), err);
      if (isActive) {
        $setSiteTitle(t("general.name.playlist"));
        $message.error(t("general.message.acquisitionFailed"));
      }
    });
};

/**
 * `trackIds` 的每一项只有 id 是我们要的身份。
 *
 * 两种形状都要认。`ncm_request_projected` 已经在 Rust 侧把它压成 `number[]`（见
 * `src-tauri/src/ncm/projection.rs` 的 `flatten_track_ids`）——上游那 14 个字段里
 * 13 个从来没人读，而一万首的歌单光这一段就是 1.6 MB 的 JSON；但 remote 传输和
 * Web 根本不经过那条路，拿回来的仍是原始的 `[{id, v, t, …}]`。
 */
const extractManifestIds = (pl: any): number[] => {
  if (!Array.isArray(pl?.trackIds)) return [];
  return pl.trackIds
    .map((entry: any) => Number(entry !== null && typeof entry === "object" ? entry.id : entry))
    .filter(Boolean);
};

/**
 * 用 `/playlist/detail` 自己带回来的 `tracks` 铺开头那一段。
 *
 * 这一段是**白送的**：同一个响应里已经含着前 1000 首的完整曲目对象（实测与
 * `trackIds` 的前缀逐条同序、同 id），字段也正是 `transformSongData` 要的那几个。
 * 旧写法把它整个丢掉，转头再用 `song_detail` 把前 100 首重新请求一遍——一次纯粹
 * 多余的 round trip，而且首屏还只有 100 行。
 *
 * 对 1000 首以内的歌单——也就是绝大多数——铺完这一段就已经是**全部**，之后一个
 * 请求都不必再发，列表从第一帧起就是完整的。
 */
const seedFromDetail = (pl: any, sourceId: number) => {
  const tracks: any[] = Array.isArray(pl?.tracks) ? pl.tracks : [];
  const ids = manifestIds.value;
  if (!tracks.length || !ids.length) return;

  // 只采信与 manifest 前缀真正对得上的那一段（最长公共前缀）。对不上就停在那里，
  // 剩下的交给 `song_detail`：顺序一旦错位，行号和后续分块的起点会跟着一起错，
  // 代价远大于多发一次请求。
  const max = Math.min(tracks.length, ids.length);
  let n = 0;
  while (n < max && Number(tracks[n]?.id) === ids[n]) n++;
  if (!n) return;

  playListData.value = transformSongData(tracks.slice(0, n), { offset: 0, sourceId });
  hydrateCursor.value = n;
};

/**
 * 同一次加载里在途的那一块。
 *
 * `hydrateUntil`（搜索时的整表补齐）和 `reach-end`（滚到底）会在同一瞬间都想要下
 * 一块。旧写法用一个 `isHydrating` 布尔把后来者挡回去并返回 `false`，而
 * `hydrateUntil` 把 `false` 读成「没有更多了」就 `break` —— 于是补齐只要撞上一次
 * 在途请求，整表补齐就**永久停在半路**，搜索从此只在已加载的那一段里过滤。同一个
 * 洞还解释了连点两个歌单时新歌单一行都不出来：新的那次补齐撞上旧歌单的在途块，当
 * 场 break，而列表为空时 DataLists 整个 `v-if` 都不渲染，`reach-end` 再没机会响，
 * 页面就永远停在只有头部的状态。
 *
 * 现在改成共享同一个 promise，并且**按加载令牌隔离**：跨歌单不共享，否则旧块被令
 * 牌判死后返回的那个 `false` 会照样把新歌单的补齐打断。
 */
let inFlight: { token: number; promise: Promise<boolean> } | null = null;

/**
 * 按 manifest 顺序补足下一块。
 *
 * `song_detail` 不保证按请求顺序返回，所以这里按 id 重排后再交给
 * `transformSongData`——否则 `num` 会和实际行对不上。
 */
const hydrateMore = (): Promise<boolean> => {
  if (inFlight && inFlight.token === loadToken) return inFlight.promise;
  if (!playListId.value) return Promise.resolve(false);
  const ids = manifestIds.value.slice(hydrateCursor.value, hydrateCursor.value + HYDRATE_CHUNK);
  if (!ids.length) return Promise.resolve(false);

  const entry = { token: loadToken } as { token: number; promise: Promise<boolean> };
  entry.promise = fetchChunk(ids, entry.token).finally(() => {
    // 只有仍然是「当前那一块」才收尾。迟到的旧块不能把新块的状态清掉，否则新块的
    // 重入守卫会被提前解开，而 spinner 也会在还在加载时熄灭。
    if (inFlight === entry) {
      inFlight = null;
      isHydrating.value = false;
    }
  });
  inFlight = entry;
  isHydrating.value = true;
  return entry.promise;
};

/** 取一块并追加。返回「是否还值得继续要下一块」。 */
const fetchChunk = async (ids: number[], token: number): Promise<boolean> => {
  const sourceId = normalizePlaylistId(playListId.value!);
  try {
    const res = await fetchSongDetail(ids);
    if (token !== loadToken) return false;
    const songs: any[] = res?.songs ?? [];
    // 游标按**请求出去的 id 数**推进，不管回来几条。下架/不可用的曲目不会出现在
    // `songs` 里，若用返回条数推进，下一轮就会把它们再请求一遍，窗口每轮少走几格，
    // manifest 尾部永远到不了——而 `hydrateUntil` 的循环条件也就永远不成立。
    hydrateCursor.value += ids.length;
    if (!songs.length) return true;
    const byId = new Map(songs.map((s) => [Number(s.id), s]));
    const ordered = ids.map((id) => byId.get(id)).filter(Boolean);
    const rows = transformSongData(ordered, {
      offset: playListData.value.length,
      sourceId,
    });
    playListData.value = [...playListData.value, ...rows];
    return true;
  } catch (err) {
    console.error("[playlist] hydrate failed", err);
    return false;
  }
};

/** 连续补块直到至少有 `count` 行（用于 `?page=` 兼容与首屏）。 */
const hydrateUntil = async (count: number, keepGoing?: () => boolean) => {
  const token = loadToken;
  let first = true;
  // 用**游标**判终止，不用已渲染行数：两者会因为丢弃的曲目而分叉，拿行数当条件在
  // 有下架曲目的歌单上就是个死循环。
  while (
    token === loadToken &&
    hydrateCursor.value < manifestIds.value.length &&
    playListData.value.length < count
  ) {
    // 块与块之间让出主线程。投影之后一块过 IPC 的 JSON 是 0.36 MB（解析约 2.5 ms，
    // 投影前是 2.06 MB / 约 12 ms），但还要建 1000 个行对象，所以一块仍然是十几毫秒
    // 量级；十块连着跑照样是一次看得见的长任务。
    //
    // 而**缓存命中时恰恰最糟**：`song_detail` 有 30 分钟 TTL、7 天磁盘 stale，所以
    // 第二次搜同一个歌单时十块几乎同时返回——网络延迟本来是唯一在替我们隔开这些
    // 解析的东西，一旦没有了，整个补齐就退化成一个不可打断的长任务。
    if (!first) await yieldToBrowser();
    first = false;
    if (token !== loadToken) return;
    // 让位期间用户可能已经不需要整表了（清掉了搜索框）。这一步之前是没有的，于是
    // 敲一个字再退格，也会把整张万首歌单**下载并解析完**才停。
    if (keepGoing && !keepGoing()) return;
    if (!(await hydrateMore())) break;
  }
};

/**
 * 让出主线程一帧。
 *
 * rAF 里再套一个 `setTimeout(0)`：rAF 的回调在绘制**前**跑，挂在它后面的宏任务才
 * 落在这一帧提交之后，于是补齐的下一块不会和刚提交的那批行挤在同一个长任务里。
 *
 * 兜底的定时器是必须的：页面不可见时（最小化、切到托盘）rAF 根本不回调，补齐会
 * 就此停住并一直占着单飞标记。
 */
const yieldToBrowser = (): Promise<void> =>
  new Promise((resolve) => {
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      resolve();
    };
    requestAnimationFrame(() => setTimeout(finish, 0));
    setTimeout(finish, 32);
  });

/** manifest 全部请求过了。此后不必再催 `reach-end`。 */
const hydrationDone = computed(
  () => !!manifestIds.value.length && hydrateCursor.value >= manifestIds.value.length,
);

// ── 列表内搜索 ──────────────────────────────────────────────
//
// 只在本地过滤已 hydrate 的行，但一开始搜索就把剩下的块也补齐——**而且这不贵**：
// `song_detail` 的缓存键是整个参数集（`timestamp` 被 VOLATILE_KEYS 剥掉），所以
// 只要分块边界不变，滚过的块再问一次就是纯缓存命中，零网络；同一歌单 7 天内还能
// 从磁盘缓存命中。真正花钱的只有从没滚到过的那几块。
//
// 正因如此，补全必须复用 `hydrateMore` 的**同一个** `HYDRATE_CHUNK`。换成更大的
// 分块会生成一批全新的缓存键，把已经拿到的全部重下一遍——比不缓存还糟。
//
// 便宜的是**网络**，不是主线程。一块 1000 首是 1~2 MB JSON 要 parse 加 1000 个行
// 对象要建，约 25 ms；缓存全中时十一块会一口气回来，于是「零网络」反而意味着一次
// 250 ms 的连续长任务。所以补齐在块之间让位（见 `hydrateUntil`），并且这里只在
// 用户停下手时才启动——每敲一个键都驱动一次，会让上面那串解析和输入抢同一条线程。
const searchKeyword = ref("");
const normalizedKeyword = computed(() => searchKeyword.value.trim());
const isSearching = computed(() => !!normalizedKeyword.value);

const displayData = computed(() =>
  isSearching.value
    ? fuzzyFilterSongs(playListData.value, normalizedKeyword.value)
    : playListData.value,
);

/**
 * 搜索时总高按**过滤结果**算，否则按 manifest。
 *
 * 搜索态下 manifest 长度不再是会渲染的行数，拿它撑高会留一大截空白。
 */
const displayTotalRows = computed(() => {
  if (isSearching.value) return displayData.value.length;
  // 补齐完成后收敛到**实际**行数。manifest 里被丢弃的曲目不会有行，继续按 manifest
  // 长度撑高会在末尾留一段空白，`reach-end` 也会一直空转。
  return hydrationDone.value ? playListData.value.length : manifestIds.value.length;
});

/**
 * 整表补齐的启动延迟。
 *
 * 不用 `utils/debounce`：那是一个**模块级共享**的定时器，全 app 一个，别处一次防抖
 * 就会把这里的取消掉（AGENTS.md 里明说了这件事）。
 *
 * 220 ms 与本地音乐页的筛选防抖同一个数量级：短于一次连续输入的键间隔，又长到
 * 「打完一个词」只驱动一次。
 */
const SEARCH_HYDRATE_DELAY = 220;
let searchHydrateTimer: ReturnType<typeof setTimeout> | null = null;

watch(normalizedKeyword, (keyword, prev) => {
  // 换了关键词就回到列表顶部。过滤后的结果是一份全新的、按相关度重排的列表，停在
  // 原来的滚动位置没有任何意义——用户要看的第一条在最上面。
  if (keyword !== prev && typeof $scrollToTop !== "undefined") $scrollToTop();
  if (searchHydrateTimer) clearTimeout(searchHydrateTimer);
  // 补齐已经跑完就不必再驱动一遍：这个 watch 每敲一个键都会响，而 `hydrateUntil`
  // 在补齐完成后每次都要重新跑一遍循环条件才退出。
  if (!keyword || hydrationDone.value) return;
  searchHydrateTimer = setTimeout(() => {
    searchHydrateTimer = null;
    // 交给 `hydrateUntil` 一个「还要不要继续」的判据：整表补齐横跨很多次让位，
    // 期间用户完全可能已经清掉了搜索框，那剩下的块就没人要了。
    void hydrateUntil(manifestIds.value.length, () => isSearching.value);
  }, SEARCH_HYDRATE_DELAY);
});

// 播放歌单所有歌曲
const playAllSong = () => {
  playAll(playListData.value);
};

// ── 歌单写入后的就地更新 ──────────────────────────────────────
//
// 歌单页是 keep-alive 的，路由再次进入时 watch 会重新拉取，所以真正会停在错误
// 状态的只有「页面开着的时候」：在这里取消喜欢、在播放器或通知栏点红心、把歌加
// 进歌单，行都不会动，总数也不会变。
//
// 先就地改（无请求、无闪烁），再安静对账（补上只有服务端知道的插入位置、分页
// 边界和真实总数）。顺序不能反：网易对 /playlist/track/all 是写后读不一致的，
// 紧跟着 /like 拉回来的可能还是写入前的列表 —— 先对账会把刚删掉的行又放回来，
// 比不更新更糟。所以 pending 增删会一直盖在服务端结果之上，直到服务端认账。

/** 等网易把写入落盘。太短会拿到旧列表，太长则总数看着不对。 */
const RECONCILE_DELAY = 1200;
/** 超过这个次数还对不上，更可能是我们猜错了，而不是服务端慢 —— 以服务端为准。 */
const MAX_RECONCILE_ATTEMPTS = 3;

/**
 * 回到本页时，隔多久就重新问一次服务端。
 *
 * 这一条是 keep-alive 的必然代价。路由 watch 只在 **id 变了** 时重载（见文件末尾那
 * 段注释：判据一旦放宽成「和上一个路由不一样」，每次回到已缓存的歌单页都会把几千行
 * hydrate 进度和滚动位置一起丢掉），而 `<keep-alive :max="10">` 会把这个实例连着它
 * 的数据留着。两者合起来的结果是：**再次进入同一个歌单，不会有任何一次请求**——在
 * 别处（网页版、手机、另一个窗口）加进来的歌，回到这一页永远看不到。
 *
 * 所以补一条按时间的重新校验，走的是和写后对账**同一条**安静路径
 * （`reconcileQuietly` → `adoptManifest`）：不显示 loading、不回顶、已 hydrate 的
 * 前缀原地保留，只有成员和顺序以服务端为准。代价是一次 `playlist_detail`。
 *
 * 5 分钟＝`TTL_LIST`（`ncm-core` 的 `cache.rs`）。短于它的话，这次请求本来就会被缓
 * 存命中，等于白跑一趟 IPC 却什么都不会变；长于它则是自己给自己留一段说不清的窗口。
 */
const REVALIDATE_AFTER = 5 * 60 * 1000;

const pendingRemoved = new Set<number>();
const pendingAdded = new Set<number>();
let reconcileTimer: ReturnType<typeof setTimeout> | null = null;
let reconcileAttempts = 0;
/**
 * keep-alive 隐藏时不对账，改为记账，回到本页时补上。
 *
 * 原先这里直接丢掉，理由写的是「回到本页时路由 watch 本来就会重拉」—— 那句话在
 * 路由 watch 改成只认 id 变化之后就不再成立了。于是隐藏期间发生的写入只剩本地补丁：
 * `pendingRemoved` / `pendingAdded` 永远对不完账，`totalCount` 停在猜的那个数，
 * 而且没有任何东西会再问一次。
 */
let isActive = true;
/** 有一次对账因为页面不可见而没做。 */
let reconcileOwed = false;
/**
 * 上一次**向服务端问过**这个歌单的时刻。
 *
 * 在 `loadPlaylist` 入口就写，而不是等响应回来：语义是「问到哪一刻为止」，而且
 * `onActivated` 在首次挂载时也会跑（Vue 里 mounted 之后紧接着就是 activated），
 * 那时首屏请求还在路上，用响应时间会让它当场判定为「过期」再多发一次。
 */
let lastSyncedAt = 0;

const rowId = (row: unknown): number => Number((row as { id?: unknown } | null)?.id);

/** 丢掉未对完账的本地增删，并取消排队中的对账。 */
const resetPendingDelta = () => {
  pendingRemoved.clear();
  pendingAdded.clear();
  reconcileAttempts = 0;
  // 这条路径本身就是一次权威读取，欠的那次对账因此作废。
  reconcileOwed = false;
  if (reconcileTimer) {
    clearTimeout(reconcileTimer);
    reconcileTimer = null;
  }
};

/** 重排 num。整体换条目对象——条目是 markRaw 的，就地改字段不会重渲染。 */
const renumber = (rows: unknown[]): unknown[] =>
  rows.map((row, index) => asRawEntry({ ...(row as object), num: index + 1 }));

/**
 * 把尚未被服务端确认的增删盖到 `rows` 上。
 *
 * 长流里不再有「页」，所以也没有 `slice(0, limit)`：新增总是插到头部（网易就是
 * 这么放的），而头部永远落在已 hydrate 的前缀里，所以补丁一定可见。
 */
const applyPendingDelta = (rows: unknown[], added: unknown[]): unknown[] => {
  let next = rows.filter((row) => !pendingRemoved.has(rowId(row)));
  if (added.length) {
    const present = new Set(next.map(rowId));
    const missing = added.filter((song) => !present.has(rowId(song)));
    if (missing.length) next = [...missing, ...next];
  }
  return next;
};

const scheduleReconcile = () => {
  if (reconcileTimer) clearTimeout(reconcileTimer);
  reconcileTimer = setTimeout(() => {
    reconcileTimer = null;
    void reconcileQuietly();
  }, RECONCILE_DELAY);
};

/**
 * 重新拉取 manifest 与详情，但不显示 loading、不回顶。
 *
 * 只拉 `/playlist/detail` 一次。一次写入改变的就是 `trackIds` 的成员与顺序，加上
 * 总数——而这正是 pending 增删要问的两件事：服务端到底认没认账、新歌被放到了第
 * 几位。歌曲详情不必失效：`song_detail` 是按 id 的，歌单写入不改变歌曲元数据
 * （`invalidated_by("like")` 也确实只清 likelist / playlist_track_all /
 * playlist_detail）。
 *
 * 这比旧实现更强也更省：旧实现重拉「当前那一页」，只能回答这一页对不对，而在长
 * 流里根本没有当前页——重拉已加载的前缀会变成 AGENTS.md 警告的无限拉取。
 *
 * `fresh: true` 是这条路径的**前提**，不是优化。对账问的是「账号认没认」，而这个
 * 问题按定义不能由一份已经握着的答案回答：
 *
 * - `invalidated_by` 只在写入**经过这个 isolate** 时才起作用。remote 传输走 axios、
 *   Rust 侧解析器的 `/like` 走 ureq、别的设备和网页版根本不碰这个进程——那些写入
 *   一条缓存都没清。（默认传输下通知栏的红心是**会**清的：它经
 *   `install_ncm_call_hook` 走同一个 `NcmCore::call`。）
 * - `playlist_detail` 有 24 小时 stale 窗口，过期后仍然**立即**返回旧的那份，只在
 *   背后刷新。所以「对账」会拿到写入前的列表，`pendingRemoved` 一条都对不上，重试
 *   到 `MAX_RECONCILE_ATTEMPTS` 后本地补丁被丢弃，删掉的行当场回来——比不对账更糟，
 *   而这正是 `playlistMutations` 那段注释在防的事。
 *
 * 只跳过缓存的**读**：答案照常写回去，`batch`/`inflight` 照常合并，所以代价是一次
 * round trip，而不是绕开缓存层。
 */
const reconcileQuietly = async () => {
  if (!playListId.value) return;
  // 页面在 keep-alive 里被藏起来了：不发请求，但把这件事记下来，`onActivated` 会补。
  // 直接丢掉是不行的——路由 watch 只认 id 变化，回到本页时不会有任何一次重拉。
  if (!isActive) {
    reconcileOwed = true;
    return;
  }
  const sourceId = normalizePlaylistId(playListId.value);
  const token = loadToken;
  reconcileAttempts += 1;
  try {
    lastSyncedAt = Date.now();
    const detail = await fetchPlaylistDetail(sourceId, { fresh: true });
    // 请求期间用户可能已经切歌单了，那这份结果就不再是当前视图的。
    if (token !== loadToken) return;
    if (!playListId.value || normalizePlaylistId(playListId.value) !== sourceId) return;
    const pl = detail?.playlist;
    if (!pl) return;

    const serverIds = extractManifestIds(pl);
    const serverSet = new Set(serverIds);
    // 服务端已经认账的从 pending 里划掉，剩下的继续盖着。
    // 迭代中删当前元素对 Set 迭代器是安全的，不必先复制一份。
    for (const id of pendingRemoved) if (!serverSet.has(id)) pendingRemoved.delete(id);
    for (const id of pendingAdded) if (serverSet.has(id)) pendingAdded.delete(id);

    if (pendingRemoved.size || pendingAdded.size) {
      if (reconcileAttempts < MAX_RECONCILE_ATTEMPTS) {
        // 还没落盘：保住本地这份，稍后再问一次。总数也不能覆盖，否则数字会跳。
        scheduleReconcile();
        return;
      }
      pendingRemoved.clear();
      pendingAdded.clear();
    }

    reconcileAttempts = 0;
    reconcileOwed = false;
    await adoptManifest(serverIds, pl, sourceId, token);
  } catch (err) {
    // 对账是尽力而为：失败了就维持本地这份，下次回到本页时再试。
    reconcileOwed = true;
    console.error("[playlist] quiet reconcile failed", err);
  }
};

/**
 * 采用服务端的 manifest，尽量复用已 hydrate 的行。
 *
 * 前缀长度保持不变（用户滚到哪就还在哪），但顺序与成员以服务端为准。头部插入了
 * 新歌时前缀里会缺几行，那几行——也只有那几行——补一次 `song_detail`。
 */
const adoptManifest = async (
  ids: number[],
  pl: PlaylistDetail & { trackCount?: number },
  sourceId: number,
  token: number,
) => {
  manifestIds.value = ids;
  totalCount.value = pl.trackCount ?? ids.length;
  playListDetail.value = pl;

  const want = Math.min(playListData.value.length, ids.length);
  if (!want) {
    playListData.value = [];
    return;
  }
  const target = ids.slice(0, want);
  const have = new Map(playListData.value.map((row) => [rowId(row), row]));
  const missing = target.filter((id) => !have.has(id));
  if (missing.length) {
    try {
      const res = await fetchSongDetail(missing);
      if (token !== loadToken) return;
      for (const row of transformSongData(res?.songs ?? [], { sourceId })) {
        have.set(rowId(row), row);
      }
    } catch (err) {
      console.error("[playlist] adopt hydrate failed", err);
    }
  }
  if (token !== loadToken) return;
  playListData.value = renumber(target.map((id) => have.get(id)).filter(Boolean));
  // 前缀被按新 manifest 重建了，游标必须跟着回到对应位置，否则后续补块会从错的地方
  // 继续，跳过或重复一整段。
  hydrateCursor.value = want;
};

const handlePlaylistChange = (change: PlaylistChange) => {
  if (!playListId.value) return;
  if (change.playlistId !== normalizePlaylistId(playListId.value)) return;

  if (change.kind === "meta") {
    // 侧边栏由调用点的 setUserPlayLists 负责，标题/简介/标签这份是本页自己的。
    void refreshDetailOnly();
    return;
  }

  const added = change.added ?? [];
  const addedIds = change.addedIds ?? [];
  const removedIds = change.removedIds ?? [];
  reconcileAttempts = 0;

  // 说不清增删了什么（例如 fm_trash）——只对账，不猜。
  if (!added.length && !addedIds.length && !removedIds.length) {
    scheduleReconcile();
    return;
  }

  for (const id of removedIds) {
    pendingRemoved.add(id);
    pendingAdded.delete(id);
  }
  const allAddedIds = [...added.map(rowId), ...addedIds];
  for (const id of allAddedIds) {
    pendingAdded.add(id);
    pendingRemoved.delete(id);
  }

  totalCount.value = Math.max(
    0,
    totalCount.value + added.length + addedIds.length - removedIds.length,
  );
  // manifest 也要就地打补丁：它是总高度和后续分块的依据，只改可见行会让滚动条
  // 长度和「还剩多少」当场对不上。
  const removedSet = new Set(removedIds);
  const kept = manifestIds.value.filter((id) => !removedSet.has(id));
  const presentInManifest = new Set(kept);
  manifestIds.value = [...allAddedIds.filter((id) => !presentInManifest.has(id)), ...kept];
  playListData.value = renumber(applyPendingDelta(playListData.value, added));
  scheduleReconcile();
};

/**
 * 只刷新详情文案，不动 manifest 与已 hydrate 的行。
 *
 * 同样要 `fresh`：这条路径的唯一触发点是「刚刚改完歌单信息」（`PlaylistUpdate.vue`
 * 的 `meta` 事件）。改名成功后紧接着重拉，若那次写入没经过这个 isolate（remote
 * 传输、另一台设备、网页版），缓存里那条 `playlist_detail` 一条都没被清，于是页面
 * 标题继续显示旧名字——正是这个函数被加进来要解决的那件事。
 */
const refreshDetailOnly = async () => {
  if (!playListId.value) return;
  const sourceId = normalizePlaylistId(playListId.value);
  const token = loadToken;
  try {
    const res = await fetchPlaylistDetail(sourceId, { fresh: true });
    if (token !== loadToken) return;
    if (res?.playlist) playListDetail.value = res.playlist;
  } catch (err) {
    console.error("[playlist] detail refresh failed", err);
  }
};

let stopPlaylistChanges: (() => void) | null = null;

/**
 * 回到本页时补上隐藏期间该做而没做的事。
 *
 * 两件：一是隐藏时被记账、没能发出的那次写后对账；二是单纯待久了的重新校验。两者
 * 走同一条安静路径，所以只需要一次请求。
 *
 * 这个钩子存在的原因是路由 watch 故意**不**在再次进入时重载（见文件末尾）：它把
 * hydrate 进度和滚动位置留住了，代价就是没有任何东西会再问一次服务端。
 */
const revalidateIfStale = () => {
  if (!playListId.value || !playListDetail.value) return;
  if (reconcileTimer) return; // 已经排着一次了
  if (reconcileOwed || Date.now() - lastSyncedAt >= REVALIDATE_AFTER) {
    reconcileAttempts = 0;
    void reconcileQuietly();
  }
};

onActivated(() => {
  isActive = true;
  revalidateIfStale();
});
onDeactivated(() => {
  isActive = false;
});
onUnmounted(() => {
  stopPlaylistChanges?.();
  stopPlaylistChanges = null;
  if (reconcileTimer) clearTimeout(reconcileTimer);
  if (searchHydrateTimer) clearTimeout(searchHydrateTimer);
});

// 删除歌单
const toDelPlayList = (data: { id: number; name: any }) => {
  if (data.id === user.getUserPlayLists?.own[0].id) {
    $message.warning(t("menu.unableToDelete"));
    return false;
  }
  $dialog.warning({
    class: "s-dialog",
    title: t("general.dialog.delete"),
    content: t("menu.delQuestion", {
      name: data.name,
    }),
    positiveText: t("general.dialog.delete"),
    negativeText: t("general.dialog.cancel"),
    onPositiveClick: () => {
      delPlayList(data.id).then((res) => {
        if (res.code === 200) {
          $message.success(t("general.message.deleteSuccess"));
          user.setUserPlayLists();
          router.push("/user/playlists");
        }
      });
    },
  });
};

// 收藏/取消收藏
const toChangeLike = async (id: string | number | string[]) => {
  const type = isLikeOrDislike(id.toString()) ? 1 : 2;
  const likeMsg = t("general.name.playlist");
  try {
    const res = await likePlaylist(normalizePlaylistId(id), type);
    if (res.code === 200) {
      $message.success(buildLikeMessage(t, likeMsg, type, "success", setting.language));
      user.setUserPlayLists(() => {
        setDropdownOptions();
      });
    } else {
      $message.error(buildLikeMessage(t, likeMsg, type, "failed", setting.language));
    }
  } catch (err) {
    console.error(buildLikeMessage(t, likeMsg, type, "failed", setting.language), err);
    $message.error(buildLikeMessage(t, likeMsg, type, "failed", setting.language));
  }
};

onMounted(() => {
  stopPlaylistChanges ??= onPlaylistChanged(handlePlaylistChange);
  if (playListId.value) {
    loadPlaylist(playListId.value, initialRowsFromQuery());
    if (user.userLogin && !user.getUserPlayLists.has && !user.getUserPlayLists.isLoading) {
      user.setUserPlayLists(() => {
        setDropdownOptions();
      });
    } else {
      setDropdownOptions();
    }
  }
});

/**
 * 旧链接带的 `?page=N` 吸收成「至少先加载到第 N 页那么多行」。
 *
 * 分页时代每页 30 条，所以 page=3 意味着用户想看到第 61-90 条；长流里没有页码可
 * 跳，但把前 90 行准备好就等价于让那批内容可达。
 */
const LEGACY_PAGE_SIZE = 30;
const initialRowsFromQuery = (): number => {
  const raw = route.query.page;
  const page = Number(Array.isArray(raw) ? raw[0] : raw);
  return Number.isFinite(page) && page > 1 ? page * LEGACY_PAGE_SIZE : 0;
};

const retryPlaylist = () => {
  if (playListId.value) loadPlaylist(playListId.value, initialRowsFromQuery());
};

/**
 * 监听路由参数变化。
 *
 * 判据是「路由的 id 和**本实例**手上这个不一样」，而不是「和上一个路由不一样」。
 * 后者把每一次**再次进入**都当成换歌单：App 的 keep-alive key 里带着 `query.id`
 * （`App.vue` 的 `<component :key>`），所以从别处回到一个已经缓存的歌单页时，
 * `oldVal` 是那个别处 —— 它的 `query.id` 当然不等于本页的（本地音乐详情页带的是
 * 自己那套 id，`/local` 和首页干脆没有 id），于是每次回来都把 `playListData` 清空
 * 重拉一遍。白花一次 247 KB 的 `playlist_detail` 是小事，真正的代价是把用户滚出来
 * 的那几千行 hydrate 进度和滚动位置一起丢掉 —— 正是上面那条 `?page=` 注释想避免
 * 的事，只是它只挡住了同一个 id 连着出现的情形。
 *
 * 更糟的是「清空 → 重新有行」这一趟本身：`DataLists` 的页面窗口化探测挨不住它
 * （见那边 `plainRootRef` 的 watch），于是从本地音乐切回一个开过的大歌单之后，
 * 列表就再也补不上下一块了。
 *
 * `isActive` 那一半是另一个方向：换歌单换的是**实例**，所以「歌单 A → 歌单 B」时
 * 被收进 keep-alive 的 A 的这个 watch 也会响。它照着 B 重载等于把一份缓存实例填成
 * 另一个歌单的内容，用户退回 A 时看到的是 B。
 */
watch(
  () => route.fullPath,
  () => {
    const val = route;
    if (val.name !== "playlist" || !isActive) return;
    const nextId = val.query.id as string | string[] | undefined;
    if (!nextId || String(nextId) === String(playListId.value)) return;
    playListId.value = nextId;
    loadPlaylist(nextId, initialRowsFromQuery());
  },
);
</script>

<style lang="scss" scoped>
.playlist,
.loading {
  // 悬浮搜索控件的玻璃参数。结构抄自 Nav 的悬浮按钮，但 `--floating-control-bg` 是
  // 定义在 `.nav` 内部的、拿不到，所以这里重新声明一份同名不同前缀的。
  // 暗色钩子和 Nav 一致：`setting.getSiteTheme === "dark"`。
  //
  // 填充**不**照抄 Nav 的纯白半透明：这里掺了封面强调色，也更实一点。原因是这一处的
  // `backdrop-filter` 静止时无事可做——药丸背后是 `.content-panel-frame` 画的面板底，
  // `--content-panel-bg` 是不透明近白，上面只叠了一层峰值 16%、向下衰减到透明的径向
  // wash（见 `coverPalette` 的 `getPanelStageGradient`）。模糊一片平坦的近白得到的还是
  // 同一片近白，`saturate(160%)` 对着几乎无饱和度的底色同理。Nav 用同一组参数看着像
  // 玻璃，是因为它钉在整页滚动内容之上、底下随时有行经过；而这条工具栏在列表**上方**，
  // 只有滚动时行才从它底下过——那时滤镜确实生效（行和药丸同在 `n-layout-content` 的
  // `clip-path` 隔离出来的那个组里），但静止时的层次只能由填充自己给。
  //
  // `--content-panel-accent-rgb` 由 `useContentPanelAccent` 按当前封面写在 `:root` 上，
  // 所以药丸跟着专辑走，而不是一块和面板同色的白片；没有取色的页面回退成中性白。
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

  // 移动端顶部那条带子本身就是模糊 + 着色的，控件要在它之上仍读得出是一个层，
  // 所以抬高填充、收紧描边。这是 Nav 移动端得出的同一个结论。
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
    // 底部收窄：头部块和列表之间原本叠了 gap 22 + padding 24 + 工具栏 margin，
    // 加起来是一条明显的死区。工具栏本身已经提供了呼吸，这里不必再留。
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
          line-height: normal;
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

      .intr {
        max-width: 760px;
        margin-top: 14px;

        .name {
          display: none;
        }

        .desc {
          display: -webkit-box;
          -webkit-line-clamp: 2;
          line-clamp: 2;
          line-height: 22px;
          color: var(--n-text-color-3);
        }

        .all-desc {
          width: fit-content;
          margin-top: 12px;
        }
      }

      .tag {
        margin-top: 13px;

        .tags {
          height: 22px;
          font-size: 12px;
          color: var(--n-text-color-2);
          background-color: color-mix(in srgb, var(--n-border-color) 62%, transparent);
          cursor: pointer;
          transition: all var(--duration-300) var(--ease-out);

          &:hover {
            background-color: color-mix(
              in srgb,
              rgb(var(--content-panel-accent-rgb, 128, 128, 128)) 16%,
              transparent
            );
            color: rgb(var(--content-panel-accent-rgb, 128, 128, 128));
          }

          &:active {
            transform: scale(0.95);
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

    .meta {
      display: none;
    }

    :deep(.datalists) {
      // 列宽的唯一出处：`.songs` 的行和 `.song-list-head` 的列头都读这组变量，
      // 所以不可能出现「改了行没改列头」的错位。
      // name:album 给到 1.6:1（NCM 官方大致是这个比例）——两边都 flex:1 会把专辑名
      // 顶到正中间，中间空一大段。
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

    // 只用 `song-row-*` 类，不用 `:nth-child`。页面窗口模式下行被包在
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

    :deep(.pagination) {
      margin-top: 18px;
    }

    // 搜索框锚在列表的**右**边缘（和时长列同一条竖线），而不是浮在左边。
    // 左边已经有封面和列表两条对齐线了，再在中间放一个小控件只会把断层放大；
    // 靠右它就落在一条已经存在的对齐线上，那条空带也不再是「左边一个小方块 +
    // 右边一大片空」。
    // 悬浮，沿用 Nav 的那套语言（见 `components/Nav/index.vue`）。
    //
    // 关键是**横条整条透明，只有药丸自己有玻璃底**。给整条填实色是行不通的：这一页
    // 的底色是封面取样出来的渐变（`--content-panel-stage-gradient` 叠在
    // `--content-panel-bg` 上），实色盖上去必然是一条突兀的横条——试过
    // `--app-shell-bg`，那是渐变底下那层 `#f2f2f4` 冷灰。只让一个控件大小的区域走
    // backdrop-filter，模糊面积和 Nav 的按钮同级，代价可以接受。
    .list-toolbar {
      position: sticky;
      // 钉在 Nav 下沿，由 shell 提供（见 `App.vue` 的 `--content-sticky-top`）：滚动
      // 视口的顶端在 Nav 底下，而且还被 `clip-path` 切掉一截，所以钉 0 等于钉到看不见
      // 的地方——移动端还会被顶部那条玻璃带洗白。这个变量两端都算好了。
      top: var(--content-sticky-top, 0px);
      z-index: 3;
      display: flex;
      align-items: center;
      justify-content: flex-end;
      margin: 2px 0 10px;
      // 一条横跨整宽的透明 sticky 元素会把下面每一行的点击都吃掉。空白处必须放行，
      // 只有药丸本身接收事件。
      pointer-events: none;

      > * {
        pointer-events: auto;
      }
    }

    // 药丸形悬浮控件。常态收窄，聚焦或有内容时展开——列表页大部分时间不在搜索，
    // 一条常驻的宽输入框会把视觉重心从列表本身抢走。
    .list-search {
      width: 170px;
      transition: width var(--duration-300) var(--ease-out);

      // 要在**两种**背景上都站得住：封面取样的渐变底，以及滚动时罩在上面那条 42px
      // 玻璃带。带子会把低对比度的东西直接洗掉——纯靠 `--n-text-color` 的淡色 tint
      // 滚进去就只剩一个幽灵轮廓（试过 5%、7%，都不行）。
      //
      // 参数直接取自 Nav 的悬浮按钮，包括它移动端那条注释的结论：带子后面要**抬高**
      // 填充不透明度，并且收紧阴影——宽而软的投影压在模糊上只会糊成一团灰光晕。
      //
      // 直接写在 `.list-search` 上，**不要**套 `:deep()`：这个 class 落在 `n-input` 的
      // 根元素上，而子组件根元素同时带着父组件的 scope id，scoped 规则本来就够得到。
      // 原先写的是 `:deep(&.n-input)`，而 SASS 不解析 `:deep()` 里的 `&`，编译结果是
      // `.list-search[data-v-x] &.n-input` —— 顶层带 `&` 的选择器浏览器整条丢弃，所以
      // 这块玻璃从来没生效过。露出来的就是 naive 自己的 `.n-input`：亮色主题下
      // `--n-color` 是纯白、圆角只有 3px，而下面两个 border 覆盖层又被改成了药丸，于是
      // 白色底从药丸四角漏在描边外面——那正是「border 有白边」。
      border-radius: var(--radius-pill);
      background-color: var(--list-search-bg);
      box-shadow:
        0 8px 22px rgb(0 0 0 / 10%),
        inset 0 1px 0 rgb(255 255 255 / 24%);
      -webkit-backdrop-filter: blur(18px) saturate(160%);
      backdrop-filter: blur(18px) saturate(160%);

      // naive 的描边画在两个绝对定位的覆盖层上，它们 `border-radius: inherit`，所以
      // 根上的药丸圆角会自己跟上；这里写一遍是为了不依赖那个 inherit。
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

          .detail-kind {
            margin-bottom: 7px;
            font-size: 11px;
          }

          .name {
            font-size: clamp(25px, 8vw, 36px);
            line-height: normal;
          }

          .creator {
            font-size: 15px;
            margin-top: 9px;
          }
        }

        .detail-stats {
          margin-top: 12px;
          color: var(--n-text-color-3);
        }

        .intr {
          margin-top: 16px;

          .desc {
            -webkit-line-clamp: 2;
            line-clamp: 2;
            color: var(--n-text-color-3);
          }
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

      // 窄屏上「靠右的窄输入框 + 左边一大片空」很怪，而且手指要伸到角上。
      // 直接占满一行，也就不需要聚焦展开了。
      // 填充抬高、描边收紧在根的 token 块里按断点声明（见文件上方），这里不重复。
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

  @media (max-width: 540px) {
    .left {
      .cover {
        width: min(64vw, 235px);
      }

      .meta {
        .tag {
          display: none !important;
        }
      }
    }
  }

  @media (max-width: 380px) {
    .left {
      .meta {
        .control {
          :deep(.n-button:first-child) {
            min-width: 96px;
          }
        }
      }
    }
  }
}

.title {
  margin-top: 30px;
  margin-bottom: 20px;
  font-size: 24px;
  .key {
    font-size: 40px;
    font-weight: bold;
    margin-right: 8px;
  }
}
.loading {
  .left {
    .pic {
      position: relative;
      z-index: 1;
      width: 100%;
      height: 100%;
      border-radius: var(--radius-md) !important;
    }

    .shadow {
      position: absolute;
      inset: 10px 0 0;
      width: 100%;
      height: 100%;
      border-radius: var(--radius-md);
      filter: blur(18px) opacity(0.24);
      transform: scale(0.92, 0.94);
      z-index: 0;
    }

    .loading-meta {
      gap: 10px;
    }

    .loading-title {
      :deep(.n-skeleton) {
        height: clamp(32px, 5vw, 52px);
      }
    }

    .loading-stats,
    .loading-actions {
      display: flex;
      flex-wrap: wrap;
      gap: 10px 14px;
      margin-top: 2px;
    }
  }

  .right {
    display: flex;
    flex-direction: column;
    gap: 0;
  }

  .loading-row {
    min-height: 52px;
    display: grid;
    grid-template-columns: 38px minmax(0, 1fr) minmax(84px, 16vw) 46px;
    align-items: center;
    gap: 14px;
    padding: 8px 12px;
    border-radius: 0;

    &:nth-child(odd) {
      background-color: color-mix(in srgb, var(--n-text-color) 3%, transparent);
    }

    &:nth-child(even) {
      background-color: color-mix(in srgb, var(--n-text-color) 6%, transparent);
    }

    &:first-child {
      border-radius: var(--radius-md) var(--radius-md) 0 0;
    }

    &:last-child {
      border-radius: 0 0 var(--radius-md) var(--radius-md);
    }

    &:only-child {
      border-radius: var(--radius-md);
    }
  }

  .loading-row-main {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  @media (max-width: 768px) {
    .left {
      .shadow {
        display: none;
      }
    }

    .loading-row {
      grid-template-columns: 42px minmax(0, 1fr);
      min-height: 58px;

      > :nth-child(n + 3) {
        display: none;
      }
    }
  }
}
</style>
