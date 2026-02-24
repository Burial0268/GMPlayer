<template>
  <div
    class="lyric-offset-control"
    :class="{
      visible: visible,
      'has-offset': offset !== 0,
    }"
  >
    <!-- [+] button -->
    <div
      class="offset-btn"
      @click.stop="adjustOffset(500)"
      :title="t('setting.lyricOffsetAdvance')"
    >
      <n-icon size="18" :component="AddRound" />
    </div>

    <!-- Offset value badge (click to reset) -->
    <div
      class="offset-value"
      @click.stop="resetOffset"
      :title="t('setting.lyricOffsetReset')"
    >
      {{ offsetDisplay }}
    </div>

    <!-- [-] button -->
    <div
      class="offset-btn"
      @click.stop="adjustOffset(-500)"
      :title="t('setting.lyricOffsetDelay')"
    >
      <n-icon size="18" :component="RemoveRound" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { AddRound, RemoveRound } from "@vicons/material";
import { settingStore } from "@/store";
import { useI18n } from "vue-i18n";

const { t } = useI18n();

defineProps<{
  visible?: boolean;
}>();

const setting = settingStore();

const offset = computed(() => setting.lyricTimeOffset ?? 0);

const offsetDisplay = computed(() => {
  const seconds = offset.value / 1000;
  const sign = seconds > 0 ? "+" : "";
  return `${sign}${seconds.toFixed(1)}s`;
});

const adjustOffset = (delta: number) => {
  setting.lyricTimeOffset += delta;
};

const resetOffset = () => {
  setting.lyricTimeOffset = 0;
};
</script>

<style lang="scss" scoped>
.lyric-offset-control {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  z-index: 10;
  // Hidden state: skip paint entirely
  visibility: hidden;
  opacity: 0;
  pointer-events: none;
  transition:
    opacity 0.25s ease,
    visibility 0.25s ease;

  // When offset ≠ 0: show only the badge
  &.has-offset .offset-value {
    visibility: visible;
    opacity: 0.6;
    pointer-events: auto;
  }

  // When parent says visible: show everything
  &.visible {
    visibility: visible;
    opacity: 1;
    pointer-events: auto;

    .offset-btn {
      visibility: visible;
      opacity: 0.6;
    }

    .offset-value {
      visibility: visible;
      opacity: 0.6;
    }

    &.has-offset .offset-value {
      opacity: 0.8;
    }
  }
}

.offset-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border-radius: 50%;
  cursor: pointer;
  color: var(--main-cover-color);
  // Hidden by default
  visibility: hidden;
  opacity: 0;
  transition:
    opacity 0.2s ease,
    visibility 0.2s ease,
    background-color 0.15s ease,
    transform 0.1s ease;

  &:hover {
    opacity: 1 !important;
    background: rgba(255, 255, 255, 0.12);
  }

  &:active {
    transform: scale(0.88);
  }
}

.offset-value {
  font-size: 11px;
  font-family: "SF Mono", "Menlo", "Monaco", "Consolas", monospace;
  color: var(--main-cover-color);
  cursor: pointer;
  padding: 3px 8px;
  border-radius: 8px;
  white-space: nowrap;
  text-align: center;
  line-height: 1.2;
  user-select: none;
  // Hidden by default
  visibility: hidden;
  opacity: 0;
  transition:
    opacity 0.2s ease,
    visibility 0.2s ease,
    background-color 0.15s ease;

  &:hover {
    opacity: 1 !important;
    background: rgba(255, 255, 255, 0.12);
  }
}
</style>
