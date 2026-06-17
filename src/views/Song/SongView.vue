<template>
  <div class="song" v-if="musicDetail">
    <div class="left">
      <div class="cover">
        <n-image
          show-toolbar-tooltip
          class="coverImg"
          :previewed-img-props="{ style: { borderRadius: 'var(--radius-md)' } }"
          :preview-src="getCoverUrl(musicDetail.al.picUrl)"
          :src="getCoverUrl(musicDetail.al.picUrl, 1024)"
          fallback-src="/images/pic/default.png"
        >
          <template #placeholder>
            <div class="cover-loading">
              <n-spin />
            </div>
          </template>
        </n-image>
        <n-image
          class="shadow"
          preview-disabled
          :src="getCoverUrl(musicDetail.al.picUrl, 1024)"
          fallback-src="/images/pic/default.png"
        />
      </div>
      <div class="meta">
        <div class="title">
          <span class="detail-kind">{{ $t("general.name.song") }}</span>
          <n-text class="name text-hidden" v-html="musicDetail.name" />
          <AllArtists
            v-if="musicDetail.ar"
            class="creator"
            :artistsData="musicDetail.ar"
            :isDark="false"
          />
        </div>
        <div class="detail-stats">
          <div class="num">
            <n-icon :depth="3" :component="RecordDisc" />
            <n-text
              class="link"
              v-html="musicDetail.al.name"
              @click="router.push(`/album?id=${musicDetail.al.id}`)"
            />
          </div>
          <div class="num" v-if="musicDetail.publishTime">
            <n-icon :depth="3" :component="Time" />
            <n-text v-html="getLongTime(musicDetail.publishTime)" />
          </div>
          <div class="num" v-if="musicDetail.ar">
            <n-icon :depth="3" :component="People" />
            <AllArtists :artistsData="musicDetail.ar" :isDark="false" />
          </div>
        </div>
        <div class="intr" v-if="songAlias">
          <span class="desc text-hidden" v-html="songAlias" />
        </div>
        <n-space class="tag" v-if="hasSongTags">
          <n-tag
            v-if="musicDetail.fee === 1 || musicDetail.fee === 4"
            class="tags"
            round
            :bordered="false"
          >
            {{ musicDetail.fee === 1 ? "VIP" : "EP" }}
          </n-tag>
          <n-tag v-if="musicDetail.pc" class="tags" round type="info" :bordered="false">
            {{ $t("general.name.cloud") }}
          </n-tag>
          <n-tag v-if="musicDetail.mv" class="tags" round :bordered="false">MV</n-tag>
        </n-space>
        <n-space class="control">
          <n-button type="primary" strong secondary round @click="addSong(musicDetail)">
            <template #icon>
              <n-icon :component="PlayOne" />
            </template>
            {{ $t("general.name.play") }}
          </n-button>
          <n-button strong secondary round @click="openAddToPlaylist">
            <template #icon>
              <n-icon :component="ListAdd" />
            </template>
            {{ $t("general.name.add") }}
          </n-button>
          <n-button
            strong
            secondary
            round
            @click="router.push(`/comment?id=${musicDetail.id}&page=1`)"
          >
            <template #icon>
              <n-icon :component="Comments" />
            </template>
            {{ $t("general.name.comment") }}
          </n-button>
          <n-button
            strong
            secondary
            round
            v-if="musicDetail.mv"
            @click="router.push(`/video?id=${musicDetail.mv}`)"
          >
            <template #icon>
              <n-icon :component="Youtube" />
            </template>
            MV
          </n-button>
        </n-space>
      </div>
    </div>
    <div class="right">
      <section class="comments detail-section" v-if="commentData[0]">
        <n-h6 class="section-title" prefix="bar">{{ $t("general.name.hotComments") }}</n-h6>
        <div class="content">
          <Comment v-for="item in commentData" :key="item.commentId || item" :commentData="item" />
        </div>
      </section>
      <section class="simiPlayList detail-section" v-if="simiPlayList[0]">
        <n-h6 class="section-title" prefix="bar">{{ $t("other.containing") }}</n-h6>
        <CoverLists :listData="simiPlayList" />
      </section>
    </div>
    <!-- 添加到歌单 -->
    <AddPlaylist ref="addPlayListRef" />
  </div>
  <div class="title" v-else-if="!musicId || !loadingState">
    <span class="key">{{
      loadingState ? $t("general.name.noKeywords") : $t("general.message.acquisitionFailed")
    }}</span>
    <br />
    <n-button strong secondary @click="router.go(-1)" style="margin-top: 20px">
      {{ $t("general.name.goBack") }}
    </n-button>
  </div>
  <div class="loading" v-else>
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
        <n-skeleton text width="min(520px, 100%)" />
        <div class="loading-actions">
          <n-skeleton :sharp="false" width="112px" height="34px" />
          <n-skeleton :sharp="false" width="112px" height="34px" />
          <n-skeleton :sharp="false" width="112px" height="34px" />
        </div>
      </div>
    </div>
    <div class="right loading-list">
      <div v-for="item in 5" :key="item" class="loading-row">
        <n-skeleton circle width="38px" height="38px" />
        <div class="loading-row-main">
          <n-skeleton text width="min(360px, 70%)" />
          <n-skeleton text width="min(560px, 86%)" />
        </div>
        <n-skeleton text width="84px" />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { getSimiPlayList, getMusicDetail } from "@/api/song";
import { getComment } from "@/api/comment";
import { useRouter } from "vue-router";
import { musicStore } from "@/store";
import { getLongTime, getSongTime } from "@/utils/timeTools";
import { PlayOne, Comments, ListAdd, Youtube, People, RecordDisc, Time } from "@icon-park/vue-next";
import { formatNumber } from "@/utils/timeTools";
import { useI18n } from "vue-i18n";
import { useContentPanelAccent } from "@/composables/useContentPanelAccent";
import AllArtists from "@/components/DataList/AllArtists.vue";
import CoverLists from "@/components/DataList/CoverLists.vue";
import AddPlaylist from "@/components/DataModal/AddPlaylist.vue";
import Comment from "@/components/Comment/index.vue";
import getCoverUrl from "@/utils/ncm/getCoverUrl";

type RouteQueryId = string | string[] | null | undefined;

const { t } = useI18n();
const router = useRouter();
const music = musicStore();
const addPlayListRef = ref<any>(null);
const { applyContentPanelAccent } = useContentPanelAccent();

// 歌曲数据
const musicId = ref<RouteQueryId>(router.currentRoute.value.query.id);
const musicDetail = ref<any | null>(null);
const loadingState = ref(true);

// 评论数据
const commentData = ref<any[]>([]);

// 相似数据
const simiPlayList = ref<any[]>([]);

const normalizeMusicId = (id: RouteQueryId) => (Array.isArray(id) ? id[0] : id);
const normalizeMusicNumberId = (id: RouteQueryId) => {
  const normalizedId = normalizeMusicId(id);
  if (!normalizedId) return null;
  const numberId = Number(normalizedId);
  return Number.isFinite(numberId) ? numberId : null;
};

const songAlias = computed(() => musicDetail.value?.alia?.[0] ?? "");

const hasSongTags = computed(
  () =>
    !!musicDetail.value &&
    (musicDetail.value.fee === 1 ||
      musicDetail.value.fee === 4 ||
      musicDetail.value.pc ||
      musicDetail.value.mv),
);

// 获取歌曲数据
const getMusicDetailData = (id: RouteQueryId) => {
  const normalizedId = normalizeMusicId(id);
  const normalizedNumberId = normalizeMusicNumberId(id);
  if (!normalizedId || normalizedNumberId === null) {
    loadingState.value = false;
    musicDetail.value = null;
    return;
  }

  loadingState.value = true;
  musicDetail.value = null;
  commentData.value = [];
  simiPlayList.value = [];

  getMusicDetail(normalizedId)
    .then((res) => {
      const song = res.songs?.[0];
      if (song) {
        musicDetail.value = song;
        applyContentPanelAccent(getCoverUrl(song.al?.picUrl, 256));
        const primaryArtistName = song.ar?.[0]?.name;
        $setSiteTitle(
          [song.name, primaryArtistName, t("general.name.song")].filter(Boolean).join(" - "),
        );
        loadingState.value = false;
        // 获取热门评论
        getCommentData(normalizedNumberId);
        // 获取相似数据
        getSimiData(normalizedNumberId);
        // 请求后回顶
        if (typeof $scrollToTop !== "undefined") $scrollToTop();
      } else {
        loadingState.value = false;
        $message.error(t("general.message.acquisitionFailed"));
      }
    })
    .catch((err) => {
      loadingState.value = false;
      console.error(t("general.message.acquisitionFailed"), err);
      $message.error(t("general.message.acquisitionFailed"));
    });
};

// 获取评论数据
const getCommentData = (id: number) => {
  getComment(id)
    .then((res) => {
      commentData.value = res.total > 0 ? (res.hotComments ?? []) : [];
    })
    .catch((err) => {
      console.error(t("general.message.acquisitionFailed"), err);
      $message.error(t("general.message.acquisitionFailed"));
    });
};

// 获取相似数据
const getSimiData = (id: number) => {
  getSimiPlayList(id)
    .then((res) => {
      simiPlayList.value = (res.playlists ?? []).map((v) => ({
        id: v.id,
        cover: v.coverImgUrl,
        name: v.name,
        artist: v.creator,
        playCount: formatNumber(v.playCount),
      }));
    })
    .catch((err) => {
      console.error(t("general.message.acquisitionFailed"), err);
    });
};

const openAddToPlaylist = () => {
  addPlayListRef.value?.openAddToPlaylist(normalizeMusicNumberId(musicId.value));
};

// 添加歌曲并播放
const addSong = (data) => {
  const song = {
    album: data.al,
    artist: data.ar,
    fee: data.fee,
    id: data.id,
    name: data.name,
    pc: data.pc,
    mv: data.mv ? data.mv : null,
    alia: data.alia,
    time: getSongTime(data.dt),
  };
  music.setPersonalFmMode(false);
  music.addSongToPlaylists(song);
};

onMounted(() => {
  getMusicDetailData(musicId.value);
});

// 监听路由参数变化
watch(
  () => router.currentRoute.value,
  (val) => {
    if (val.name === "song") {
      musicId.value = val.query.id;
      getMusicDetailData(musicId.value);
    }
  },
);
</script>

<style lang="scss" scoped>
.song,
.loading {
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
    padding: 18px 2px 24px;

    .cover {
      position: relative;
      display: flex;
      align-items: center;
      justify-content: flex-start;
      width: 100%;
      aspect-ratio: 1 / 1;
      border-radius: var(--radius-md);
      transition: transform 0.3s;
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

        .cover-loading {
          position: relative;
          display: flex;
          align-items: center;
          justify-content: center;
          width: 100%;
          height: 0;
          padding-bottom: 100%;
          background-color: #0001;

          .n-spin-body {
            position: absolute;
            top: 0;
            height: 100%;
            display: flex;
            align-items: center;
            justify-content: center;
          }
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
          max-width: 780px;
          font-size: clamp(32px, 5vw, 56px);
          font-weight: 800;
          line-height: 1.06;
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

          .link {
            cursor: pointer;
            transition: color var(--duration-150) var(--ease-out);

            &:hover {
              color: rgb(var(--content-panel-accent-rgb, 128, 128, 128));
            }
          }
        }
      }

      .intr {
        max-width: 760px;
        margin-top: 14px;

        .desc {
          display: -webkit-box;
          -webkit-line-clamp: 2;
          line-clamp: 2;
          line-height: 22px;
          color: var(--n-text-color-3);
        }
      }

      .tag {
        margin-top: 13px;

        .tags {
          height: 22px;
          font-size: 12px;
          color: var(--n-text-color-2);
          background-color: color-mix(in srgb, var(--n-border-color) 62%, transparent);
          transition: all 0.3s;

          &:hover {
            background-color: color-mix(
              in srgb,
              rgb(var(--content-panel-accent-rgb, 128, 128, 128)) 16%,
              transparent
            );
            color: rgb(var(--content-panel-accent-rgb, 128, 128, 128));
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
    display: flex;
    flex-direction: column;
    gap: 22px;
  }

  .detail-section {
    min-width: 0;

    .section-title {
      margin: 0 0 12px;
      font-size: 16px;
      font-weight: 800;
    }
  }

  .comments {
    .content {
      display: flex;
      flex-direction: column;
      gap: 0;
    }

    :deep(.comment) {
      --n-color: transparent;
      --n-border-color: transparent;

      margin-bottom: 0;
      border: 0;
      border-radius: 0;
      box-shadow: none;
    }

    :deep(.comment:nth-child(odd)) {
      background-color: color-mix(in srgb, var(--n-text-color) 3%, transparent);
    }

    :deep(.comment:nth-child(even)) {
      background-color: color-mix(in srgb, var(--n-text-color) 6%, transparent);
    }

    :deep(.comment:first-child) {
      border-radius: var(--radius-md) var(--radius-md) 0 0;
    }

    :deep(.comment:last-child) {
      border-radius: 0 0 var(--radius-md) var(--radius-md);
    }

    :deep(.comment:only-child) {
      border-radius: var(--radius-md);
    }

    :deep(.comment:hover) {
      background-color: color-mix(in srgb, var(--n-text-color) 10%, transparent);
      box-shadow: none;
    }

    :deep(.comment .n-card__content) {
      min-height: 82px;
      padding: 12px !important;
    }
  }

  .simiPlayList {
    :deep(.coverlists) {
      margin-top: 2px;
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

    .comments {
      :deep(.comment .n-card__content) {
        min-height: 76px;
        padding: 11px 8px !important;
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

  @media (max-width: 420px) {
    .left {
      .meta {
        .control {
          :deep(.n-button) {
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
    min-height: 82px;
    display: grid;
    grid-template-columns: 38px minmax(0, 1fr) 84px;
    align-items: center;
    gap: 14px;
    padding: 12px;
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
      min-height: 76px;

      > :nth-child(n + 3) {
        display: none;
      }
    }
  }
}
</style>
