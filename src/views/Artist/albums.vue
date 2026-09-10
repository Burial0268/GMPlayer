<template>
  <div class="albums">
    <PageLoadState v-if="error" error @retry="retry" />
    <CoverLists v-else :listData="artistData" listType="album" :loading="loading" />
    <Pagination
      v-if="artistData.length"
      :totalCount="totalCount"
      :pageNumber="pageNumber"
      @pageSizeChange="pageSizeChange"
      @pageNumberChange="pageNumberChange"
    />
  </div>
</template>

<script setup lang="ts">
import { getArtistAlbums } from "@/api/album";
import { getLongTime } from "@/utils/timeTools";
import CoverLists from "@/components/DataList/CoverLists.vue";
import Pagination from "@/components/Pagination/index.vue";
import PageLoadState from "@/components/Navigation/PageLoadState.vue";
import { useRoutePagination } from "@/composables/useRoutePagination";

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
  routeName: "ar-albums",
  load: async ({ route, page, limit, hiddenBar }) => {
    const res = await getArtistAlbums(Number(route.query.id), limit, (page - 1) * limit, {
      hiddenBar,
    });
    return {
      total: res.artist.albumSize,
      items: (res.hotAlbums ?? []).map((item: any) => ({
        id: item.id,
        cover: item.picUrl,
        name: item.name,
        artist: item.artists,
        time: getLongTime(item.publishTime),
      })),
    };
  },
});
</script>

<style lang="scss" scoped>
.albums {
  padding-top: 10px;
}
</style>
