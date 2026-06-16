<template>
  <div class="desktop-toggle-controls">
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
  color: var(--main-cover-color);
  mix-blend-mode: plus-lighter;
}

.toggle-icon-button {
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
  border-radius: 8px;
  cursor: pointer;
  transition:
    opacity 0.24s ease,
    transform 0.26s cubic-bezier(0.34, 1.56, 0.64, 1);
  will-change: opacity, transform;

  :deep(svg) {
    width: 2.45rem;
    height: 2.45rem;
    aspect-ratio: 1 / 1;
    display: block;
  }

  &:hover,
  &[aria-pressed="true"] {
    opacity: 0.92;
  }

  &:hover {
    transform: scale(1.05);
  }

  &:active {
    transform: scale(0.96);
  }

  @media screen and (max-width: 1600px), (max-height: 1000px) {
    width: 2.8rem;
    height: 2.8rem;

    :deep(svg) {
      width: 2.25rem;
      height: 2.25rem;
    }
  }
}
</style>
