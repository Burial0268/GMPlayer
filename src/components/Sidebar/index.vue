<template>
  <Motion
    tag="aside"
    :class="[
      'sidebar',
      {
        dark: setting.getSiteTheme === 'dark',
        collapsed: setting.sidebarCollapsed,
        'search-active': site.searchInputActive,
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
    <n-scrollbar :class="['sidebar-scroll', { collapsed: setting.sidebarCollapsed }]">
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

    <!-- Footer: history, settings, avatar -->
    <div :class="['sidebar-footer', { collapsed: setting.sidebarCollapsed }]">
      <SidebarItem
        :to="'/history'"
        :icon="History"
        :label="$t('sidebar.history')"
        :collapsed="setting.sidebarCollapsed"
      />
      <SidebarItem
        :to="'/setting'"
        :icon="SettingTwo"
        :label="$t('sidebar.settings')"
        :collapsed="setting.sidebarCollapsed"
        :badge="hasUpdate"
      />
      <n-tooltip placement="right" :disabled="!setting.sidebarCollapsed" :delay="300">
        <template #trigger>
          <div
            :class="['sidebar-user', { collapsed: setting.sidebarCollapsed }]"
            @click="user.userLogin ? router.push('/user') : router.push('/login')"
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
            <span :class="['sidebar-user-name text-hidden', { hidden: setting.sidebarCollapsed }]">
              {{ user.userLogin ? user.getUserData.nickname : $t("nav.avatar.notLogin") }}
            </span>
          </div>
        </template>
        {{ user.userLogin ? user.getUserData.nickname : $t("nav.avatar.notLogin") }}
      </n-tooltip>
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
} from "@icon-park/vue-next";
import { NIcon, NAvatar, NSkeleton, NScrollbar, NTooltip } from "naive-ui";
import { Motion, AnimatePresence } from "motion-v";
import { settingStore, siteStore, userStore } from "@/store";
import { useRouter } from "vue-router";
import SidebarItem from "./SidebarItem.vue";
import SidebarPlaylistItem from "./SidebarPlaylistItem.vue";
import SearchInp from "@/components/SearchInp/index.vue";
import { useAppUpdater } from "@/composables/useAppUpdater";

const router = useRouter();
const setting = settingStore();
const site = siteStore();
const user = userStore();
const { hasUpdate, checkForUpdate } = useAppUpdater();

const sidebarWidth = computed(() => (setting.sidebarCollapsed ? "56px" : "208px"));
const sectionOpen = reactive({
  library: true,
  own: true,
  liked: true,
});

const toggleSection = (key: "library" | "own" | "liked") => {
  sectionOpen[key] = !sectionOpen[key];
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

const goToPlaylist = (id) => {
  router.push({ path: "/playlist", query: { id, page: 1 } });
};
</script>

<style lang="scss" scoped>
.sidebar {
  position: relative;
  z-index: 0;
  height: 100vh;
  display: flex;
  flex-direction: column;
  background-color: var(--app-shell-bg, var(--layout-bg, #fff));
  transition: background-color 0.3s;
  overflow: visible;

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
  flex: 1;
  overflow: hidden;

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
    :deep(.n-scrollbar-rail) {
      display: none;
    }
  }
}

.sidebar-section {
  padding: 1px 8px;
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
  padding: 7px 8px;
  border-top: 1px solid var(--sidebar-divider);
  transition: padding 0.3s ease;

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
