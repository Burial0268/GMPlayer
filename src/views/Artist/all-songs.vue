<template>
  <div class="all-songs">
    <template v-if="artistId">
      <div class="detail-header">
        <span class="detail-kind">{{ $t("general.name.allSong") }}</span>
        <h1 class="detail-name">
          {{ artistName ? artistName : $t("general.name.unknownArtist") }}
        </h1>
        <div class="detail-stats" v-if="totalCount">
          <div class="num">
            <n-icon :depth="3" :component="MusicList" />
            <n-text>{{ $t("general.name.songSize", { size: totalCount }) }}</n-text>
          </div>
        </div>
      </div>
      <div class="song-panel">
        <DataLists :listData="artistData" />
      </div>
      <Pagination
        v-if="artistData[0]"
        :pageNumber="pageNumber"
        :totalCount="totalCount"
        @pageSizeChange="pageSizeChange"
        @pageNumberChange="pageNumberChange"
      />
    </template>
    <div class="empty-state" v-else>
      <h1 class="detail-name">{{ $t("general.name.noKeywords") }}</h1>
      <n-button strong secondary class="back-btn" @click="router.go(-1)">
        {{ $t("general.name.goBack") }}
      </n-button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { getArtistDetail, getArtistAllSongs } from "@/api/artist";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { transformSongData } from "@/utils/ncm/transformSongData";
import DataLists from "@/components/DataList/DataLists.vue";
import Pagination from "@/components/Pagination/index.vue";
import { MusicList } from "@icon-park/vue-next";
import { ArtistSongsSortOrder } from "@/api";

const { t } = useI18n();
const router = useRouter();

// 歌手信息
const artistId = ref(router.currentRoute.value.query.id);
const artistData = ref([]);
const artistName = ref(null);
const totalCount = ref(0);
const pagelimit = ref(30);
const pageNumber = ref(
  router.currentRoute.value.query.page ? Number(router.currentRoute.value.query.page) : 1,
);

// 获取歌手名称
const getArtistDetailData = (id: number) => {
  getArtistDetail(id).then((res) => {
    artistName.value = res.data.artist.name;
  });
};

// 获取歌手信息
const getArtistAllSongsData = (
  id: string | number | string[],
  limit = 30,
  offset = 0,
  order: ArtistSongsSortOrder = "hot",
) => {
  if (!id) return false;
  getArtistAllSongs(Number(id), limit, offset, order)
    .then((res) => {
      // 获取歌手名称
      getArtistDetailData(Number(id));
      // 全部歌曲数据
      if (res.songs[0]) {
        // 数据总数
        totalCount.value = res.total;
        // 列表数据
        artistData.value = transformSongData(res.songs, {
          offset: (pageNumber.value - 1) * pagelimit.value,
        });
      } else {
        $message.error(t("general.message.acquisitionFailed"));
      }
      // 请求后回顶
      if (typeof $scrollToTop !== "undefined") $scrollToTop();
    })
    .catch((err) => {
      router.go(-1);
      console.error(t("general.message.acquisitionFailed"), err);
      $message.error(t("general.message.acquisitionFailed"));
    });
};

// 监听路由参数变化
watch(
  () => router.currentRoute.value,
  (val) => {
    if (val.name === "all-songs") {
      artistId.value = val.query.id;
      pageNumber.value = Number(val.query.page ? val.query.page : 1);
      getArtistAllSongsData(
        artistId.value,
        pagelimit.value,
        pageNumber.value ? (pageNumber.value - 1) * pagelimit.value : 0,
      );
    }
  },
);

// 每页个数数据变化
const pageSizeChange = (val: number) => {
  pagelimit.value = val;
  getArtistAllSongsData(artistId.value, val, (pageNumber.value - 1) * pagelimit.value);
};

// 当前页数数据变化
const pageNumberChange = (val: number) => {
  router.push({
    path: "/all-songs",
    query: {
      id: artistId.value,
      page: val,
    },
  });
};

onMounted(() => {
  getArtistAllSongsData(artistId.value, pagelimit.value, (pageNumber.value - 1) * pagelimit.value);
});
</script>

<style lang="scss" scoped>
.all-songs {
  display: flex;
  flex-direction: column;
  gap: 20px;
  padding: 6px 0 32px;

  .detail-header {
    display: flex;
    flex-direction: column;
    min-width: 0;

    .detail-kind {
      margin-bottom: 7px;
      font-size: 11px;
      font-weight: 700;
      line-height: 1;
      text-transform: uppercase;
      color: rgb(var(--content-panel-accent-rgb, 128, 128, 128));
    }

    .detail-name {
      margin: 0;
      max-width: 780px;
      font-size: clamp(28px, 4vw, 44px);
      font-weight: 800;
      line-height: 1.08;
      letter-spacing: -0.02em;
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
  }

  .song-panel {
    --detail-song-list-radius: var(--radius-md);

    width: 100%;
    min-width: 0;

    :deep(.datalists .songs) {
      --n-color: transparent;
      --n-border-color: transparent;

      margin-bottom: 0;
      border: 0;
      border-radius: 0;
      background-color: transparent;
      box-shadow: none;
    }

    :deep(.datalists .songs:nth-child(odd)),
    :deep(.datalists .songs.song-row-odd) {
      background-color: color-mix(in srgb, var(--n-text-color) 3%, transparent);
    }

    :deep(.datalists .songs:nth-child(even)),
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
      padding: 8px 12px !important;
    }

    :deep(.datalists .songs .pic),
    :deep(.datalists .songs .num) {
      width: 38px;
      height: 38px;
      min-width: 38px;
      margin-right: 14px;
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

    :deep(.datalists .songs .action) {
      width: 76px;
    }
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    padding: 40px 0;

    .detail-name {
      margin: 0;
      font-size: clamp(24px, 3.4vw, 36px);
      font-weight: 800;
      line-height: 1.1;
      letter-spacing: -0.02em;
    }

    .back-btn {
      margin-top: 20px;
    }
  }

  @media (max-width: 768px) {
    gap: 14px;

    .song-panel {
      :deep(.datalists .songs .n-card__content) {
        min-height: 58px;
        padding: 9px 6px !important;
      }

      :deep(.datalists .songs .pic),
      :deep(.datalists .songs .num) {
        width: 42px;
        height: 42px;
        min-width: 42px;
        margin-right: 11px;
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
}
</style>
