<template>
  <div class="albums">
    <PageLoadState v-if="error" error @retry="retry" />
    <CoverLists v-else :listData="searchData" listType="album" :loading="loading" />
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
import { getLongTime } from "@/utils/timeTools";

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
  category: "albums",
  type: 10,
  items: "albums",
  total: "albumCount",
  map: (items) =>
    items.map((item) => ({
      id: item.id,
      cover: item.picUrl,
      name: item.name,
      artist: item.artists,
      time: getLongTime(item.publishTime),
    })),
});
</script>
