<template>
  <div class="paplaylists">
    <div class="home-section-head">
      <h2 class="head-title">{{ $t("home.title.playlists") }}</h2>
      <span class="head-more" @click="router.push('/discover/playlists?page=1')">
        {{ $t("home.title.more") }}
      </span>
    </div>
    <CoverLists :listData="personalizedData" :loadingNum="12" :gridCollapsed="true" />
  </div>
</template>

<script setup>
import { getPersonalized } from "@/api/home";
import { useRouter } from "vue-router";
import { formatNumber } from "@/utils/timeTools";
import CoverLists from "@/components/DataList/CoverLists.vue";
const router = useRouter();

// 推荐数据
const personalizedData = ref([]);

// 获取推荐数据
const getPersonalizedData = (type = null, limit = 12) => {
  getPersonalized(type, limit).then((res) => {
    personalizedData.value = [];
    if (res.result) {
      res.result.forEach((v) => {
        personalizedData.value.push({
          id: v.id,
          cover: v.picUrl,
          name: v.name,
          playCount: formatNumber(v.playCount),
        });
      });
    } else {
      $message.error("歌单推荐内容为空");
    }
  });
};

onMounted(() => {
  getPersonalizedData();
});
</script>

<style lang="scss" scoped>
.paplaylists {
  padding: 0 4px;
  position: relative;
  transform: translateZ(0);
  perspective: 1px;
}
</style>
