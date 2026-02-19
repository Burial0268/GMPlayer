<template>
  <div class="discover">
    <n-text class="title">{{ $t("nav.discover") }}</n-text>
    <n-tabs
      class="main-tab"
      type="segment"
      @update:value="tabChange"
      v-model:value="tabValue"
    >
      <n-tab name="playlists">{{ $t("nav.discoverChildren.playlists") }}</n-tab>
      <n-tab name="toplists">{{ $t("nav.discoverChildren.toplists") }}</n-tab>
      <n-tab name="artists">{{ $t("nav.discoverChildren.artists") }}</n-tab>
    </n-tabs>
    <main class="content">
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
import { useTabTransition } from "@/composables/useTabTransition";

const router = useRouter();
const { transitionName, updateDirection, syncIndex } = useTabTransition(["playlists", "toplists", "artists"]);

// Tab 默认选中
const tabValue = ref(router.currentRoute.value.path.split("/")[2]);
syncIndex(tabValue.value);

// Tab 选项卡变化
const tabChange = (value: string) => {
  updateDirection(value);
  router.push({
    path: `/discover/${value}`,
  });
};

// 监听路由参数变化
watch(
  () => router.currentRoute.value,
  (val) => {
    tabValue.value = val.path.split("/")[2];
    syncIndex(tabValue.value);
  }
);
</script>

<style lang="scss" scoped>
.discover {
  .title {
    display: block;
    margin: 30px 0 20px;
    font-size: 40px;
    font-weight: bold;
  }
  .content {
    position: relative;
    overflow: hidden;
    margin-top: 20px;
  }
}
</style>
