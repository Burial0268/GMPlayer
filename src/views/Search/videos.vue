<template>
  <div class="videos">
    <PageLoadState v-if="error" error @retry="retry" />
    <VideoLists v-else :listData="searchData" :loading="loading" />
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
import VideoLists from "@/components/DataList/VideoLists.vue";
import PageLoadState from "@/components/Navigation/PageLoadState.vue";
import Pagination from "@/components/Pagination/index.vue";
import { useSearchResults } from "@/composables/useSearchResults";
import { formatNumber, getSongTime } from "@/utils/timeTools";

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
  category: "videos",
  type: 1004,
  items: "mvs",
  total: "mvCount",
  map: (items) =>
    items.map((item) => ({
      id: item.id,
      cover: item.cover,
      name: item.name,
      artist: item.artists,
      playCount: formatNumber(item.playCount),
      duration: getSongTime(item.duration),
    })),
});
</script>
