<template>
  <div class="user">
    <header class="user-head">
      <n-avatar
        class="avatar"
        round
        :src="
          user.getUserData.avatarUrl
            ? user.getUserData.avatarUrl.replace(/^http:/, 'https:') + '?param=200y200'
            : '/images/ico/user-filling.svg'
        "
        fallback-src="/images/ico/user-filling.svg"
      />
      <div class="text">
        <span class="eyebrow s-label">{{ $t("nav.user") }}</span>
        <h1 class="title text-hidden">{{ user.getUserData.nickname }}</h1>
      </div>
      <!-- 本地音乐入口与退出登录收进一个菜单：并排两颗按钮在手机上会挤出头部。 -->
      <n-dropdown
        trigger="click"
        placement="bottom-end"
        :options="menuOptions"
        @select="handleMenuSelect"
      >
        <n-button class="more" circle strong secondary :aria-label="$t('nav.userChildren.more')">
          <template #icon>
            <n-icon :component="More" />
          </template>
        </n-button>
      </n-dropdown>
    </header>

    <n-tabs
      class="main-tab user-tab"
      type="segment"
      @update:value="tabChange"
      v-model:value="tabValue"
    >
      <n-tab v-for="tab in TABS" :key="tab" :name="tab">
        {{ $t(`nav.userChildren.tab.${TAB_LABELS[tab]}`) }}
      </n-tab>
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
import { NIcon } from "naive-ui";
import { userStore } from "@/store";
import { useRoute } from "vue-router";
import { useLayerNavigation } from "@/utils/navigation";
import { useI18n } from "vue-i18n";
import { FolderMusic, Logout, More } from "@icon-park/vue-next";
import { useTabTransition } from "@/composables/useTabTransition";
import { isTauri as isTauriRuntime } from "@/utils/tauri/core/runtime";

const { t } = useI18n();
const route = useRoute();
const navigation = useLayerNavigation();
const user = userStore();

const TABS = ["playlists", "like", "album", "artists", "cloud"];
const TAB_LABELS: Record<string, string> = {
  playlists: "playlist",
  like: "like",
  album: "album",
  artists: "artist",
  cloud: "cloud",
};

const renderIcon = (icon) => () => h(NIcon, null, { default: () => h(icon) });

// 本地音乐只在 Tauri 上存在。这是手机上回到 `/local` 的唯一入口：登录后底栏的
// 「音乐库」指向 `/user`，侧栏的本地音乐又只在桌面显示。
const menuOptions = computed(() => [
  ...(isTauriRuntime()
    ? [{ key: "local", label: t("sidebar.localMusic"), icon: renderIcon(FolderMusic) }]
    : []),
  { key: "logout", label: t("nav.avatar.logout"), icon: renderIcon(Logout) },
]);

const handleMenuSelect = (key: string) => {
  if (key === "local") navigation.openPage("/local");
  else if (key === "logout") handleLogout();
};

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

const { transitionName, updateDirection, syncIndex } = useTabTransition(TABS);

// Tab 默认选中
const tabValue = ref(route.path.split("/")[2]);
syncIndex(tabValue.value);

// Tab 选项卡变化
const tabChange = (value: string) => {
  updateDirection(value);
  navigation.replacePage({ path: `/user/${value}` });
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
  display: flex;
  flex-direction: column;
  padding: 6px 0 32px;

  // 与搜索结果页同一套头部：小标签 + 大标题，右侧只放一颗菜单按钮。
  .user-head {
    display: flex;
    align-items: center;
    gap: 16px;
    min-width: 0;

    .avatar {
      --size: clamp(52px, 7vw, 72px);
      width: var(--size);
      height: var(--size);
      flex-shrink: 0;
      box-shadow: 0 6px 8px -2px rgb(0 0 0 / 16%);
    }

    .text {
      display: flex;
      flex-direction: column;
      flex: 1;
      min-width: 0;

      .eyebrow {
        margin-bottom: 10px;
        color: var(--n-text-color-3);
      }

      .title {
        margin: 0;
        font-size: clamp(26px, 4.6vw, 50px);
        font-weight: 700;
        line-height: 1.08;
        letter-spacing: -0.02em;
        color: var(--n-text-color);
        word-break: break-all;
      }
    }

    .more {
      flex-shrink: 0;
    }
  }

  .user-tab {
    margin-top: 22px;
    max-width: 480px;
  }

  // `clip` 而不是 `hidden`：`hidden` 让它成为滚动容器，子页里的 sticky（DataLists 的定位
  // 药丸）就钉在这个永不滚动的盒子上。完整理由见 `views/Local/index.vue` 的同名规则。
  .content {
    position: relative;
    overflow: clip;
    margin-top: 24px;
  }
}
</style>
