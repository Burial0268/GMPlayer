<template>
  <div class="user">
    <div class="title">
      <n-avatar
        class="avatar"
        round
        :src="
          user.getUserData.avatarUrl
            ? user.getUserData.avatarUrl.replace(/^http:/, 'https:')
            : '/images/ico/user-filling.svg'
        "
        fallback-src="/images/ico/user-filling.svg"
      />
      <div class="text">
        <n-text class="key">{{ user.getUserData.nickname }}</n-text>
        <n-text class="tip" v-html="$t('nav.userChildren.results')" />
      </div>
      <n-button class="logout-btn" strong secondary round type="error" @click="handleLogout">
        <template #icon>
          <n-icon :component="Logout" />
        </template>
        {{ $t("nav.avatar.logout") }}
      </n-button>
    </div>
    <n-tabs class="main-tab" type="line" @update:value="tabChange" v-model:value="tabValue">
      <n-tab name="playlists">{{ $t("nav.userChildren.playlist") }}</n-tab>
      <n-tab name="like">{{ $t("nav.userChildren.like") }}</n-tab>
      <n-tab name="album">{{ $t("nav.userChildren.album") }}</n-tab>
      <n-tab name="artists">{{ $t("nav.userChildren.artist") }}</n-tab>
      <n-tab name="cloud">{{ $t("nav.userChildren.cloud") }}</n-tab>
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
import { userStore } from "@/store";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { Logout } from "@icon-park/vue-next";
import { useTabTransition } from "@/composables/useTabTransition";

const { t } = useI18n();
const router = useRouter();
const user = userStore();

// 退出登录
const handleLogout = () => {
  $dialog.warning({
    class: "s-dialog",
    title: t("nav.avatar.logout"),
    content: t("nav.avatar.tip"),
    positiveText: t("nav.avatar.logout"),
    negativeText: t("general.dialog.cancel"),
    onPositiveClick: () => {
      user.userLogOut();
      $message.success(t("nav.avatar.success"));
      router.push("/");
    },
  });
};
const { transitionName, updateDirection, syncIndex } = useTabTransition([
  "playlists",
  "like",
  "album",
  "artists",
  "cloud",
]);

// Tab 默认选中
const tabValue = ref(router.currentRoute.value.path.split("/")[2]);
syncIndex(tabValue.value);

// Tab 选项卡变化
const tabChange = (value) => {
  updateDirection(value);
  router.push({
    path: `/user/${value}`,
  });
};

// 监听路由参数变化
watch(
  () => router.currentRoute.value,
  (val) => {
    tabValue.value = val.path.split("/")[2];
    syncIndex(tabValue.value);
  },
);
</script>

<style lang="scss" scoped>
.user {
  .title {
    margin-top: 30px;
    margin-bottom: 20px;
    font-size: 24px;
    display: flex;
    align-items: center;
    .avatar {
      width: 80px;
      height: 80px;
      min-width: 80px;
      margin-right: 16px;
      box-shadow: 0 6px 8px -2px rgb(0 0 0 / 16%);
    }
    .logout-btn {
      margin-left: auto;
      flex-shrink: 0;
      align-self: center;
    }
    .text {
      display: flex;
      align-items: center;
      .key {
        font-size: 40px;
        font-weight: bold;
        margin-right: 8px;
      }
      .tip {
        transform: translateY(8px);
      }
      @media (max-width: 620px) {
        flex-direction: column;
        align-items: flex-start;
        .key {
          font-size: 30px;
          margin-right: 0;
        }
        .tip {
          font-size: 18px;
          transform: translateY(0);
        }
      }
    }
  }
  .content {
    position: relative;
    overflow: hidden;
    margin-top: 20px;
  }
}
</style>
