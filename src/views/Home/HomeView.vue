<template>
  <div class="home">
    <div v-if="setting.bannerShow" class="home-section">
      <Banner />
    </div>
    <!-- 个性化推荐 -->
    <n-h3 class="title home-section" prefix="bar">{{ $t("home.title.exclusive") }}</n-h3>
    <n-grid class="recommend home-section" :x-gap="20" :cols="2">
      <n-gi class="rec-left">
        <!-- 每日推荐 -->
        <PaDailySongs />
        <!-- 其他推荐 -->
        <n-grid class="rec-func" x-gap="12" :cols="2">
          <n-gi>
            <PaRadar />
          </n-gi>
          <n-gi>
            <PaLikeSongs />
          </n-gi>
        </n-grid>
      </n-gi>
      <n-gi class="rec-right">
        <PaPersonalFm />
      </n-gi>
    </n-grid>
    <!-- 公共推荐 -->
    <div class="home-section">
      <PaPlayLists />
    </div>
    <div class="home-section">
      <PaArtists />
    </div>
    <div class="home-section">
      <PaAlbum />
    </div>
  </div>
</template>

<script setup lang="ts">
import { settingStore } from "@/store";
import Banner from "@/components/Banner/index.vue";
import PaPlayLists from "@/components/Personalized/PaPlayLists.vue";
import PaArtists from "@/components/Personalized/PaArtists.vue";
import PaAlbum from "@/components/Personalized/PaAlbum.vue";
import PaDailySongs from "@/components/Personalized/PaDailySongs.vue";
import PaPersonalFm from "@/components/Personalized/PaPersonalFm.vue";
import PaRadar from "@/components/Personalized/PaRadar.vue";
import PaLikeSongs from "@/components/Personalized/PaLikeSongs.vue";
import gsap from "gsap";
import { onMounted } from "vue";

const setting = settingStore();

onMounted(() => {
  if (typeof $setSiteTitle !== "undefined") $setSiteTitle(import.meta.env.VITE_SITE_TITLE);
  // 回顶
  if (typeof $scrollToTop !== "undefined") $scrollToTop();

  // GSAP 入场动画
  gsap.from(".home-section", {
    opacity: 0,
    y: 30,
    duration: 0.5,
    stagger: 0.15,
    ease: "power2.out",
  });
});
</script>

<style lang="scss" scoped>
.home {
  .title {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-left: 16px;
  }
  .recommend {
    @media (max-width: 850px) {
      grid-template-columns: repeat(1, minmax(0px, 1fr)) !important;
      gap: 20px 0 !important;
      .rec-left {
        display: flex;
        flex-direction: column-reverse;
        .padailysongs {
          margin-bottom: 0;
          margin-top: 20px;
        }
      }
    }
    .rec-left,
    .rec-right {
      height: 200px;
    }
    .rec-func {
      height: 70px;
    }
  }
}
// .home-section {
//   opacity: 0;
// }
</style>
