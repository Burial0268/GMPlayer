<template>
  <div class="artist" v-if="artistId && artistData">
    <div class="left">
      <RouteArtwork class="cover" data-navigation-cover="page">
        <RouteCover
          class="coverImg"
          :src="getCoverUrl(safeArtistCover, 1024)"
          :preview-src="getCoverUrl(safeArtistCover)"
        />
        <RouteShadow class="shadow" :src="getCoverUrl(safeArtistCover, 512)" />
      </RouteArtwork>

      <div class="meta">
        <div class="title">
          <span v-content-intro class="detail-kind">{{ $t("general.name.artists") }}</span>
          <n-text class="name" data-navigation-title="page">{{ artistData.name }}</n-text>
          <n-text v-if="artistData.occupation" v-content-intro class="creator">
            {{ artistData.occupation }}
          </n-text>
        </div>

        <div v-content-intro class="detail-stats">
          <button class="num" type="button" @click="tabChange('songs')">
            <n-icon :depth="3" :component="MusicNoteFilled" />
            <n-text>{{ $t("general.name.songSize", { size: artistData.musicSize }) }}</n-text>
          </button>
          <button class="num" type="button" @click="tabChange('albums')">
            <n-icon :depth="3" :component="AlbumFilled" />
            <n-text>{{ $t("general.name.albumSize", { size: artistData.albumSize }) }}</n-text>
          </button>
          <button class="num" type="button" @click="tabChange('videos')">
            <n-icon :depth="3" :component="VideocamRound" />
            <n-text>{{ $t("general.name.mvSize", { size: artistData.mvSize }) }}</n-text>
          </button>
        </div>

        <div v-content-intro class="intr" v-if="artistData.desc">
          <span class="name">{{ $t("general.name.artistDesc") }}</span>
          <span class="desc text-hidden">{{ artistData.desc }}</span>
          <n-button class="all-desc" strong secondary @click="artistDescShow = true">
            {{ $t("general.name.allDesc") }}
          </n-button>
        </div>

        <n-space v-content-intro class="control">
          <n-button
            strong
            secondary
            round
            type="primary"
            :loading="artistSongsLoading"
            :disabled="!artistData.musicSize"
            @click="playArtistSongs"
          >
            <template #icon>
              <n-icon :component="PlayArrowRound" />
            </template>
            {{ $t("general.name.play") }}
          </n-button>
          <n-button
            v-if="user.userLogin"
            class="icon-action"
            strong
            secondary
            circle
            :type="artistLikeBtn ? 'default' : 'primary'"
            @click="toLikeArtist(artistData)"
          >
            <template #icon>
              <n-icon :component="artistLikeBtn ? PersonAddAlt1Round : PersonRemoveAlt1Round" />
            </template>
          </n-button>
          <n-button
            v-if="artistData.desc"
            class="icon-action"
            strong
            secondary
            circle
            @click="artistDescShow = true"
          >
            <template #icon>
              <n-icon :component="MoreHorizRound" />
            </template>
          </n-button>
        </n-space>
      </div>
    </div>

    <n-tabs
      v-content-intro
      class="main-tab"
      type="line"
      @update:value="tabChange"
      v-model:value="tabValue"
    >
      <n-tab name="songs"> {{ $t("general.name.hotSong") }} </n-tab>
      <n-tab name="albums"> {{ $t("general.name.album") }} </n-tab>
      <n-tab name="videos"> MV </n-tab>
    </n-tabs>

    <main class="content">
      <router-view v-slot="{ Component }" :mvSize="artistData ? artistData.mvSize : null">
        <Transition :name="transitionName" mode="out-in">
          <keep-alive :max="3">
            <component :is="Component" />
          </keep-alive>
        </Transition>
      </router-view>
    </main>

    <n-modal
      class="s-modal"
      v-model:show="artistDescShow"
      preset="card"
      :title="$t('general.name.artistDesc')"
      :bordered="false"
    >
      <n-scrollbar>
        <n-text v-html="artistData.desc.replace(/\n/g, '<br>')" />
      </n-scrollbar>
    </n-modal>
  </div>

  <div class="title" v-else-if="!artistId">
    <span class="key">{{ $t("general.name.noKeywords") }}</span>
    <br />
    <n-button strong secondary @click="router.go(-1)" style="margin-top: 20px">
      {{ $t("general.name.goBack") }}
    </n-button>
  </div>
  <PageLoadState v-else-if="loadFailed" error @retry="getArtistDetailData(artistId)" />
  <DetailPageSkeleton v-else kind="artist" />
</template>

<script setup lang="ts">
import { useRouter, useRoute } from "vue-router";
import { userStore } from "@/store";
import { getArtistDetail, getArtistSongs, likeArtist } from "@/api/artist";
import {
  MusicNoteFilled,
  AlbumFilled,
  VideocamRound,
  PersonAddAlt1Round,
  PersonRemoveAlt1Round,
  PlayArrowRound,
  MoreHorizRound,
} from "@vicons/material";
import { useI18n } from "vue-i18n";
import { useTabTransition } from "@/composables/useTabTransition";
import { usePlayAllSong } from "@/composables/usePlayAllSong";
import { useContentPanelAccent } from "@/composables/useContentPanelAccent";
import { transformSongData } from "@/utils/ncm/transformSongData";
import getCoverUrl from "@/utils/ncm/getCoverUrl";
import DetailPageSkeleton from "@/components/Navigation/DetailPageSkeleton.vue";
import RouteArtwork from "@/components/Navigation/RouteArtwork.vue";
import RouteCover from "@/components/Navigation/RouteCover.vue";
import RouteShadow from "@/components/Navigation/RouteShadow.vue";
import PageLoadState from "@/components/Navigation/PageLoadState.vue";
import { useLayerNavigation } from "@/utils/navigation";
import { useContentIntro } from "@/composables/useContentIntro";

const { t } = useI18n();
const router = useRouter();
const route = useRoute();
const navigation = useLayerNavigation();
const { vContentIntro } = useContentIntro();
const user = userStore();
const { playAllSong } = usePlayAllSong();
const { applyContentPanelAccent } = useContentPanelAccent();
const { transitionName, updateDirection, syncIndex } = useTabTransition([
  "songs",
  "albums",
  "videos",
]);

interface ArtistDetailData {
  id: number;
  name: string;
  occupation: string | null;
  cover: string;
  desc: string;
  albumSize: number;
  musicSize: number;
  mvSize: number;
}

const artistId = ref(route.query.id);
const loadFailed = ref(false);
const artistData = ref<ArtistDetailData | null>(null);
const artistHotSongs = ref<any[]>([]);
const artistSongsLoading = ref(false);
const artistDescShow = ref(false);
const artistLikeBtn = ref(false);

const safeArtistCover = computed(() =>
  artistData.value?.cover
    ? artistData.value.cover.replace(/^http:/, "https:")
    : "/images/pic/default.png",
);

const tabValue = ref(route.path.split("/")[2]);
syncIndex(tabValue.value);

const getArtistDetailData = (id: string | number | string[]) => {
  if (!id) return;
  loadFailed.value = false;
  getArtistDetail(Number(id), { hiddenBar: true })
    .then((res) => {
      artistData.value = {
        id: res.data.artist.id,
        name: res.data.artist.name,
        occupation: res.data.identify ? res.data.identify.imageDesc : null,
        cover: res.data.artist.cover,
        desc: res.data.artist.briefDesc,
        albumSize: res.data.artist.albumSize,
        musicSize: res.data.artist.musicSize,
        mvSize: res.data.artist.mvSize,
      };
      artistHotSongs.value = [];
      applyContentPanelAccent(getCoverUrl(res.data.artist.cover, 256));
      if (
        router.currentRoute.value.path.startsWith("/artist/") &&
        String(router.currentRoute.value.query.id) === String(id)
      ) {
        $setSiteTitle(res.data.artist.name + " - " + t("general.name.artists"));
      }
    })
    .catch((err) => {
      loadFailed.value = true;
      console.warn("[artist] detail failed to load", err);
    });
};

const playArtistSongs = async () => {
  if (!artistId.value || artistSongsLoading.value) return;
  artistSongsLoading.value = true;
  try {
    if (!artistHotSongs.value.length) {
      const res = await getArtistSongs(Number(artistId.value));
      artistHotSongs.value = res.hotSongs?.length ? transformSongData(res.hotSongs) : [];
    }
    if (artistHotSongs.value.length) {
      playAllSong(artistHotSongs.value);
    } else {
      $message.warning(t("general.message.acquisitionFailed"));
    }
  } catch (err) {
    console.error(t("general.message.acquisitionFailed"), err);
    $message.error(t("general.message.acquisitionFailed"));
  } finally {
    artistSongsLoading.value = false;
  }
};

const tabChange = (value: any) => {
  updateDirection(value);
  navigation.replacePage({
    path: `/artist/${value}`,
    query: {
      id: artistId.value,
      page: 1,
    },
  });
};

const isLikeOrDislike = (id: string | string[]) => {
  return !user.getUserArtistIds.has(Number(id));
};

const toLikeArtist = (data: { id: number; name: any }) => {
  const type = isLikeOrDislike(data.id.toString()) ? 1 : 2;
  likeArtist(type, data.id).then((res) => {
    if (res.code === 200) {
      $message.success(
        `${data.name} ${
          type === 1
            ? t("menu.collection", { name: t("general.dialog.success") })
            : t("menu.cancelCollection", { name: t("general.dialog.success") })
        }`,
      );
      user.setUserArtistLists(() => {
        artistLikeBtn.value = isLikeOrDislike(artistId.value);
      });
    } else {
      $message.error(
        `${data.name} ${
          type === 1
            ? t("menu.collection", { name: t("general.dialog.failed") })
            : t("menu.cancelCollection", { name: t("general.dialog.failed") })
        }`,
      );
    }
  });
};

onMounted(() => {
  getArtistDetailData(artistId.value);
  artistLikeBtn.value = isLikeOrDislike(artistId.value);
  if (user.userLogin && !user.getUserArtistLists.has && !user.getUserArtistLists.isLoading) {
    user.setUserArtistLists(() => {
      artistLikeBtn.value = isLikeOrDislike(artistId.value);
    });
  }
});

watch(
  () => route.path,
  (path) => {
    tabValue.value = path.split("/")[2];
    syncIndex(tabValue.value);
  },
);
</script>

<style lang="scss" scoped>
.artist {
  display: flex;
  flex-direction: column;
  gap: 18px;
  padding: 10px clamp(16px, 3vw, 36px) 36px;

  .left {
    width: 100%;
    min-height: 0;
    position: relative;
    display: grid;
    grid-template-columns: minmax(176px, 278px) minmax(0, 1fr);
    align-items: center;
    gap: clamp(22px, 4vw, 38px);
    padding: 18px 2px 18px;

    .cover {
      position: relative;
      display: flex;
      align-items: center;
      justify-content: flex-start;
      width: 100%;
      aspect-ratio: 1 / 1;

      &:active {
        transform: scale(0.95);
      }

      .coverImg {
        width: 100%;
        height: 100%;
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
        aspect-ratio: 1 / 1;

        :deep(img) {
          width: 100%;
          height: 100%;
          border-radius: var(--radius-md);
        }
      }
    }

    .meta {
      width: 100%;
      min-width: 0;
      display: flex;
      flex-direction: column;
      justify-content: flex-end;

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
          appearance: none;
          display: flex;
          align-items: center;
          min-width: 0;
          padding: 0;
          border: 0;
          background: transparent;
          color: inherit;
          font-size: 13px;
          cursor: pointer;

          .n-icon {
            flex: 0 0 auto;
            margin-right: 5px;
          }

          &:hover {
            color: rgb(var(--content-panel-accent-rgb, 128, 128, 128));
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

          height: 34px;
          border: 1px solid rgba(var(--content-panel-on-button-rgb, 18, 18, 22), 0.16);
          box-shadow:
            inset 0 1px 0 rgba(255, 255, 255, 0.26),
            inset 0 0 0 1px rgba(var(--content-panel-button-rgb, 226, 154, 128), 0.2),
            0 8px 18px rgba(var(--content-panel-accent-rgb, 0, 0, 0), 0.12);
          font-weight: 700;
        }

        :deep(.n-button:not(.icon-action)) {
          min-width: 112px;
        }

        :deep(.icon-action) {
          width: 34px;
          min-width: 34px;
        }

        :deep(.n-button .n-button__border),
        :deep(.n-button .n-button__state-border) {
          border-color: transparent !important;
        }
      }
    }
  }

  .main-tab {
    margin-top: 0;
  }

  .content {
    position: relative;
    overflow: hidden;
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
        .title {
          .detail-kind {
            margin-bottom: 7px;
          }

          .name {
            font-size: clamp(25px, 8vw, 36px);
            line-height: normal;
          }

          .creator {
            margin-top: 9px;
            font-size: 14px;
          }
        }

        .detail-stats {
          margin-top: 12px;
        }

        .intr {
          margin-top: 16px;
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

  @media (max-width: 540px) {
    .left {
      .cover {
        width: min(64vw, 235px);
      }
    }
  }
}

.title {
  margin-top: 30px;
  margin-bottom: 20px;
  font-size: 24px;

  .key {
    margin-right: 8px;
    font-size: 40px;
    font-weight: bold;
  }
}
</style>
