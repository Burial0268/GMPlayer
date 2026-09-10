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
      <!--
        Mobile's only route into the local library: the bottom bar's "library" tab
        goes to `/user` once signed in, so without this there is no way back to
        `/local` on a phone (the sidebar entry is desktop-only).
      -->
      <n-button
        v-if="isTauriRuntime()"
        class="local-btn"
        strong
        secondary
        round
        @click="navigation.openPage('/local', { origin: $event })"
      >
        <template #icon>
          <n-icon :component="FolderMusic" />
        </template>
        {{ $t("sidebar.localMusic") }}
      </n-button>
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
import { useRoute } from "vue-router";
import { useLayerNavigation } from "@/utils/navigation";
import { useI18n } from "vue-i18n";
import { FolderMusic, Logout } from "@icon-park/vue-next";
import { useTabTransition } from "@/composables/useTabTransition";
import { isTauri as isTauriRuntime } from "@/utils/tauri/core/runtime";

const { t } = useI18n();
const route = useRoute();
const navigation = useLayerNavigation();
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
      navigation.switchRoot("home", "/");
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
const tabValue = ref(route.path.split("/")[2]);
syncIndex(tabValue.value);

// Tab 选项卡变化
const tabChange = (value) => {
  updateDirection(value);
  navigation.replacePage({
    path: `/user/${value}`,
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
    .local-btn {
      margin-left: auto;
      flex-shrink: 0;
      align-self: center;
    }
    .logout-btn {
      // `margin-left: auto` on whichever of the two comes first, so the pair
      // stays right-aligned when the local button is absent (web build).
      margin-left: 10px;
      flex-shrink: 0;
      align-self: center;
      &:first-of-type {
        margin-left: auto;
      }
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
