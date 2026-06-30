<template>
  <Motion
    tag="aside"
    :class="[
      'sidebar',
      {
        dark: setting.getSiteTheme === 'dark',
        collapsed: setting.sidebarCollapsed,
        'search-active': site.searchInputActive,
        'scroll-top-shadow': sidebarShowTopShadow,
        'scroll-bottom-shadow': sidebarShowBottomShadow,
      },
    ]"
    :animate="{ width: sidebarWidth, minWidth: sidebarWidth }"
    :transition="{ type: 'tween', duration: 0.22, ease: 'easeInOut' }"
    :data-tauri-drag-region="isTauri || undefined"
  >
    <!-- Header: search + collapse toggle -->
    <div :class="['sidebar-header', { collapsed: setting.sidebarCollapsed }]">
      <div :class="['sidebar-search', { hidden: setting.sidebarCollapsed }]">
        <SearchInp />
      </div>
      <n-tooltip placement="right" :disabled="!setting.sidebarCollapsed" :delay="300">
        <template #trigger>
          <n-icon
            class="sidebar-toggle"
            :size="18"
            :component="setting.sidebarCollapsed ? IndentRight : IndentLeft"
            @click="setting.sidebarCollapsed = !setting.sidebarCollapsed"
          />
        </template>
        {{ setting.sidebarCollapsed ? $t("sidebar.expand") : $t("sidebar.collapse") }}
      </n-tooltip>
    </div>

    <!-- Scrollable nav area -->
    <n-scrollbar
      v-if="!setting.sidebarCollapsed"
      class="sidebar-scroll"
      @scroll="handleSidebarScroll"
    >
      <!-- Menu section -->
      <div :class="['sidebar-section sidebar-primary', { collapsed: setting.sidebarCollapsed }]">
        <SidebarItem
          :to="'/'"
          :icon="HomeTwo"
          :label="$t('nav.home')"
          :collapsed="setting.sidebarCollapsed"
        />
        <SidebarItem
          :to="'/discover'"
          :icon="FindOne"
          :label="$t('nav.discover')"
          :collapsed="setting.sidebarCollapsed"
        />
        <SidebarItem
          v-if="user.userLogin"
          :to="'/dailySongs'"
          :icon="CalendarThirty"
          :label="$t('sidebar.dailySongs')"
          :collapsed="setting.sidebarCollapsed"
        />
      </div>

      <!-- Library section (login required) -->
      <div
        v-if="user.userLogin"
        :class="['sidebar-section', { collapsed: setting.sidebarCollapsed }]"
      >
        <div class="sidebar-section-header" @click="toggleSection('library')">
          <span>{{ $t("sidebar.library") }}</span>
          <n-icon
            class="section-chevron"
            :class="{ open: sectionOpen.library }"
            :size="13"
            :component="Right"
          />
        </div>
        <div v-show="sectionOpen.library" class="sidebar-section-content">
          <SidebarItem
            :to="'/user/playlists'"
            :icon="MusicList"
            :label="$t('sidebar.playlists')"
            :collapsed="setting.sidebarCollapsed"
          />
          <SidebarItem
            :to="'/user/album'"
            :icon="RecordDisc"
            :label="$t('sidebar.albums')"
            :collapsed="setting.sidebarCollapsed"
          />
          <SidebarItem
            :to="'/user/artists'"
            :icon="Voice"
            :label="$t('sidebar.artists')"
            :collapsed="setting.sidebarCollapsed"
          />
          <SidebarItem
            :to="'/user/cloud'"
            :icon="CloudStorage"
            :label="$t('sidebar.cloud')"
            :collapsed="setting.sidebarCollapsed"
          />
        </div>
      </div>

      <!-- My Playlists section (login required) -->
      <div
        v-if="user.userLogin"
        :class="['sidebar-section sidebar-playlists', { collapsed: setting.sidebarCollapsed }]"
      >
        <div class="sidebar-section-header" @click="toggleSection('own')">
          <span>{{ $t("sidebar.myPlaylists") }}</span>
          <n-icon
            class="section-chevron"
            :class="{ open: sectionOpen.own }"
            :size="13"
            :component="Right"
          />
        </div>
        <div v-show="sectionOpen.own" class="sidebar-section-content">
          <template v-if="user.getUserPlayLists.isLoading">
            <n-skeleton
              v-for="i in 3"
              :key="i"
              :height="32"
              :width="setting.sidebarCollapsed ? 32 : '100%'"
              :round="true"
              style="margin-bottom: 6px"
            />
          </template>
          <template v-else-if="user.getUserPlayLists.own.length">
            <SidebarPlaylistItem
              v-for="pl in user.getUserPlayLists.own"
              :key="pl.id"
              :id="pl.id"
              :cover="pl.cover"
              :name="pl.name"
              :collapsed="setting.sidebarCollapsed"
              @navigate="goToPlaylist"
            />
          </template>
          <AnimatePresence>
            <Motion
              v-if="
                !setting.sidebarCollapsed &&
                !user.getUserPlayLists.isLoading &&
                !user.getUserPlayLists.own.length
              "
              class="sidebar-empty"
              :initial="{ opacity: 0 }"
              :animate="{ opacity: 1 }"
              :exit="{ opacity: 0 }"
              :transition="{ duration: 0.15 }"
            >
              {{ $t("sidebar.noPlaylists") }}
            </Motion>
          </AnimatePresence>
        </div>
      </div>

      <!-- Liked Playlists section (login required) -->
      <div
        v-if="user.userLogin && user.getUserPlayLists.like.length"
        :class="['sidebar-section sidebar-playlists', { collapsed: setting.sidebarCollapsed }]"
      >
        <div class="sidebar-section-header" @click="toggleSection('liked')">
          <span>{{ $t("sidebar.likedPlaylists") }}</span>
          <n-icon
            class="section-chevron"
            :class="{ open: sectionOpen.liked }"
            :size="13"
            :component="Right"
          />
        </div>
        <div v-show="sectionOpen.liked" class="sidebar-section-content">
          <SidebarPlaylistItem
            v-for="pl in user.getUserPlayLists.like"
            :key="pl.id"
            :id="pl.id"
            :cover="pl.cover"
            :name="pl.name"
            :collapsed="setting.sidebarCollapsed"
            @navigate="goToPlaylist"
          />
        </div>
      </div>
    </n-scrollbar>

    <n-virtual-list
      v-else
      class="sidebar-scroll sidebar-virtual collapsed"
      :items="collapsedNavItems"
      :item-size="38"
      key-field="key"
      :show-scrollbar="false"
      @scroll="handleSidebarScroll"
    >
      <template #default="{ item }">
        <div class="sidebar-virtual-row">
          <SidebarItem
            v-if="item.type === 'route'"
            :to="item.to"
            :icon="item.icon"
            :label="item.label"
            :collapsed="true"
            :badge="item.badge"
          />
          <SidebarPlaylistItem
            v-else-if="item.type === 'playlist'"
            :id="item.id"
            :cover="item.cover"
            :name="item.name"
            :collapsed="true"
            @navigate="goToPlaylist"
          />
          <n-skeleton v-else-if="item.type === 'skeleton'" :height="32" :width="32" :round="true" />
          <div v-else class="sidebar-virtual-divider" />
        </div>
      </template>
    </n-virtual-list>

    <!-- Footer: history, settings, avatar -->
    <div :class="['sidebar-footer', { collapsed: setting.sidebarCollapsed }]">
      <SidebarItem
        :to="'/history'"
        :icon="History"
        :label="$t('sidebar.history')"
        :collapsed="setting.sidebarCollapsed"
      />
      <SidebarItem
        :to="'/setting/appearance'"
        :icon="SettingTwo"
        :label="$t('sidebar.settings')"
        :collapsed="setting.sidebarCollapsed"
        :badge="hasUpdate"
      />
      <n-dropdown
        trigger="click"
        placement="top-start"
        :disabled="!user.userLogin"
        :options="userMenuOptions"
        @select="handleUserMenuSelect"
      >
        <n-tooltip placement="right" :disabled="!setting.sidebarCollapsed" :delay="300">
          <template #trigger>
            <div
              :class="['sidebar-user', { collapsed: setting.sidebarCollapsed }]"
              @click="!user.userLogin && router.push('/login')"
            >
              <n-avatar
                round
                :size="setting.sidebarCollapsed ? 24 : 22"
                :src="
                  user.getUserData.avatarUrl
                    ? user.getUserData.avatarUrl.replace(/^http:/, 'https:') + '?param=60y60'
                    : '/images/ico/user-filling.svg'
                "
                fallback-src="/images/ico/user-filling.svg"
              />
              <span
                :class="['sidebar-user-name text-hidden', { hidden: setting.sidebarCollapsed }]"
              >
                {{ user.userLogin ? user.getUserData.nickname : $t("nav.avatar.notLogin") }}
              </span>
            </div>
          </template>
          {{ user.userLogin ? user.getUserData.nickname : $t("nav.avatar.notLogin") }}
        </n-tooltip>
      </n-dropdown>
    </div>
  </Motion>
</template>

<script setup lang="ts">
import {
  HomeTwo,
  FindOne,
  CalendarThirty,
  MusicList,
  RecordDisc,
  Voice,
  CloudStorage,
  History,
  SettingTwo,
  IndentLeft,
  IndentRight,
  Right,
  Logout,
  User,
} from "@icon-park/vue-next";
import { NIcon, NAvatar, NSkeleton, NScrollbar, NTooltip, NDropdown, NVirtualList } from "naive-ui";
import { Motion, AnimatePresence } from "motion-v";
import { settingStore, siteStore, userStore } from "@/store";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import type { Component } from "vue";
import SidebarItem from "./SidebarItem.vue";
import SidebarPlaylistItem from "./SidebarPlaylistItem.vue";
import SearchInp from "@/components/SearchInp/index.vue";
import { useAppUpdater } from "@/composables/useAppUpdater";

const router = useRouter();
const setting = settingStore();
const site = siteStore();
const user = userStore();
const { t } = useI18n();
const { hasUpdate, checkForUpdate } = useAppUpdater();

type CollapsedNavItem =
  | {
      type: "route";
      key: string;
      to: string | { path: string; query?: Record<string, string | number> };
      icon: Component;
      label: string;
      badge?: boolean;
    }
  | {
      type: "playlist";
      key: string;
      id: number;
      cover: string;
      name: string;
    }
  | {
      type: "divider" | "skeleton";
      key: string;
    };

// 用户条下拉菜单（登录后点击头像弹出）
const userMenuOptions = computed(() => [
  {
    key: "profile",
    label: t("nav.avatar.profile"),
    icon: () => h(NIcon, null, { default: () => h(User) }),
  },
  {
    key: "logout",
    label: t("nav.avatar.logout"),
    icon: () => h(NIcon, null, { default: () => h(Logout) }),
  },
]);

const handleUserMenuSelect = (key: string) => {
  if (key === "profile") router.push("/user");
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
      router.push("/");
    },
  });
};

const sidebarWidth = computed(() => (setting.sidebarCollapsed ? "56px" : "208px"));
const sidebarShowTopShadow = ref(false);
const sidebarShowBottomShadow = ref(false);
const sectionOpen = reactive({
  library: true,
  own: true,
  liked: true,
});

const collapsedNavItems = computed<CollapsedNavItem[]>(() => {
  const items: CollapsedNavItem[] = [
    { type: "route", key: "home", to: "/", icon: HomeTwo, label: t("nav.home") },
    { type: "route", key: "discover", to: "/discover", icon: FindOne, label: t("nav.discover") },
  ];

  if (user.userLogin) {
    items.push({
      type: "route",
      key: "dailySongs",
      to: "/dailySongs",
      icon: CalendarThirty,
      label: t("sidebar.dailySongs"),
    });
    items.push({ type: "divider", key: "divider-library" });

    if (sectionOpen.library) {
      items.push(
        {
          type: "route",
          key: "library-playlists",
          to: "/user/playlists",
          icon: MusicList,
          label: t("sidebar.playlists"),
        },
        {
          type: "route",
          key: "library-album",
          to: "/user/album",
          icon: RecordDisc,
          label: t("sidebar.albums"),
        },
        {
          type: "route",
          key: "library-artists",
          to: "/user/artists",
          icon: Voice,
          label: t("sidebar.artists"),
        },
        {
          type: "route",
          key: "library-cloud",
          to: "/user/cloud",
          icon: CloudStorage,
          label: t("sidebar.cloud"),
        },
      );
    }

    items.push({ type: "divider", key: "divider-own" });
    if (sectionOpen.own) {
      if (user.getUserPlayLists.isLoading) {
        items.push(
          { type: "skeleton", key: "own-skeleton-1" },
          { type: "skeleton", key: "own-skeleton-2" },
          { type: "skeleton", key: "own-skeleton-3" },
        );
      } else {
        for (const playlist of user.getUserPlayLists.own) {
          items.push({
            type: "playlist",
            key: `own-${playlist.id}`,
            id: playlist.id,
            cover: playlist.cover,
            name: playlist.name,
          });
        }
      }
    }

    if (user.getUserPlayLists.like.length) {
      items.push({ type: "divider", key: "divider-liked" });
      if (sectionOpen.liked) {
        for (const playlist of user.getUserPlayLists.like) {
          items.push({
            type: "playlist",
            key: `liked-${playlist.id}`,
            id: playlist.id,
            cover: playlist.cover,
            name: playlist.name,
          });
        }
      }
    }
  }

  return items;
});

const toggleSection = (key: "library" | "own" | "liked") => {
  sectionOpen[key] = !sectionOpen[key];
  resetSidebarScrollShadow();
};

const resetSidebarScrollShadow = () => {
  sidebarShowTopShadow.value = false;
  sidebarShowBottomShadow.value = false;
};

const handleSidebarScroll = (event: Event) => {
  if (!(event.target instanceof HTMLElement)) {
    resetSidebarScrollShadow();
    return;
  }

  const { scrollTop, clientHeight, scrollHeight } = event.target;
  const awayFromTop = scrollTop > 2;
  sidebarShowTopShadow.value = awayFromTop;
  sidebarShowBottomShadow.value = awayFromTop && scrollTop + clientHeight < scrollHeight - 2;
};

// Tauri detection
const isTauri = ref(false);
onMounted(() => {
  isTauri.value = typeof window !== "undefined" && "__TAURI__" in window;
  if (isTauri.value) checkForUpdate({ silent: true });
  // Load playlists if logged in
  if (user.userLogin && !user.getUserPlayLists.has) {
    user.setUserPlayLists();
  }
});

watch(
  () => [
    setting.sidebarCollapsed,
    collapsedNavItems.value.length,
    user.getUserPlayLists.own.length,
    user.getUserPlayLists.like.length,
  ],
  () => {
    resetSidebarScrollShadow();
  },
);

const goToPlaylist = (id: number) => {
  router.push({ path: "/playlist", query: { id, page: 1 } });
};
</script>

<style lang="scss" scoped>
.sidebar {
  position: relative;
  z-index: 0;
  flex: 0 0 auto;
  height: 100vh;
  max-height: 100vh;
  min-height: 0;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background-color: var(--app-shell-bg, var(--layout-bg, #fff));
  transition: background-color 0.3s;
  overflow: hidden;
  overscroll-behavior: contain;
  contain: layout paint;

  --sidebar-text: #333;
  --sidebar-text-secondary: #999;
  --sidebar-hover-bg: rgba(0, 0, 0, 0.045);
  --sidebar-active-bg: color-mix(in srgb, var(--main-color) 12%, transparent);
  --sidebar-accent: var(--main-color);
  --sidebar-divider: rgba(0, 0, 0, 0.04);
  --sidebar-item-slot: 40px;
  --sidebar-control-size: 32px;

  &.search-active {
    z-index: var(--z-search-overlay, 1900);
    overflow: visible;
    contain: none;
  }

  &.collapsed {
    width: 56px;
    min-width: 56px;
    max-width: 56px;
    overflow: clip;
    contain: size layout paint;
  }

  &.scroll-top-shadow {
    .sidebar-header::after {
      opacity: 1;
    }
  }

  &.scroll-bottom-shadow {
    .sidebar-footer::before {
      opacity: 1;
    }
  }

  &.dark {
    --sidebar-text: rgba(255, 255, 255, 0.9);
    --sidebar-text-secondary: rgba(255, 255, 255, 0.4);
    --sidebar-hover-bg: rgba(255, 255, 255, 0.06);
    --sidebar-divider: rgba(255, 255, 255, 0.04);
  }

  @media (max-width: 768px) {
    display: none;
  }
}

.sidebar-header {
  position: relative;
  z-index: 2;
  display: grid;
  grid-template-columns: minmax(0, 1fr) var(--sidebar-control-size);
  align-items: center;
  gap: 4px;
  padding: 8px;
  min-height: 46px;
  background-color: var(--app-shell-bg, var(--layout-bg, #fff));
  transition:
    grid-template-columns 0.22s ease,
    padding 0.22s ease;

  &::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    bottom: -18px;
    height: 18px;
    pointer-events: none;
    opacity: 0;
    background: linear-gradient(
      to bottom,
      rgba(var(--app-shell-rgb, 242, 242, 244), 0.72),
      rgba(var(--app-shell-rgb, 242, 242, 244), 0)
    );
    transition: opacity 0.18s ease;
  }

  &.collapsed {
    grid-template-columns: var(--sidebar-item-slot);
    justify-content: center;
    padding: 8px;
  }

  &.collapsed {
    .sidebar-search {
      display: none;
    }
  }
}

.sidebar-search {
  min-width: 0;
  opacity: 1;
  transition: opacity 0.15s ease;

  :deep(.list) {
    z-index: var(--z-search-overlay, 1900);
  }

  :deep(.searchInp) {
    width: 100%;
    --search-surface-bg: rgba(var(--app-shell-rgb, 242, 242, 244), 0.64);
    --search-surface-bg-focus: rgba(var(--app-shell-rgb, 242, 242, 244), 0.78);
  }

  :deep(.input) {
    height: 28px;
    border-radius: var(--radius-md);
    box-shadow: none;
  }

  :deep(.input .n-input__input-el) {
    height: 28px;
    font-size: 12px;
  }

  :deep(.list) {
    top: 34px;
    width: 276px;
  }

  &.hidden {
    opacity: 0;
    pointer-events: none;
  }
}

.sidebar-toggle {
  width: var(--sidebar-control-size);
  height: var(--sidebar-control-size);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  padding: 0;
  border-radius: var(--radius-sm);
  transition:
    background-color 0.2s,
    color 0.2s;
  color: var(--sidebar-text);

  &:hover {
    background-color: var(--sidebar-hover-bg);
  }
}

.sidebar-scroll {
  flex: 1 1 auto;
  min-height: 0;
  max-height: 100%;
  min-width: 0;
  width: 100%;
  overflow: hidden;
  overscroll-behavior: contain;

  :deep(.n-scrollbar-container),
  :deep(.n-scrollbar-content) {
    max-width: 100%;
    min-width: 0;
    box-sizing: border-box;
    overflow-x: hidden !important;
  }

  :deep(.n-scrollbar-rail) {
    opacity: 0;
    transition: opacity 0.3s;
  }

  &:hover {
    :deep(.n-scrollbar-rail) {
      opacity: 1;
    }
  }

  &.collapsed {
    width: 56px;
    min-width: 56px;
    max-width: 56px;
    scrollbar-width: none;

    :deep(.n-scrollbar-container) {
      width: 56px;
      max-width: 56px;
      overflow-x: hidden !important;
      overscroll-behavior: contain;
    }

    :deep(.n-scrollbar-content) {
      width: 56px;
      min-width: 56px;
      max-width: 56px;
      overflow-x: hidden;
    }

    :deep(.n-scrollbar-rail) {
      display: none;
    }
  }
}

.sidebar-virtual {
  flex: 1 1 auto;
  width: 56px;
  min-width: 56px;
  max-width: 56px;
  min-height: 0;
  overflow: hidden;
  overscroll-behavior: contain;
  scrollbar-width: none;
  touch-action: pan-y;

  :deep(.v-vl),
  :deep(.v-vl-items),
  :deep(.n-virtual-list) {
    width: 56px !important;
    min-width: 56px !important;
    max-width: 56px !important;
    overflow-x: hidden !important;
    overscroll-behavior: contain;
  }

  :deep(.v-vl) {
    scrollbar-width: none;
  }

  :deep(.v-vl::-webkit-scrollbar) {
    display: none;
  }
}

.sidebar-virtual-row {
  width: 56px;
  height: 38px;
  min-width: 56px;
  max-width: 56px;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  box-sizing: border-box;
}

.sidebar-virtual-divider {
  width: 14px;
  height: 1px;
  border-radius: var(--radius-pill);
  background-color: var(--sidebar-divider);
}

.sidebar-section {
  padding: 1px 8px;
  max-width: 100%;
  box-sizing: border-box;
  transition: padding 0.3s ease;

  &.collapsed {
    padding: 1px 8px;
  }

  & + .sidebar-section {
    margin-top: 10px;
    padding-top: 2px;
  }
}

.sidebar-primary {
  padding-top: 4px;
}

.sidebar-section-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-height: 24px;
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  color: var(--sidebar-text-secondary);
  padding: 1px 7px 3px 11px;
  letter-spacing: 0;
  white-space: nowrap;
  overflow: hidden;
  cursor: pointer;
  opacity: 1;
  position: relative;
  transition:
    color 0.2s,
    opacity 0.18s ease,
    padding 0.22s ease;

  span {
    opacity: 1;
    transition: opacity 0.18s ease;
  }

  .section-chevron {
    opacity: 0.68;
    transition:
      opacity 0.2s,
      transform 0.2s;

    &.open {
      transform: rotate(90deg);
    }
  }

  &:hover {
    color: var(--sidebar-text);

    .section-chevron {
      opacity: 1;
    }
  }
}

.sidebar-section-content {
  overflow: hidden;
}

.sidebar-section.collapsed {
  .sidebar-section-header {
    padding: 1px 0 3px;
    cursor: default;
    opacity: 1;
    pointer-events: none;

    span,
    .n-icon {
      opacity: 0;
    }

    &::after {
      content: "";
      position: absolute;
      left: 13px;
      right: 13px;
      top: 50%;
      height: 1px;
      border-radius: var(--radius-pill);
      background-color: var(--sidebar-divider);
      transform: translateY(-50%);
    }
  }
}

.sidebar-empty {
  font-size: 12px;
  color: var(--sidebar-text-secondary);
  padding: 8px 12px;
  white-space: nowrap;
  overflow: hidden;
}

.sidebar-footer {
  position: relative;
  z-index: 2;
  flex: 0 0 auto;
  min-width: 0;
  padding: 7px 8px;
  border-top: 1px solid var(--sidebar-divider);
  background-color: var(--app-shell-bg, var(--layout-bg, #fff));
  transition: padding 0.3s ease;

  &::before {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    top: -18px;
    height: 18px;
    pointer-events: none;
    opacity: 0;
    background: linear-gradient(
      to top,
      rgba(var(--app-shell-rgb, 242, 242, 244), 0.78),
      rgba(var(--app-shell-rgb, 242, 242, 244), 0)
    );
    transition: opacity 0.18s ease;
  }

  &.collapsed {
    padding: 8px;
  }
}

.sidebar-user {
  display: grid;
  grid-template-columns: var(--sidebar-item-slot) minmax(0, 1fr);
  align-items: center;
  width: 100%;
  margin-top: 4px;
  padding: 0;
  min-height: 32px;
  border-radius: var(--radius-md);
  background-color: var(--sidebar-hover-bg);
  cursor: pointer;
  transition:
    background-color 0.2s,
    width 0.3s ease;
  overflow: hidden;
  white-space: nowrap;

  &.collapsed {
    width: var(--sidebar-item-slot);
    min-height: 34px;
    margin-inline: auto;
    margin-top: 4px;
  }

  :deep(.n-avatar) {
    justify-self: center;
  }

  &:hover {
    background-color: var(--sidebar-active-bg);
  }
}

.sidebar-user-name {
  min-width: 0;
  padding-right: 10px;
  font-size: 12.5px;
  color: var(--sidebar-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  opacity: 1;
  transform: translateX(0);
  transition:
    opacity 0.2s ease 0.1s,
    transform 0.3s ease;

  &.hidden {
    opacity: 0;
    transform: translateX(-4px);
    transition:
      opacity 0.1s ease,
      transform 0.3s ease;
  }
}

// Text fade transition for section titles
.sidebar-text-fade-enter-active {
  transition: opacity 0.25s ease 0.15s;
}

.sidebar-text-fade-leave-active {
  transition: opacity 0.1s ease;
}

.sidebar-text-fade-enter-from,
.sidebar-text-fade-leave-to {
  opacity: 0;
}
</style>
