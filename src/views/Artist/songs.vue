<template>
  <div class="songs">
    <DataLists :listData="artistData" />
    <n-space justify="center" v-if="artistData[0]">
      <n-button
        class="more"
        size="large"
        strong
        secondary
        round
        @click="router.push(`/all-songs?id=${artistId}&page=1`)"
      >
        {{ $t("general.name.allSong") }}
      </n-button>
    </n-space>
  </div>
</template>

<script setup>
import { getArtistSongs } from "@/api/artist";
import { useRouter } from "vue-router";
import { transformSongData } from "@/utils/transformSongData";
import DataLists from "@/components/DataList/DataLists.vue";
const router = useRouter();

// 歌手数据
const artistId = ref(router.currentRoute.value.query.id);
const artistData = ref([]);

// 获取歌手热门歌曲
const getArtistSongsData = async (id) => {
  const res = await getArtistSongs(id);
  // transformSongData 内部自动从 al.pic_str 推算 picUrl，无需额外请求
  artistData.value = res.hotSongs?.length ? transformSongData(res.hotSongs) : [];
};

onMounted(() => {
  getArtistSongsData(artistId.value);
});

// 监听路由参数变化
watch(
  () => router.currentRoute.value,
  (val) => {
    artistId.value = val.query.id;
    if (val.name == "ar-songs") {
      getArtistSongsData(artistId.value);
    }
  }
);
</script>

<style lang="scss" scoped>
.songs {
  .more {
    margin-top: 40px;
    width: 140px;
    font-size: 16px;
    transition: all 0.3s;
    &:hover {
      background-color: var(--main-second-color);
      color: var(--main-color);
    }
    &:active {
      transform: scale(0.95);
    }
  }
}
</style>
