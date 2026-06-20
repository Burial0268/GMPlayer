<template>
  <div
    v-if="showTitleBar"
    class="titlebar"
    :class="{ 'bigplayer-mode': music.showBigPlayer, 'dark-mode': isDark }"
  >
    <button
      class="decorum-tb-btn"
      type="button"
      aria-label="Minimize window"
      @click="minimizeWindow"
    >
      <svg class="titlebar-icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <path d="M3.5 8h9" />
      </svg>
    </button>
    <button
      class="decorum-tb-btn"
      type="button"
      :aria-label="isMaximized ? 'Restore window size' : 'Maximize window size'"
      @click="toggleMaximize"
    >
      <svg
        v-if="isMaximized"
        class="titlebar-icon restore-icon"
        viewBox="0 0 16 16"
        aria-hidden="true"
        focusable="false"
      >
        <path d="M5.5 4.5h6v6h-6z" />
        <path d="M4.5 6.5h-1v6h6v-1" />
      </svg>
      <svg v-else class="titlebar-icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <path d="M4.5 4.5h7v7h-7z" />
      </svg>
    </button>
    <button
      class="decorum-tb-btn close"
      type="button"
      aria-label="Close window"
      @click="closeWindow"
    >
      <svg
        class="titlebar-icon close-icon"
        viewBox="0 0 16 16"
        aria-hidden="true"
        focusable="false"
      >
        <path d="M4.5 4.5l7 7" />
        <path d="M11.5 4.5l-7 7" />
      </svg>
    </button>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount } from "vue";
import { musicStore, settingStore } from "@/store";
import { isTauri } from "@/utils/tauri/windowManager";
import { isMobile } from "@/utils/tauri";
import { useOsTheme } from "naive-ui";

const music = musicStore();
const setting = settingStore();
const osThemeRef = useOsTheme();

const isDark = computed(() => {
  return setting.themeAuto ? osThemeRef.value === "dark" : setting.theme === "dark";
});
const showTitleBar = ref(false);
const isMaximized = ref(false);

let unlistenResize: (() => void) | null = null;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!isTauri()) return null;
  return window.__TAURI__!.core.invoke<T>(cmd, args);
}

async function listen(event: string, handler: (payload: unknown) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  return window.__TAURI__!.event.listen(event, (e) => handler(e.payload));
}

async function minimizeWindow() {
  await invoke("plugin:window|minimize", { label: "main" });
}

async function toggleMaximize() {
  await invoke("plugin:window|toggle_maximize", { label: "main" });
}

async function closeWindow() {
  await invoke("plugin:window|close", { label: "main" });
}

async function checkMaximized() {
  const result = await invoke<boolean>("plugin:window|is_maximized", { label: "main" });
  if (result !== null) {
    isMaximized.value = result;
  }
}

onMounted(async () => {
  if (!isTauri()) return;
  if (await isMobile()) return;

  // Don't show custom titlebar on macOS (traffic lights handle it)
  const isMac = navigator.platform.toUpperCase().indexOf("MAC") >= 0;
  if (isMac) return;

  showTitleBar.value = true;
  await checkMaximized();

  unlistenResize = await listen("tauri://resize", () => {
    checkMaximized();
  });
});

onBeforeUnmount(() => {
  unlistenResize?.();
});
</script>

<style lang="scss" scoped>
.titlebar {
  position: fixed;
  top: var(--app-floating-control-top);
  right: var(--app-floating-control-inset, 14px);
  z-index: 9999;
  display: flex;
  height: 30px;
  align-items: center;
  overflow: hidden;
  border: 1px solid var(--acrylic-border, rgba(0, 0, 0, 0.06));
  border-radius: var(--radius-lg);
  background-color: var(--titlebar-bg, rgba(255, 255, 255, 0.52));
  box-shadow:
    0 8px 20px rgb(0 0 0 / 8%),
    inset 0 1px 0 rgb(255 255 255 / 26%);
  -webkit-backdrop-filter: blur(18px) saturate(160%);
  backdrop-filter: blur(18px) saturate(160%);
  user-select: none;
  -webkit-app-region: no-drag;
  transition:
    opacity 0.25s ease,
    background-color 0.2s ease,
    border-color 0.2s ease;

  &.dark-mode {
    --titlebar-bg: rgba(24, 24, 24, 0.56);
  }

  // When BigPlayer is open: hidden by default, visible on hover
  &.bigplayer-mode {
    opacity: 0;

    &:hover {
      opacity: 1;
    }
  }
}

// Match decorum's button styling exactly
.decorum-tb-btn {
  width: 38px;
  height: 100%;
  border: none;
  padding: 0;
  outline: none;
  display: flex;
  font-size: 10px;
  font-weight: 300;
  cursor: default;
  box-shadow: none;
  border-radius: 0;
  align-items: center;
  justify-content: center;
  transition:
    background-color 0.12s ease,
    color 0.12s ease;
  background-color: transparent;
  color: var(--n-text-color, #333);

  &:hover {
    background-color: rgba(0, 0, 0, 0.08);
  }

  &.close:hover {
    background-color: rgba(255, 0, 0, 0.7);
    color: #fff;
  }

  // Dark mode: light overlays on dark background
  .dark-mode & {
    color: rgba(255, 255, 255, 0.85);

    &:hover {
      background-color: rgba(255, 255, 255, 0.12);
    }

    &.close:hover {
      background-color: rgba(255, 0, 0, 0.7);
      color: #fff;
    }
  }

  // BigPlayer mode: white text on dark background
  .bigplayer-mode & {
    color: rgba(255, 255, 255, 0.85);

    &:hover {
      background-color: rgba(255, 255, 255, 0.1);
    }

    &.close:hover {
      background-color: rgba(255, 0, 0, 0.7);
      color: #fff;
    }
  }
}

.titlebar-icon {
  width: 20px;
  height: 20px;
  display: block;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.55;
  stroke-linecap: round;
  stroke-linejoin: round;
  pointer-events: none;
}

.restore-icon {
  width: 20px;
  height: 20px;
}

.close-icon {
  stroke-width: 1.65;
}
</style>
