<template>
  <nav
    :class="{
      'tauri-app': isTauri() && !isMobileState,
      'mobile-nav': isCompactViewport,
      dark: setting.getSiteTheme === 'dark',
    }"
    :aria-label="$t('navigation.label')"
  >
    <div class="left">
      <button
        v-if="isCompactViewport && navigation.canGoBack.value"
        type="button"
        class="layer-back"
        :aria-label="$t('general.name.goBack')"
        @click="navigation.closeTop()"
      >
        <n-icon size="26" :component="Left" />
        <span>{{ backLabel }}</span>
      </button>
      <div v-else-if="!isCompactViewport" class="controls">
        <button type="button" :aria-label="$t('general.name.goBack')" @click="router.back()">
          <n-icon size="22" :component="Left" />
        </button>
        <button type="button" :aria-label="$t('navigation.forward')" @click="router.forward()">
          <n-icon size="22" :component="Right" />
        </button>
      </div>
    </div>
    <div class="right">
      <SearchInp v-if="showNavSearch" location="nav" class="nav-search" />
      <button
        type="button"
        class="action-icon"
        :aria-label="$t('setting.theme')"
        @click="toggleTheme"
      >
        <n-icon size="18" :component="setting.getSiteTheme === 'light' ? Moon : SunOne" />
      </button>
    </div>
  </nav>
</template>

<script setup lang="ts">
import { NIcon } from "naive-ui";
import { Left, Right, Moon, SunOne } from "@icon-park/vue-next";
import { settingStore } from "@/store";
import { useRouter } from "vue-router";
import SearchInp from "@/components/SearchInp/index.vue";
import { isTauri, isMobile, isMobileDevice } from "@/utils/tauri";
import { ref, onMounted, onUnmounted, computed } from "vue";
import { useLayerNavigation } from "@/utils/navigation";
import { useI18n } from "vue-i18n";

const router = useRouter();
const navigation = useLayerNavigation();
const { t } = useI18n();
const setting = settingStore();
const backLabel = computed(() => {
  const parent = navigation.state.value.layers.at(-2);
  return parent?.label
    ? t(parent.label)
    : t(`sidebar.tab.${navigation.current.value?.root ?? "home"}`);
});
const isMobileState = ref(isMobileDevice());
const compactViewportQuery = window.matchMedia("(max-width: 768px)");
const isCompactViewport = ref(compactViewportQuery.matches);
const updateCompactViewport = (event: MediaQueryListEvent) => {
  isCompactViewport.value = event.matches;
};
const showNavSearch = computed(() => isMobileState.value || isCompactViewport.value);

onMounted(async () => {
  compactViewportQuery.addEventListener("change", updateCompactViewport);
  isMobileState.value = await isMobile();
});
onUnmounted(() => compactViewportQuery.removeEventListener("change", updateCompactViewport));

const toggleTheme = (event: MouseEvent) => {
  const root = document.documentElement;
  const target = event.currentTarget;
  const rect = target instanceof Element ? target.getBoundingClientRect() : null;
  const x = event.clientX || (rect ? rect.left + rect.width / 2 : window.innerWidth / 2);
  const y = event.clientY || (rect ? rect.top + rect.height / 2 : window.innerHeight / 2);
  root.style.setProperty("--theme-transition-x", `${x}px`);
  root.style.setProperty("--theme-transition-y", `${y}px`);
  root.dataset.themeTransitionOrigin = "custom";

  const nextTheme = setting.getSiteTheme === "light" ? "dark" : "light";
  if (typeof window.$setSiteThemeWithTransition === "function") {
    window.$setSiteThemeWithTransition(nextTheme);
  } else {
    setting.setSiteTheme(nextTheme);
  }
};
</script>

<style lang="scss" scoped>
nav {
  --nav-control-height: 32px;
  --nav-icon-button-size: 28px;

  width: 100%;
  height: 34px;
  min-height: 34px;
  display: flex;
  flex-direction: row;
  justify-content: space-between;
  align-items: center;
  max-width: none;
  margin: 0;
  padding: 0;
  pointer-events: none;

  .left {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 12px;
    flex: 1;
    min-width: 0;

    .controls {
      pointer-events: auto;
      display: flex;
      flex-direction: row;
      align-items: center;
      gap: 2px;
      height: var(--nav-control-height);
      box-sizing: border-box;
      padding: 2px;
      border: 1px solid var(--material-border);
      border-radius: var(--radius-pill);
      background-color: var(--material-regular-bg);
      box-shadow:
        var(--shadow-1),
        inset 0 1px 0 var(--material-highlight);
      -webkit-backdrop-filter: var(--material-filter);
      backdrop-filter: var(--material-filter);

      .n-icon {
        width: var(--nav-icon-button-size);
        height: var(--nav-icon-button-size);
        box-sizing: border-box;
        display: flex;
        align-items: center;
        justify-content: center;
        margin: 0;
        border-radius: var(--radius-pill);
        padding: 3px;
        cursor: pointer;
        transition:
          background-color var(--duration-200) var(--ease-out),
          transform var(--duration-200) var(--ease-out);

        @media (min-width: 640px) {
          &:hover {
            background-color: var(--hover-overlay);
          }
        }

        &:active {
          transform: scale(0.95);
        }
      }
    }
  }

  .right {
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    min-width: 0;
    flex: 0 1 auto;

    .action-icon {
      flex: 0 0 auto;
      width: var(--nav-control-height);
      height: var(--nav-control-height);
      box-sizing: border-box;
      display: flex;
      align-items: center;
      justify-content: center;
      pointer-events: auto;
      cursor: pointer;
      padding: 0;
      border: 1px solid var(--material-border);
      border-radius: var(--radius-pill);
      background-color: var(--material-regular-bg);
      box-shadow:
        var(--shadow-1),
        inset 0 1px 0 var(--material-highlight);
      -webkit-backdrop-filter: var(--material-filter);
      backdrop-filter: var(--material-filter);
      transition:
        background-color var(--duration-200) var(--ease-out),
        transform var(--duration-200) var(--ease-out),
        color var(--duration-200) var(--ease-out);

      &:hover {
        background-color: color-mix(in srgb, var(--material-regular-bg) 94%, var(--text-primary));
      }

      &:active {
        transform: scale(0.95);
      }
    }

    .nav-search {
      pointer-events: auto;
      min-width: 0;
      // Grow to fill whatever the row leaves (theme icon + optional back label take
      // their share first). The floor is sized to hold the full "搜索音乐/视频"
      // placeholder — the old 128px floor left ~10px slack and clipped the last glyph.
      flex: 1 1 clamp(164px, 42vw, 230px);
      width: clamp(164px, 42vw, 230px);

      @media (min-width: 769px) {
        display: none;
      }

      @media (max-width: 450px) {
        flex: 0 0 auto;
        width: auto;
      }
    }
  }

  &.tauri-app {
    --nav-control-height: 30px;
    --nav-icon-button-size: 24px;

    height: 30px;
    min-height: 30px;
  }

  @media (max-width: 768px) {
    height: var(--app-mobile-nav-height);
    min-height: var(--app-mobile-nav-height);
    padding-top: var(--app-safe-area-top, 0px);
    box-sizing: border-box;

    // Keep the mobile navigation controls' established appearance.
    --floating-control-bg: rgba(255, 255, 255, 0.62);
    --acrylic-border: rgba(0, 0, 0, 0.07);
    --nav-pill-specular: rgba(255, 255, 255, 0.7);
    --nav-pill-grade: saturate(180%) brightness(106%) contrast(96%);

    &.dark {
      --floating-control-bg: rgba(32, 32, 38, 0.6);
      --acrylic-border: rgba(255, 255, 255, 0.11);
      --nav-pill-specular: rgba(255, 255, 255, 0.16);
      --nav-pill-grade: saturate(170%) brightness(82%) contrast(88%);

      .right .action-icon:hover {
        background-color: rgba(255, 255, 255, 0.12);
      }
    }

    .left .controls,
    .right .action-icon {
      border-color: var(--acrylic-border);
      background-color: var(--floating-control-bg);
      box-shadow:
        0 1px 1px rgb(0 0 0 / 5%),
        0 3px 8px rgb(0 0 0 / 7%),
        inset 0 1px 0 var(--nav-pill-specular);
      -webkit-backdrop-filter: blur(12px) var(--nav-pill-grade);
      backdrop-filter: blur(12px) var(--nav-pill-grade);
    }

    .right .action-icon:hover {
      background-color: var(--hover-overlay);
    }

    .left {
      flex: 0 0 auto;
    }

    .right {
      flex: 1 1 auto;
    }
  }
  button {
    font: inherit;
    color: inherit;
    border: 0;
    padding: 0;
    background: transparent;
    cursor: pointer;

    &:focus-visible {
      outline: 2px solid var(--main-color);
      outline-offset: 2px;
    }
  }

  .layer-back {
    pointer-events: auto;
    display: flex;
    align-items: center;
    justify-content: flex-start;
    min-width: 44px;
    min-height: 44px;
    max-width: 100%;
    padding-right: 12px;
    color: var(--main-color);
    font-size: 17px;
    font-weight: 500;
    letter-spacing: -0.025em;
    line-height: 1;
    -webkit-tap-highlight-color: transparent;
    transition: opacity 150ms ease;

    .n-icon {
      flex: 0 0 26px;
    }
    > span {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    &:active {
      opacity: 0.45;
    }
  }

  @media (max-width: 768px) {
    .left {
      min-width: 0;
      flex: 1 1 0;
    }
    .right {
      flex: 0 0 auto;
    }
  }
}
</style>
