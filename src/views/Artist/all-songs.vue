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
        <PageLoadState v-if="error" error @retry="retry" />
        <DataLists
          v-else
          :listData="artistData"
          :loading="loading"
          virtual
          virtual-height="min(68vh, 760px)"
          :virtual-item-size="54"
          :virtual-threshold="40"
        />
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
      <n-button strong secondary class="back-btn" @click="navigation.closeTop()">
        {{ $t("general.name.goBack") }}
      </n-button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { getArtistDetail, getArtistAllSongs } from "@/api/artist";
import { useRoute } from "vue-router";
import { transformSongData } from "@/utils/ncm/transformSongData";
import DataLists from "@/components/DataList/DataLists.vue";
import Pagination from "@/components/Pagination/index.vue";
import { MusicList } from "@icon-park/vue-next";
import { useRoutePagination } from "@/composables/useRoutePagination";
import { useLayerNavigation } from "@/utils/navigation";
import PageLoadState from "@/components/Navigation/PageLoadState.vue";

const navigation = useLayerNavigation();
const artistId = useRoute().query.id;
const artistName = ref("");
const {
  items: artistData,
  loading,
  error,
  retry,
  totalCount,
  pageNumber,
  pageSizeChange,
  pageNumberChange,
} = useRoutePagination({
  routeName: "all-songs",
  load: async ({ page, limit, hiddenBar }) => {
    if (!artistId) return { items: [], total: 0 };
    const offset = (page - 1) * limit;
    const [songs, detail] = await Promise.all([
      getArtistAllSongs(Number(artistId), limit, offset, "hot", { hiddenBar }),
      artistName.value ? undefined : getArtistDetail(Number(artistId), { hiddenBar: true }),
    ]);
    if (detail) artistName.value = detail.data.artist.name;
    return { items: transformSongData(songs.songs ?? [], { offset }), total: songs.total };
  },
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
