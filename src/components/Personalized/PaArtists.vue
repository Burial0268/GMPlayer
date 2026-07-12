<template>
  <div class="paartists">
    <div class="home-section-head">
      <h2 class="head-title">{{ $t("home.title.artists") }}</h2>
      <n-tabs class="tab main-tab" :default-value="-1" size="small" @update:value="tabChange">
        <n-tab :name="-1"> {{ $t("general.type.all") }} </n-tab>
        <n-tab :name="7"> {{ $t("general.type.china") }} </n-tab>
        <n-tab :name="96"> {{ $t("general.type.western") }} </n-tab>
        <n-tab :name="8"> {{ $t("general.type.japan") }} </n-tab>
        <n-tab :name="16"> {{ $t("general.type.korea") }} </n-tab>
      </n-tabs>
      <span class="head-more" @click="router.push('/discover/artists?page=1')">
        {{ $t("home.title.more") }}
      </span>
    </div>
    <ArtistLists :listData="artistsData" :gridCollapsed="true" />
  </div>
</template>

<script setup>
import { getArtistList } from "@/api/artist";
import { useRouter } from "vue-router";
import ArtistLists from "@/components/DataList/ArtistLists.vue";

const router = useRouter();

// 歌手数据
const artistsData = ref([]);

// 获取歌手数据
const getArtistListData = (type = -1, area = -1, limit = 6) => {
  getArtistList(type, area, limit).then((res) => {
    artistsData.value = [];
    if (res.artists) {
      res.artists.forEach((v) => {
        artistsData.value.push({
          id: v.id,
          name: v.name,
          cover: v.img1v1Url,
          size: v.musicSize,
        });
      });
    } else {
      $message.error("推荐歌手内容为空");
    }
  });
};

// Tab 切换
const tabChange = (value) => {
  artistsData.value = [];
  getArtistListData(-1, value);
};

onMounted(() => {
  getArtistListData();
});
</script>

<style lang="scss" scoped>
.paartists {
  padding: 0 4px;
  .tab {
    width: auto;
    margin-right: auto;
    margin-left: 8px;
    @media (max-width: 440px) {
      display: none;
    }
    :deep(.n-tabs-tab-pad) {
      width: 12px;
    }
  }
}
</style>
