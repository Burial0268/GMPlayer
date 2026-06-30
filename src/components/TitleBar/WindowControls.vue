<template>
  <div v-if="showControls" class="window-controls">
    <button
      v-if="minimizable"
      class="win-ctrl-btn"
      type="button"
      aria-label="Minimize window"
      @click="minimizeWindow"
    >
      <svg class="win-ctrl-icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <path d="M3.5 8h9" />
      </svg>
    </button>
    <button
      v-if="maximizable"
      class="win-ctrl-btn"
      type="button"
      :aria-label="isMaximized ? 'Restore window size' : 'Maximize window size'"
      @click="toggleMaximize"
    >
      <svg
        v-if="isMaximized"
        class="win-ctrl-icon restore-icon"
        viewBox="0 0 16 16"
        aria-hidden="true"
        focusable="false"
      >
        <path d="M5.5 4.5h6v6h-6z" />
        <path d="M4.5 6.5h-1v6h6v-1" />
      </svg>
      <svg v-else class="win-ctrl-icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <path d="M4.5 4.5h7v7h-7z" />
      </svg>
    </button>
    <button class="win-ctrl-btn close" type="button" aria-label="Close window" @click="closeWindow">
      <svg
        class="win-ctrl-icon close-icon"
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
import { ref, onMounted, onBeforeUnmount } from "vue";
import { isTauri } from "@/utils/tauri/windowManager";
import { isMobile } from "@/utils/tauri";

/**
 * Reusable native window control cluster (minimize / maximize / close).
 *
 * Runs inside the window it controls, so it listens to the local
 * `tauri://resize` event to keep the maximize/restore glyph in sync.
 *
 * Colour is fully inherited: icons use `currentColor` and hover surfaces
 * mix `currentColor`, so the host only needs to set `color` to retint.
 * Hidden on mobile and macOS (traffic lights handle controls there).
 */
const props = withDefaults(
  defineProps<{
    /** Target window label (defaults to the main window). */
    label?: string;
    /** Whether to render the minimize button. */
    minimizable?: boolean;
    /** Whether to render the maximize/restore button. */
    maximizable?: boolean;
  }>(),
  {
    label: "main",
    minimizable: true,
    maximizable: true,
  },
);

const showControls = ref(false);
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
  await invoke("plugin:window|minimize", { label: props.label });
}

async function toggleMaximize() {
  await invoke("plugin:window|toggle_maximize", { label: props.label });
}

async function closeWindow() {
  await invoke("plugin:window|close", { label: props.label });
}

async function checkMaximized() {
  const result = await invoke<boolean>("plugin:window|is_maximized", { label: props.label });
  if (result !== null) isMaximized.value = result;
}

onMounted(async () => {
  if (!isTauri()) return;
  if (await isMobile()) return;

  // macOS uses the native traffic lights overlay instead of DOM controls.
  const isMac = navigator.platform.toUpperCase().indexOf("MAC") >= 0;
  if (isMac) return;

  showControls.value = true;
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
.window-controls {
  display: flex;
  align-items: stretch;
  height: 100%;
  -webkit-app-region: no-drag;
  app-region: no-drag;
}

.win-ctrl-btn {
  width: 38px;
  height: 100%;
  min-height: 30px;
  padding: 0;
  border: none;
  outline: none;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 0;
  box-shadow: none;
  cursor: default;
  color: inherit;
  background-color: transparent;
  transition:
    background-color 0.12s ease,
    color 0.12s ease;

  &:hover {
    background-color: color-mix(in srgb, currentColor 12%, transparent);
  }

  &:active {
    background-color: color-mix(in srgb, currentColor 18%, transparent);
  }

  &.close:hover {
    background-color: rgba(232, 17, 35, 0.9);
    color: #fff;
  }

  &.close:active {
    background-color: rgba(232, 17, 35, 0.75);
    color: #fff;
  }
}

.win-ctrl-icon {
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

.close-icon {
  stroke-width: 1.65;
}
</style>
