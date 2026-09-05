<template>
  <div :class="['local-song', { 'is-dark': setting.getSiteTheme === 'dark' }]">
    <!--
      这个注释必须留在根元素**内部**，而且正文里不能出现 HTML 注释的结束记号（连续两个
      减号加一个右尖括号）。

      Vue 的 SFC 编译在 dev 下 `comments: true`，所以模板顶层的注释会变成一个真正的注释
      vnode —— 把它放在根元素前面，这个组件就有了**两个根节点**，是个 fragment。而
      `App.vue` 用的是 `<transition mode="out-in">`：Transition 只能作用于单个元素，拿到
      fragment 时离场永远不会完成，于是新页面只渲染一个空占位节点，主内容区就整片空白。
      而且**只在 dev 出现** —— 生产构建会把注释剥掉，正好把证据一起剥掉。

      至于结束记号：写在正文里会把这条注释就地截断，后面的文字变成真正的模板内容直接渲染
      到页面上。上一版就是这么翻的车。

      一并说明这一页为什么是「一个恒定根 + 三个状态各自是它的单个子元素」：状态切换如果换
      的是**根**元素，进场动画还在跑时把它换掉会让 Transition 抓着一个已经游离的锚点，下一
      次 patch 抛 `insertBefore of null`。网易那几个详情页用 v-if 换根没事，只是因为它们的
      请求要 100-300ms，那时动画早结束了；本地曲目走 IPC 是个位数毫秒，正落在动画中间。
    -->
    <div class="stage" v-if="detail">
      <div class="left">
        <div class="cover">
          <n-image
            show-toolbar-tooltip
            class="coverImg"
            :src="coverUrl"
            :previewed-img-props="{ style: { borderRadius: 'var(--radius-md)' } }"
            :preview-src="coverUrl"
            fallback-src="/images/pic/default.png"
          />
          <n-image
            class="shadow"
            preview-disabled
            :src="coverUrl"
            fallback-src="/images/pic/default.png"
          />
        </div>
        <div class="meta">
          <div class="title">
            <span class="detail-kind">{{ $t("local.detail.kind") }}</span>
            <n-text class="name text-hidden">{{ view?.title || view?.displayName }}</n-text>
            <n-text class="creator">{{ view?.artist || $t("local.unknown") }}</n-text>
          </div>
          <div class="detail-stats">
            <div class="num" v-if="view?.album">
              <n-icon :depth="3" :component="RecordDisc" />
              <n-text>{{ view?.album }}</n-text>
            </div>
            <div class="num">
              <n-icon :depth="3" :component="Time" />
              <n-text>{{ durationText }}</n-text>
            </div>
            <div class="num" v-if="formatText">
              <n-icon :depth="3" :component="FileMusic" />
              <n-text>{{ formatText }}</n-text>
            </div>
            <div class="num" v-if="detail.view.overridden">
              <n-icon :depth="3" :component="Write" />
              <n-text>{{ $t("local.detail.edited") }}</n-text>
            </div>
          </div>
          <n-space class="control">
            <n-button strong secondary round type="primary" @click="playNow">
              <template #icon>
                <n-icon :component="PlayOne" />
              </template>
              {{ $t("general.name.play") }}
            </n-button>
            <n-button strong secondary round @click="toggleFavourite">
              <template #icon>
                <n-icon :component="detail.view.favourite ? Like : Unlike" />
              </template>
              {{ detail.view.favourite ? $t("local.detail.unlike") : $t("local.detail.like") }}
            </n-button>
            <n-button v-if="canReveal" strong secondary round @click="revealInFolder">
              <template #icon>
                <n-icon :component="FolderOpen" />
              </template>
              {{ $t("menu.revealInFolder") }}
            </n-button>
          </n-space>
        </div>
      </div>
      <div class="right">
        <div class="meta">
          <n-text class="name">{{ view?.title || view?.displayName }}</n-text>
          <n-text class="creator">
            <n-icon :depth="3" :component="People" />
            {{ view?.artist || $t("local.unknown") }}
          </n-text>
        </div>
        <!--
          `<n-tab>` 按钮 + 内容渲染在下面，**不用 `<n-tab-pane>`** —— 这是全站每个带
          标签的路由页面（Search / User / Artist / Profile / Discover / Local 首页）
          一致的写法，而 `n-tab-pane` 在这个仓库里只出现在弹窗（ListenTogetherModal）
          里，也就是从来没有在「keep-alive + 页面过场动画」下面跑过。

          差别不是风格：`n-tab-pane` 会额外套一层被 NTabs 用 ResizeObserver 量着的面板
          容器，而这一页离场时元素还在 DOM 里、正在被过场动画缩放。外层是
          `<transition mode="out-in">`，离场不结束进场就永远不开始，`<main>` 里只剩注释
          占位符 —— 就是「跳到任何其他页面主内容全白」。改成按钮 + `v-if` 后这层容器
          和它的观察者都不存在了。
        -->
        <n-tabs class="main-tab tabs" v-model:value="tab" type="line">
          <n-tab name="info">{{ $t("local.detail.tab.info") }}</n-tab>
          <n-tab name="tags">{{ $t("local.detail.tab.tags") }}</n-tab>
          <n-tab name="lyric">{{ $t("local.detail.tab.lyric") }}</n-tab>
        </n-tabs>
        <div class="pane">
          <LocalSongInfoPane v-if="tab === 'info'" :detail="detail" />
          <LocalSongTagsPane v-else-if="tab === 'tags'" :detail="detail" @updated="reload" />
          <LocalSongLyricPane v-else :detail="detail" @updated="reload" />
        </div>
      </div>
    </div>

    <div class="missing" v-else-if="!trackKey || !loading">
      <span class="key">{{
        loading ? $t("general.name.noKeywords") : $t("local.detail.missing")
      }}</span>
      <br />
      <n-button strong secondary @click="router.go(-1)" style="margin-top: 20px">
        {{ $t("general.name.goBack") }}
      </n-button>
    </div>

    <div class="stage" v-else>
      <div class="left">
        <div class="cover">
          <n-skeleton class="pic" />
          <n-skeleton class="shadow" />
        </div>
        <div class="meta loading-meta">
          <n-skeleton text width="64px" />
          <n-skeleton class="loading-title" text width="min(560px, 100%)" />
          <n-skeleton text width="160px" />
          <div class="loading-stats">
            <n-skeleton text width="116px" />
            <n-skeleton text width="136px" />
            <n-skeleton text width="82px" />
          </div>
          <div class="loading-actions">
            <n-skeleton :sharp="false" width="112px" height="34px" />
            <n-skeleton :sharp="false" width="112px" height="34px" />
          </div>
        </div>
      </div>
      <div class="right loading-list">
        <div v-for="item in 6" :key="item" class="loading-row">
          <n-skeleton text width="88px" />
          <n-skeleton text width="min(320px, 60%)" />
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import {
  FileMusic,
  FolderOpen,
  Like,
  PlayOne,
  People,
  RecordDisc,
  Time,
  Unlike,
  Write,
} from "@icon-park/vue-next";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { musicStore, settingStore, useLocalLibraryStore } from "@/store";
import {
  localTrackDetail,
  localTrackToSongData,
  type LocalTrackDetail,
} from "@/utils/localLibrary";
import { getSongTime } from "@/utils/timeTools";
import { DEFAULT_COVER } from "@/utils/coverUrl";
import { isTauri } from "@/utils/tauri/core/runtime";
import { useContentPanelAccent } from "@/composables/useContentPanelAccent";
import LocalSongInfoPane from "./LocalSongInfoPane.vue";
import LocalSongTagsPane from "./LocalSongTagsPane.vue";
import LocalSongLyricPane from "./LocalSongLyricPane.vue";

/**
 * The detail page for one imported file.
 *
 * Structurally a copy of `views/Song/SongView.vue` — same `.left` cover-plus-meta
 * block, same `.right` column, same loading skeleton, same `is-dark` hook — so
 * that a local track's detail page reads as the same page as a Netease track's.
 * The one difference is what fills `.right`: three tabs instead of comments and
 * similar playlists, because a local file has no comment thread and does have
 * three things only this page can offer (what the file is, a correction for its
 * tags, a lyric import).
 *
 * Deliberately **not** a route on `/song`: that page is Netease's, keyed on a
 * positive id, and it fires three requests on mount. A local track's id is a
 * negative hash and belongs to nothing outside this process.
 */
const { t } = useI18n();
const router = useRouter();
const music = musicStore();
const setting = settingStore();
const local = useLocalLibraryStore();
const { applyContentPanelAccent } = useContentPanelAccent();

const detail = ref<LocalTrackDetail | null>(null);
const loading = ref(true);
const tab = ref<"info" | "tags" | "lyric">("info");

/**
 * The locator.
 *
 * Carried in the query rather than the path: it is a Windows path or a
 * `content://` URI, and neither survives being a path segment.
 */
const trackKey = computed(() => {
  const raw = router.currentRoute.value.query.key;
  return typeof raw === "string" ? raw : Array.isArray(raw) ? (raw[0] ?? "") : "";
});

/**
 * The row, when there is one.
 *
 * Optional rather than `detail.value!.view`: `App.vue` keeps routed pages in
 * `<keep-alive>`, so this template can be re-rendered *after* the page stopped
 * being the active route — and a non-null assertion there throws inside Vue's
 * patch, which aborts it and leaves the whole main container blank. That was the
 * blank page after navigating away from this view.
 */
const view = computed(() => detail.value?.view);

/**
 * The cover, or the app's default picture.
 *
 * Never the empty string: `<n-image :src="">` makes Chromium resolve the empty
 * URL against the document, fail to decode the HTML it gets back, and draw its
 * own broken-image glyph. `fallback-src` does not cover that — it reacts to a
 * failed *load*, and an `<img>` that never had a real URL is not reliably one.
 * The fallback stays for the case it is actually for: a path that exists in the
 * index but whose file the OS reclaimed from the cache directory.
 */
const coverUrl = computed(() => {
  const path = detail.value?.view.coverPath;
  return path ? convertFileSrc(path) : DEFAULT_COVER;
});

const durationText = computed(() => getSongTime(view.value?.durationMs ?? 0) || "--:--");

const formatText = computed(() => {
  const row = view.value;
  if (!row) return "";
  const parts = [row.codec?.toUpperCase()].filter(Boolean) as string[];
  if (row.sampleRate) parts.push(`${(row.sampleRate / 1000).toFixed(1)} kHz`);
  if (row.bitrateBps) parts.push(`${Math.round(row.bitrateBps / 1000)} kbps`);
  return parts.join(" · ");
});

/**
 * Desktop only: the opener plugin's reveal action has no Android equivalent, and
 * a `content://` document has no folder to open.
 */
const canReveal = computed(
  () => isTauri() && !!detail.value && !detail.value.view.key.startsWith("content://"),
);

const reload = async () => {
  const key = trackKey.value;
  if (!key) {
    loading.value = false;
    detail.value = null;
    return;
  }
  loading.value = true;
  try {
    const next = await localTrackDetail(key);
    // The route may have changed while this was in flight.
    if (key !== trackKey.value) return;
    detail.value = next;
    if (next) {
      $setSiteTitle(`${next.view.title || next.view.displayName} - ${t("sidebar.localMusic")}`);
      if (next.view.coverPath) applyContentPanelAccent(convertFileSrc(next.view.coverPath));
    }
  } finally {
    if (key === trackKey.value) loading.value = false;
  }
};

const playNow = () => {
  if (!detail.value) return;
  music.setPersonalFmMode(false);
  music.addSongToPlaylists(localTrackToSongData(detail.value.view));
};

const toggleFavourite = async () => {
  if (!detail.value) return;
  const next = !detail.value.view.favourite;
  if (await local.setFavourite(detail.value.view.key, next)) {
    // Replace rather than mutate: list entries are `markRaw`'d, and this object
    // is handed to child panes.
    detail.value = { ...detail.value, view: { ...detail.value.view, favourite: next } };
  }
};

const revealInFolder = async () => {
  const key = view.value?.key;
  if (!key) return;
  try {
    const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
    await revealItemInDir(key);
  } catch (err) {
    console.error("[LocalSong] could not reveal the file:", err);
    $message.error(t("local.detail.revealFailed"));
  }
};

onMounted(() => {
  void reload();
  if (typeof $scrollToTop !== "undefined") $scrollToTop();
});

// Entering the page again with a different `?key=` reuses this component — the
// keep-alive key in `App.vue` is built from `route.matched[0].path` plus
// `query.id`, and this route carries neither a child path nor an `id`.
//
// The route-path guard is the load-bearing half: `trackKey` reads the *global*
// current route, so leaving this page for anywhere else makes it `""` and fires
// this watch. Without the guard that wiped `detail` on a page the user had
// already left, and the cached component then re-rendered with nothing to show.
watch(trackKey, (key, previous) => {
  if (router.currentRoute.value.path !== "/local/song") return;
  if (key !== previous) {
    tab.value = "info";
    void reload();
    if (typeof $scrollToTop !== "undefined") $scrollToTop();
  }
});
</script>

<style lang="scss" scoped>
// 与 `views/Song/SongView.vue` / `views/Album/AlbumView.vue` 同一份布局。
// 本仓的约定是每个详情页各带一份（三页现在都是这样），不是抽公共 mixin，所以这里
// 也照抄；改动其中一页的头部时另外几页要一起看。
.local-song {
  // 布局挂在 `.stage` 上而不是根节点上：根节点必须是一个**从头到尾都在**的元素
  // （见模板顶部的注释），而三个状态各自是它的单个子元素。
  .stage {
    display: flex;
    flex-direction: column;
    gap: 22px;
    padding: 10px clamp(16px, 3vw, 36px) 36px;
  }

  .left {
    width: 100%;
    min-height: 0;
    position: relative;
    display: grid;
    grid-template-columns: minmax(176px, 278px) minmax(0, 1fr);
    align-items: center;
    gap: clamp(22px, 4vw, 38px);
    padding: 18px 2px 24px;

    .cover {
      position: relative;
      display: flex;
      align-items: center;
      justify-content: flex-start;
      width: 100%;
      aspect-ratio: 1 / 1;
      border-radius: var(--radius-md);
      transition: transform var(--duration-300) var(--ease-out);
      filter: drop-shadow(0 16px 28px rgba(var(--content-panel-accent-rgb, 0, 0, 0), 0.22));

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

    // 桌面端头部已经把标题写在封面右侧了，这一份是移动端用的（封面在上、标题在下）。
    .meta {
      display: none;
    }

    .tabs {
      margin-top: 2px;
    }

    // 面板内容自己撑开，和 `Local/index.vue` 的 `.content` 一样：标签只负责切换，
    // 内容是它的兄弟节点。
    .pane {
      min-width: 0;
      padding-top: 14px;
    }
  }

  @media (max-width: 768px) {
    .stage {
      gap: 14px;
      padding: 8px 14px 28px;
    }

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
          color: var(--n-text-color-3);
        }

        .control {
          margin-top: 16px;

          :deep(.n-button) {
            height: 38px;
          }
        }
      }
    }
  }
}

.local-song {
  .missing {
    margin-top: 30px;
    margin-bottom: 20px;
    font-size: 24px;

    .key {
      font-size: 40px;
      font-weight: bold;
      margin-right: 8px;
    }
  }

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
    display: grid;
    grid-template-columns: 88px minmax(0, 1fr);
    align-items: center;
    gap: 14px;
    padding: 9px 12px;

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
  }

  @media (max-width: 768px) {
    .left {
      .shadow {
        display: none;
      }
    }
  }
}
</style>
