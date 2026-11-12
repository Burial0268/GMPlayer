<template>
  <div class="desktop-toggle-controls">
    <OutputDeviceButton v-if="nativeOutput" placement="top-end" />
    <button
      v-if="hasLyrics"
      class="toggle-icon-button"
      type="button"
      :aria-pressed="lyricsVisible"
      aria-label="Toggle lyrics"
      @click="$emit('toggleLyrics')"
    >
      <component :is="lyricsVisible ? IconLyricsOn : IconLyricsOff" />
    </button>
    <button
      class="toggle-icon-button"
      type="button"
      :aria-pressed="queueOpen"
      aria-label="Toggle playlist"
      @click="$emit('toggleQueue')"
    >
      <component :is="queueOpen ? IconPlaylistOn : IconPlaylistOff" />
    </button>
  </div>
</template>

<script setup lang="ts">
import IconLyricsOff from "../icons/IconLyricsOff.vue";
import IconLyricsOn from "../icons/IconLyricsOn.vue";
import IconPlaylistOff from "../icons/IconPlaylistOff.vue";
import IconPlaylistOn from "../icons/IconPlaylistOn.vue";
import OutputDeviceButton from "./OutputDeviceButton.vue";
import { isTauri } from "@/utils/tauri/core/runtime";

// The output menu drives the native backend; the web player has no device list to offer.
const nativeOutput = isTauri();

defineProps<{
  lyricsVisible: boolean;
  queueOpen: boolean;
  hasLyrics: boolean;
}>();

defineEmits<{
  toggleLyrics: [];
  toggleQueue: [];
}>();
</script>

<style lang="scss" scoped>
.desktop-toggle-controls {
  position: absolute;
  right: clamp(24px, 4vw, 64px);
  bottom: clamp(22px, 4vh, 44px);
  z-index: 8;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  color: var(--main-cover-mix-color, rgb(239, 239, 239));
  mix-blend-mode: plus-lighter;

  :deep(.n-icon),
  :deep(svg),
  :deep(path) {
    mix-blend-mode: plus-lighter;
  }
}

// The output button renders inside n-popselect, where this component's scoped attribute does not
// reach; `:deep` gives it the same look as the toggles beside it.
.toggle-icon-button,
:deep(.output-device-button) {
  appearance: none;
  border: none;
  background-color: transparent;
  color: currentColor;
  aspect-ratio: 1 / 1;
  display: flex;
  mix-blend-mode: plus-lighter;
  justify-content: center;
  align-items: center;
  opacity: 0.45;
  width: 3rem;
  height: 3rem;
  padding: 0;
  border-radius: var(--radius-md);
  cursor: pointer;
  transition:
    opacity 0.24s ease,
    transform 0.26s cubic-bezier(0.34, 1.56, 0.64, 1);
  will-change: opacity, transform;

  &[aria-pressed="true"] {
    opacity: 0.92;
  }

  @media (hover: hover) {
    &:hover {
      opacity: 0.92;
    }
  }

  @media (hover: hover) {
    &:hover {
      transform: scale(1.05);
    }
  }

  &:active {
    transform: scale(0.96);
  }

  @media screen and (max-width: 1600px), (max-height: 1000px) {
    width: 2.8rem;
    height: 2.8rem;
  }
}

// Flat, not nested in the list above: a `:deep()` nested inside `:deep()` compiles to an invalid
// selector, which would drop the whole rule for the toggles too.
.toggle-icon-button :deep(svg),
:deep(.output-device-button svg) {
  width: 2.45rem;
  height: 2.45rem;
  aspect-ratio: 1 / 1;
  display: block;

  @media screen and (max-width: 1600px), (max-height: 1000px) {
    width: 2.25rem;
    height: 2.25rem;
  }
}
</style>
