<template>
  <div
    v-if="showTitleBar"
    class="titlebar"
    :class="{ 'bigplayer-mode': music.showBigPlayer, 'dark-mode': isDark }"
  >
    <button class="decorum-tb-btn" aria-label="Minimize window" @click="minimizeWindow">
      &#xE921;
    </button>
    <button
      class="decorum-tb-btn"
      :aria-label="isMaximized ? 'Restore window size' : 'Maximize window size'"
      @click="toggleMaximize"
    >
      {{ isMaximized ? "\uE923" : "\uE922" }}
    </button>
    <button class="decorum-tb-btn close" aria-label="Close window" @click="closeWindow">
      &#xE8BB;
    </button>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount } from "vue";
import { musicStore, settingStore } from "@/store";
import { isTauri } from "@/utils/tauri/windowManager";
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
  top: 0;
  right: 0;
  z-index: 9999;
  display: flex;
  height: 32px;
  user-select: none;
  -webkit-app-region: no-drag;
  transition: opacity 0.25s ease;

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
  width: 58px;
  height: 32px;
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
  transition: background 0.1s;
  background-color: transparent;
  text-rendering: optimizeLegibility;
  font-family: "Segoe Fluent Icons", "Segoe MDL2 Assets";
  color: var(--n-text-color, #333);

  &:hover {
    background-color: rgba(0, 0, 0, 0.06);
  }

  &.close:hover {
    background-color: rgba(255, 0, 0, 0.7);
    color: #fff;
  }

  // Dark mode: light overlays on dark background
  .dark-mode & {
    color: rgba(255, 255, 255, 0.85);

    &:hover {
      background-color: rgba(255, 255, 255, 0.08);
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
</style>
