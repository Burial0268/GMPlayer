<template>
  <div class="paalbum">
    <div class="home-section-head">
      <h2 class="head-title">{{ $t("home.title.newAlbum") }}</h2>
      <span class="head-more" @click="router.push('/new-album?page=1')">
        {{ $t("home.title.more") }}
      </span>
    </div>
    <CoverLists listType="album" :listData="newAlbumData" :loadingNum="12" :gridCollapsed="true" />
  </div>
</template>

<script setup>
import { getNewAlbum } from "@/api/home";
import { useRouter } from "vue-router";
import { getLongTime } from "@/utils/timeTools";
import CoverLists from "@/components/DataList/CoverLists.vue";
const router = useRouter();

// 专辑数据
const newAlbumData = ref([]);

// 获取推荐数据
const getNewAlbumData = () => {
  getNewAlbum().then((res) => {
    newAlbumData.value = [];
    if (res.albums) {
      res.albums.forEach((v) => {
        newAlbumData.value.push({
          id: v.id,
          cover: v.picUrl,
          name: v.name,
          artist: [v.artist],
          time: getLongTime(v.publishTime),
        });
      });
    } else {
      $message.error("新碟上架内容为空");
    }
  });
};

onMounted(() => {
  getNewAlbumData();
});
</script>

<style lang="scss" scoped>
.paalbum {
  padding: 0 4px;
}
</style>
