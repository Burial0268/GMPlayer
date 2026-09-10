<template>
  <div class="videos">
    <PageLoadState v-if="error" error @retry="retry" />
    <VideoLists v-else :listData="artistData" :loading="loading" />
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
import { getArtistVideos } from "@/api/artist";
import { formatNumber, getSongTime } from "@/utils/timeTools";
import VideoLists from "@/components/DataList/VideoLists.vue";
import Pagination from "@/components/Pagination/index.vue";

import PageLoadState from "@/components/Navigation/PageLoadState.vue";
import { useRoutePagination } from "@/composables/useRoutePagination";
const props = defineProps({
  // 视频总数
  mvSize: {
    type: Number,
    default: 0,
  },
});

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
  routeName: "ar-videos",
  load: async ({ route, page, limit, hiddenBar }) => {
    const res = await getArtistVideos(Number(route.query.id), limit, (page - 1) * limit, {
      hiddenBar,
    });
    return {
      total: props.mvSize,
      items: (res.mvs ?? []).map((item: any) => ({
        id: item.id,
        cover: item.imgurl16v9,
        name: item.name,
        artist: [item.artist],
        playCount: formatNumber(item.playCount),
        duration: getSongTime(item.duration),
      })),
    };
  },
});
</script>
