<template>
  <div class="search">
    <!-- 结果头部 -->
    <header class="search-head" v-if="searchKeywords">
      <span class="eyebrow">{{ $t("nav.search.resultsLabel") }}</span>
      <h1 class="title">{{ keywordText }}</h1>
    </header>

    <!-- 无关键词 -->
    <div class="search-empty" v-else>
      <div class="empty-icon">
        <n-icon :component="Search" />
      </div>
      <h2 class="empty-title">{{ $t("general.name.noKeywords") }}</h2>
      <p class="empty-desc">{{ $t("nav.search.searchTip") }}</p>
      <n-button strong secondary round class="empty-btn" @click="router.go(-1)">
        {{ $t("general.name.goBack") }}
      </n-button>
    </div>

    <!-- 分类切换 -->
    <n-tabs
      class="main-tab search-tab"
      type="line"
      @update:value="tabChange"
      v-model:value="tabValue"
      v-if="searchKeywords"
    >
      <n-tab name="songs">{{ $t("general.name.song") }}</n-tab>
      <n-tab name="artists">{{ $t("general.name.artists") }}</n-tab>
      <n-tab name="albums">{{ $t("general.name.album") }}</n-tab>
      <n-tab name="videos">{{ $t("general.name.videos") }}</n-tab>
      <n-tab name="playlists">{{ $t("general.name.playlist") }}</n-tab>
    </n-tabs>

    <!-- 结果内容 -->
    <main class="content" v-if="searchKeywords">
      <router-view v-slot="{ Component }">
        <Transition :name="transitionName" mode="out-in">
          <keep-alive>
            <component :is="Component" />
          </keep-alive>
        </Transition>
      </router-view>
    </main>
  </div>
</template>

<script setup lang="ts">
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { Search } from "@icon-park/vue-next";
import { useTabTransition } from "@/composables/useTabTransition";

const { t } = useI18n();
const router = useRouter();
const { transitionName, updateDirection, syncIndex } = useTabTransition([
  "songs",
  "artists",
  "albums",
  "videos",
  "playlists",
]);

// 搜索关键词
const searchKeywords = ref(router.currentRoute.value.query.keywords);
const keywordText = computed(() => String(searchKeywords.value ?? ""));

// Tab 默认选中
const tabValue = ref(router.currentRoute.value.path.split("/")[2]);
syncIndex(tabValue.value);

// 监听路由参数变化
watch(
  () => router.currentRoute.value,
  (val) => {
    if (!val.path.startsWith("/search")) return;
    $setSiteTitle(val.query.keywords + " " + t("nav.search.results"));
    searchKeywords.value = val.query.keywords;
    tabValue.value = val.path.split("/")[2];
    syncIndex(tabValue.value);
  },
);

// Tab 选项卡变化
const tabChange = (value: string) => {
  updateDirection(value);
  router.push({
    path: `/search/${value}`,
    query: {
      keywords: searchKeywords.value,
      page: 1,
    },
  });
};

onMounted(() => {
  if (searchKeywords.value) $setSiteTitle(keywordText.value + " " + t("nav.search.results"));
});
</script>

<style lang="scss" scoped>
.search {
  display: flex;
  flex-direction: column;
  padding: 6px 0 32px;

  // 结果头部
  .search-head {
    display: flex;
    flex-direction: column;
    min-width: 0;

    .eyebrow {
      margin-bottom: 10px;
      font-size: 12px;
      font-weight: 600;
      line-height: 1;
      letter-spacing: 0.08em;
      text-transform: uppercase;
      color: var(--n-text-color-3);
    }

    .title {
      margin: 0;
      max-width: 900px;
      font-size: clamp(30px, 4.6vw, 50px);
      font-weight: 800;
      line-height: 1.08;
      letter-spacing: -0.02em;
      color: var(--n-text-color);
      word-break: break-word;
    }
  }

  // 无关键词占位
  .search-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    padding: clamp(48px, 12vh, 120px) 0;

    .empty-icon {
      display: grid;
      place-items: center;
      width: 68px;
      height: 68px;
      margin-bottom: 20px;
      border-radius: var(--radius-panel);
      background-color: color-mix(in srgb, var(--n-text-color) 6%, transparent);
      color: var(--n-text-color-3);
      font-size: 30px;
    }

    .empty-title {
      margin: 0;
      font-size: clamp(22px, 3vw, 30px);
      font-weight: 800;
      line-height: 1.1;
      letter-spacing: -0.02em;
    }

    .empty-desc {
      margin: 10px 0 0;
      max-width: 360px;
      font-size: 14px;
      line-height: 1.6;
      color: var(--n-text-color-3);
    }

    .empty-btn {
      margin-top: 22px;
    }
  }

  .search-tab {
    margin-top: 22px;
  }

  .content {
    position: relative;
    overflow: hidden;
    margin-top: 24px;
  }
}
</style>
