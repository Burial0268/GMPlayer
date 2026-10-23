<template>
  <div class="playlists">
    <PageLoadState v-if="error" error @retry="retry" />
    <CoverLists v-else :listData="searchData" :loading="loading" />
    <Pagination
      v-if="searchData[0]"
      :pageNumber="pageNumber"
      :totalCount="totalCount"
      @pageSizeChange="pageSizeChange"
      @pageNumberChange="pageNumberChange"
    />
  </div>
</template>

<script setup lang="ts">
import CoverLists from "@/components/DataList/CoverLists.vue";
import PageLoadState from "@/components/Navigation/PageLoadState.vue";
import Pagination from "@/components/Pagination/index.vue";
import { useSearchResults } from "@/composables/useSearchResults";
import { formatNumber } from "@/utils/timeTools";

const {
  items: searchData,
  loading,
  error,
  retry,
  totalCount,
  pageNumber,
  pageSizeChange,
  pageNumberChange,
} = useSearchResults({
  category: "playlists",
  type: 1000,
  items: "playlists",
  total: "playlistCount",
  map: (items) =>
    items.map((item) => ({
      id: item.id,
      cover: item.coverImgUrl,
      name: item.name,
      artist: item.creator,
      playCount: formatNumber(item.playCount),
    })),
});
</script>
