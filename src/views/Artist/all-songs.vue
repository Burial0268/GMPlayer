<template>
  <div class="all-songs">
    <div class="title" v-if="artistId">
      <template v-if="artistData[0]">
        <span class="key">{{
          artistName ? artistName : $t("general.name.unknownArtist")
        }}</span>
        <span>{{ $t("general.name.allSong") }} </span>
      </template>
    </div>
    <div class="title" v-else>
      <span class="key">{{ $t("general.name.noKeywords") }}</span>
      <br />
      <n-button
        strong
        secondary
        @click="router.go(-1)"
        style="margin-top: 20px"
      >
        {{ $t("general.name.goBack") }}
      </n-button>
    </div>
    <div class="songs" v-if="artistId">
      <DataLists :listData="artistData" />
      <Pagination
        v-if="artistData[0]"
        :pageNumber="pageNumber"
        :totalCount="totalCount"
        @pageSizeChange="pageSizeChange"
        @pageNumberChange="pageNumberChange"
      />
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
  router.currentRoute.value.query.page
    ? Number(router.currentRoute.value.query.page)
    : 1
);

// 获取歌手名称
const getArtistDetailData = (id: number) => {
  getArtistDetail(id).then((res) => {
    artistName.value = res.data.artist.name;
  });
};

// 获取歌手信息
const getArtistAllSongsData = (id: string | number | string[], limit = 30, offset = 0, order: ArtistSongsSortOrder = "hot") => {
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
    if (val.name == "all-songs") {
      artistId.value = val.query.id;
      pageNumber.value = Number(val.query.page ? val.query.page : 1);
      getArtistAllSongsData(
        artistId.value,
        pagelimit.value,
        pageNumber.value ? (pageNumber.value - 1) * pagelimit.value : 0
      );
    }
  }
);

// 每页个数数据变化
const pageSizeChange = (val: number) => {
  pagelimit.value = val;
  getArtistAllSongsData(
    artistId.value,
    val,
    (pageNumber.value - 1) * pagelimit.value
  );
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
  getArtistAllSongsData(
    artistId.value,
    pagelimit.value,
    (pageNumber.value - 1) * pagelimit.value
  );
});
</script>

<style lang="scss" scoped>
.all-songs {
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
}
</style>
