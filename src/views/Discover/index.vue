<template>
  <div class="discover">
    <!-- 与搜索页、我的页同一套头部与分段标签 -->
    <header class="discover-head">
      <h1 class="title">{{ $t("nav.discover") }}</h1>
    </header>
    <n-tabs
      class="main-tab discover-tab"
      type="segment"
      @update:value="tabChange"
      v-model:value="tabValue"
    >
      <n-tab name="playlists">{{ $t("nav.discoverChildren.playlists") }}</n-tab>
      <n-tab name="toplists">{{ $t("nav.discoverChildren.toplists") }}</n-tab>
      <n-tab name="artists">{{ $t("nav.discoverChildren.artists") }}</n-tab>
    </n-tabs>
    <div class="content">
      <router-view v-slot="{ Component }">
        <Transition :name="transitionName" mode="out-in">
          <keep-alive>
            <component :is="Component" />
          </keep-alive>
        </Transition>
      </router-view>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useRoute } from "vue-router";
import { useLayerNavigation } from "@/utils/navigation";
import { useTabTransition } from "@/composables/useTabTransition";

const route = useRoute();
const navigation = useLayerNavigation();
const { transitionName, updateDirection, syncIndex } = useTabTransition([
  "playlists",
  "toplists",
  "artists",
]);

// Tab 默认选中
const tabValue = ref(route.path.split("/")[2]);
syncIndex(tabValue.value);

// Tab 选项卡变化
const tabChange = (value: string) => {
  updateDirection(value);
  navigation.replacePage({
    path: `/discover/${value}`,
  });
};

// 监听路由参数变化
watch(
  () => route.path,
  (path) => {
    tabValue.value = path.split("/")[2];
    syncIndex(tabValue.value);
  },
);
</script>

<style lang="scss" scoped>
.discover {
  display: flex;
  flex-direction: column;
  padding: 6px 0 32px;

  .discover-head .title {
    margin: 0;
    font-size: clamp(30px, 4.6vw, 50px);
    font-weight: 700;
    line-height: 1.08;
    letter-spacing: -0.02em;
    color: var(--n-text-color);
  }

  .discover-tab {
    margin-top: 22px;
    max-width: 480px;
  }

  .content {
    position: relative;
    overflow: hidden;
    margin-top: 24px;
  }
}
</style>
