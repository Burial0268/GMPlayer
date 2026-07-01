<template>
  <div
    v-if="showTitleBar"
    class="titlebar"
    :class="titlebarClasses"
    :data-tauri-drag-region="draggable ? '' : undefined"
  >
    <div v-if="title" class="titlebar-title" data-tauri-drag-region>
      <n-icon v-if="icon" class="titlebar-icon" :component="icon" />
      <span class="titlebar-text" data-tauri-drag-region>{{ title }}</span>
    </div>
    <WindowControls :label="label" :minimizable="minimizable" :maximizable="maximizable" />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, type Component } from "vue";
import { musicStore, settingStore } from "@/store";
import { isTauri } from "@/utils/tauri/windowManager";
import { getDesktopEnvironment, isMobile, type DesktopEnvironment } from "@/utils/tauri";
import type { WindowLabel } from "@/utils/tauri/types";
import { useOsTheme } from "naive-ui";
import WindowControls from "./WindowControls.vue";

const props = withDefaults(
  defineProps<{
    label?: WindowLabel;
    title?: string;
    icon?: Component;
    variant?: "floating" | "window";
    draggable?: boolean;
    minimizable?: boolean;
    maximizable?: boolean;
    showOnMac?: boolean;
  }>(),
  {
    label: "main",
    variant: "floating",
    draggable: false,
    minimizable: true,
    maximizable: true,
    showOnMac: false,
  },
);

const music = musicStore();
const setting = settingStore();
const osThemeRef = useOsTheme();
const desktopEnvironment = ref<DesktopEnvironment | null>(null);

const isDark = computed(() => {
  return setting.themeAuto ? osThemeRef.value === "dark" : setting.theme === "dark";
});
const usesNativeTrafficLights = computed(
  () => desktopEnvironment.value?.usesNativeTrafficLights ?? false,
);
const titlebarClasses = computed(() => [
  `titlebar--${props.variant}`,
  {
    "bigplayer-mode": props.variant === "floating" && music.showBigPlayer,
    "dark-mode": isDark.value,
    "has-title": Boolean(props.title),
    "is-mac": usesNativeTrafficLights.value,
    "is-linux": desktopEnvironment.value?.isLinux ?? false,
    "is-hyprland": desktopEnvironment.value?.isHyprland ?? false,
  },
]);
const showTitleBar = ref(false);

onMounted(async () => {
  if (!isTauri()) return;
  if (await isMobile()) return;

  desktopEnvironment.value = await getDesktopEnvironment();

  // Don't show the floating DOM titlebar on macOS; native traffic lights handle it.
  if (usesNativeTrafficLights.value && !props.showOnMac) return;

  showTitleBar.value = true;
});
</script>

<style lang="scss" scoped>
.titlebar {
  display: flex;
  align-items: center;
  overflow: hidden;
  color: var(--n-text-color, #333);
  user-select: none;
  -webkit-app-region: no-drag;
  app-region: no-drag;
  transition:
    opacity 0.25s ease,
    background-color 0.2s ease,
    border-color 0.2s ease;

  &.dark-mode {
    --titlebar-bg: rgba(24, 24, 24, 0.56);
    color: rgba(255, 255, 255, 0.85);
  }
}

.titlebar--floating {
  position: fixed;
  top: var(--app-floating-control-top);
  right: var(--app-floating-control-inset, 14px);
  z-index: 9999;
  height: 30px;
  border: 1px solid var(--acrylic-border, rgba(0, 0, 0, 0.06));
  border-radius: var(--radius-lg);
  background-color: var(--titlebar-bg, rgba(255, 255, 255, 0.52));
  box-shadow:
    0 8px 20px rgb(0 0 0 / 8%),
    inset 0 1px 0 rgb(255 255 255 / 26%);
  -webkit-backdrop-filter: blur(18px) saturate(160%);
  backdrop-filter: blur(18px) saturate(160%);

  // BigPlayer mode: white controls on dark cover, hidden until hover
  &.bigplayer-mode {
    opacity: 0;
    color: rgba(255, 255, 255, 0.85);

    &:hover {
      opacity: 1;
    }
  }
}

.titlebar--window {
  position: relative;
  z-index: 2;
  flex: 0 0 auto;
  justify-content: space-between;
  width: 100%;
  height: 44px;
  padding: 0 6px 0 16px;
  box-sizing: border-box;
  border-bottom: 1px solid color-mix(in srgb, var(--n-border-color) 50%, transparent);
  background-color: color-mix(in srgb, var(--n-text-color) 2.5%, transparent);
  -webkit-app-region: drag;
  app-region: drag;

  &.is-mac {
    padding-left: 78px;
  }
}

.titlebar-title {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  pointer-events: none;
}

.titlebar-icon {
  flex: 0 0 auto;
  font-size: 18px;
  color: var(--main-color);
}

.titlebar-text {
  min-width: 0;
  overflow: hidden;
  font-size: 13.5px;
  font-weight: 600;
  letter-spacing: 0.2px;
  opacity: 0.92;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
