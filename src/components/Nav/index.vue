<template>
  <nav :class="{ 'tauri-app': isTauri() && !isMobileState, dark: setting.getSiteTheme === 'dark' }">
    <div class="left">
      <div class="controls">
        <n-icon size="22" :component="Left" @click="router.go(-1)" />
        <n-icon size="22" :component="Right" @click="router.go(1)" />
      </div>
    </div>
    <div class="right">
      <SearchInp v-if="showNavSearch" class="nav-search" />
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
import { useRouter } from "vue-router";
import SearchInp from "@/components/SearchInp/index.vue";
import { isTauri, isMobile, isMobileDevice } from "@/utils/tauri";
import { ref, onMounted, onUnmounted, computed } from "vue";

const router = useRouter();
const setting = settingStore();
const isMobileState = ref(isMobileDevice());
const isCompactViewport = ref(false);
let compactViewportQuery = null;

onMounted(async () => {
  isMobileState.value = await isMobile();
  compactViewportQuery = window.matchMedia("(max-width: 768px)");
  isCompactViewport.value = compactViewportQuery.matches;
  compactViewportQuery.addEventListener("change", updateCompactViewport);
});

onUnmounted(() => {
  compactViewportQuery?.removeEventListener("change", updateCompactViewport);
});

const updateCompactViewport = (event) => {
  isCompactViewport.value = event.matches;
};

const showNavSearch = computed(() => isMobileState.value || isCompactViewport.value);

// Tauri detection
const toggleTheme = () => {
  if (setting.getSiteTheme === "light") {
    setting.setSiteTheme("dark");
  } else {
    setting.setSiteTheme("light");
  }
};
</script>

<style lang="scss" scoped>
nav {
  --nav-control-height: 32px;
  --nav-icon-button-size: 26px;

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
      border: 1px solid var(--acrylic-border, rgba(0, 0, 0, 0.06));
      border-radius: var(--radius-pill);
      background-color: var(--floating-control-bg, rgba(255, 255, 255, 0.48));
      box-shadow:
        0 8px 22px rgb(0 0 0 / 10%),
        inset 0 1px 0 rgb(255 255 255 / 24%);
      -webkit-backdrop-filter: blur(18px) saturate(160%);
      backdrop-filter: blur(18px) saturate(160%);

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
      border: 1px solid var(--acrylic-border, rgba(0, 0, 0, 0.06));
      border-radius: var(--radius-pill);
      background-color: var(--floating-control-bg, rgba(255, 255, 255, 0.48));
      box-shadow:
        0 8px 22px rgb(0 0 0 / 8%),
        inset 0 1px 0 rgb(255 255 255 / 20%);
      -webkit-backdrop-filter: blur(18px) saturate(160%);
      backdrop-filter: blur(18px) saturate(160%);
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

    .nav-search {
      pointer-events: auto;
      min-width: 0;
      flex: 0 1 clamp(128px, 36vw, 220px);
      width: clamp(128px, 36vw, 220px);

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
    --floating-control-bg: rgba(255, 255, 255, 0.42);
  }

  &.dark {
    --floating-control-bg: rgba(24, 24, 24, 0.5);

    .controls .n-icon:hover,
    .right .action-icon:hover {
      background-color: rgba(255, 255, 255, 0.12);
    }
  }

  @media (max-width: 768px) {
    height: calc(42px + var(--app-safe-area-top, 0px));
    min-height: calc(42px + var(--app-safe-area-top, 0px));
    padding-top: var(--app-safe-area-top, 0px);
    box-sizing: border-box;

    .left {
      flex: 0 0 auto;
    }

    .right {
      flex: 1 1 auto;
    }
  }
}
</style>
