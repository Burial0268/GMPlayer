<template>
  <!-- 悬浮变体 teleport 到 body：窗口控制的 z-index:9999 必须与同样挂在 body 上的
       BigPlayer(z-index:2000) 在同一个 stacking context 里比较，才能真正压在其上。
       留在 #app 内时，只要祖先(如带 contain/isolation 的容器)形成 stacking context，
       9999 就会被封顶，BigPlayer 反而盖住窗口控制，导致大播放器打开时无法关闭窗口。
       window 变体(从设置从窗等处内联使用)保持原地渲染。 -->
  <Teleport to="body" :disabled="variant !== 'floating'">
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
  </Teleport>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, type Component } from "vue";
import { musicStore, settingStore } from "@/store";
import { isTauri } from "@/utils/tauri/core/runtime";
import { getDesktopEnvironment, isMobile, type DesktopEnvironment } from "@/utils/tauri";
import type { WindowLabel } from "@/utils/tauri/window/types";
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

  // 悬浮窗口控制按钮占用右上角，通知 body 级浮层（右侧抽屉、通知）让位。
  // 抽屉/通知 teleport 到 body，取不到 .app-body 上的环境类，因此挂在 <html> 上。
  if (props.variant === "floating") {
    document.documentElement.classList.add("has-floating-titlebar");
  }
});

onBeforeUnmount(() => {
  if (showTitleBar.value && props.variant === "floating") {
    document.documentElement.classList.remove("has-floating-titlebar");
  }
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
    opacity var(--duration-200) var(--ease-out),
    background-color var(--duration-200) var(--ease-out),
    border-color var(--duration-200) var(--ease-out);

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
  height: var(--app-titlebar-height, 30px);
  border: 1px solid var(--acrylic-border, rgba(0, 0, 0, 0.06));
  border-radius: var(--radius-lg);
  background-color: var(--titlebar-bg, rgba(255, 255, 255, 0.52));
  box-shadow:
    0 8px 20px rgb(0 0 0 / 8%),
    inset 0 1px 0 rgb(255 255 255 / 26%);
  -webkit-backdrop-filter: blur(18px) saturate(160%);
  backdrop-filter: blur(18px) saturate(160%);

  // BigPlayer mode: white controls over the immersive cover. Keep them faintly
  // visible (not opacity:0) and rise to full on hover — on desktop these are the
  // ONLY window min/close controls, so a fully-invisible hover-only target is
  // undiscoverable and leaves the user unable to close the app while the player
  // is open. The frosted pill + backdrop-blur keeps them legible over the cover.
  &.bigplayer-mode {
    opacity: 0.6;
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
