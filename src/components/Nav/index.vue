<template>
  <nav
    :class="{ 'tauri-app': isTauri() && !isMobileState }"
    :data-tauri-drag-region="isTauri() || undefined"
  >
    <div class="left" :data-tauri-drag-region="(isTauri() && !isMobileState) || undefined">
      <div class="controls">
        <n-icon size="22" :component="Left" @click="router.go(-1)" />
        <n-icon size="22" :component="Right" @click="router.go(1)" />
      </div>
      <span v-if="routeTitle" class="route-title">{{ routeTitle }}</span>
    </div>
    <div class="right" :data-tauri-drag-region="(isTauri() && !isMobileState) || undefined">
      <SearchInp />
      <!-- Theme toggle -->
      <n-icon
        class="action-icon"
        size="18"
        :component="setting.getSiteTheme === 'light' ? Moon : SunOne"
        @click="toggleTheme"
      />
    </div>
  </nav>
</template>

<script setup>
import { NIcon } from "naive-ui";
import { Left, Right, Moon, SunOne } from "@icon-park/vue-next";
import { settingStore } from "@/store";
import { useRouter, useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import AboutSite from "@/components/DataModal/AboutSite.vue";
import SearchInp from "@/components/SearchInp/index.vue";
import { isTauri, isMobile } from "@/utils/tauri";
import { ref, onMounted, computed } from "vue";

const router = useRouter();
const route = useRoute();
const setting = settingStore();
const { t } = useI18n();
const aboutSiteRef = ref(null);
const isMobileState = ref(false);

onMounted(async () => {
  isMobileState.value = await isMobile();
});

// Route name → i18n key mapping
const routeTitleMap = {
  home: "nav.home",
  discover: "nav.discover",
  "dsc-playlists": "nav.discover",
  "dsc-toplists": "nav.discover",
  "dsc-artists": "nav.discover",
  search: "nav.search.placeholder",
  "s-songs": "nav.search.placeholder",
  "s-artists": "nav.search.placeholder",
  "s-albums": "nav.search.placeholder",
  "s-videos": "nav.search.placeholder",
  "s-playlists": "nav.search.placeholder",
  "mobile-search": "sidebar.tab.search",
  setting: "nav.avatar.setting",
  "setting-main": "nav.avatar.setting",
  "setting-player": "nav.avatar.setting",
  "setting-other": "nav.avatar.setting",
  history: "nav.avatar.history",
  login: "nav.avatar.login",
  playlist: "general.name.playlist",
  album: "general.name.album",
  artist: "general.name.artists",
  "ar-songs": "general.name.artists",
  "ar-albums": "general.name.artists",
  "ar-videos": "general.name.artists",
  song: "general.name.song",
  dailySongs: "sidebar.dailySongs",
  user: "nav.user",
  "user-playlists": "nav.user",
  "user-like": "nav.user",
  "user-album": "nav.user",
  "user-artists": "nav.user",
  "user-cloud": "nav.user",
};

const routeTitle = computed(() => {
  const name = route.name;
  const key = routeTitleMap[name];
  return key ? t(key) : "";
});

// Tauri detection
const toggleTheme = () => {
  if (setting.getSiteTheme === "light") {
    setting.setSiteTheme("dark");
  } else {
    setting.setSiteTheme("light");
  }
  setting.themeAuto = false;
};
</script>

<style lang="scss" scoped>
nav {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: row;
  justify-content: space-between;
  align-items: center;
  max-width: 1400px;
  margin: 0 auto;
  // Push nav content below the status-bar / notch on Tauri mobile.
  // --app-safe-area-top is 0px on all non-tauri-mobile platforms so
  // this is a no-op in every other environment.
  padding-top: var(--app-safe-area-top, 0px);

  // Tauri window controls reserved space
  &.tauri-app {
    padding-right: 138px;
  }

  .left {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 12px;
    flex: 1;
    min-width: 0;

    .controls {
      display: flex;
      flex-direction: row;
      align-items: center;

      .n-icon {
        margin: 0 4px;
        border-radius: 8px;
        padding: 4px;
        cursor: pointer;
        transition:
          background-color 0.2s,
          transform 0.2s;

        @media (min-width: 640px) {
          &:hover {
            background-color: rgba(0, 0, 0, 0.05);
          }
        }

        &:active {
          transform: scale(0.95);
        }
      }
    }

    .route-title {
      font-size: 13px;
      font-weight: 500;
      color: var(--n-text-color-3, #999);
      max-width: 200px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
  }

  .right {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 8px;

    .action-icon {
      cursor: pointer;
      padding: 6px;
      border-radius: 8px;
      transition:
        background-color 0.2s,
        transform 0.2s,
        color 0.2s;

      &:hover {
        background-color: rgba(0, 0, 0, 0.05);
      }

      &:active {
        transform: scale(0.95);
      }
    }
  }
}
</style>
