<template>
  <div
    class="lyric-offset-control"
    :class="{
      visible: visible,
      'has-offset': offset !== 0,
    }"
  >
    <!-- 翻译语言：仅 TTML 带多种语言翻译时出现 -->
    <n-popselect
      v-if="languageOptions.length > 1"
      v-model:value="selectedLanguage"
      :options="languageOptions"
      trigger="click"
      size="small"
    >
      <div v-button class="offset-btn" :title="t('setting.lyricTranslationLanguage')" @click.stop>
        <n-icon size="18" :component="TranslateFilled" />
      </div>
    </n-popselect>

    <!-- [+] button -->
    <div
      v-button
      class="offset-btn"
      @click.stop="adjustOffset(LYRIC_OFFSET_STEP)"
      :title="t('setting.lyricOffsetAdvance')"
    >
      <n-icon size="18" :component="AddRound" />
    </div>

    <!-- Offset value badge (click to reset) -->
    <div
      v-button
      class="offset-value"
      @click.stop="resetOffset"
      :title="t('setting.lyricOffsetReset')"
    >
      {{ offsetDisplay }}
    </div>

    <!-- [-] button -->
    <div
      v-button
      class="offset-btn"
      @click.stop="adjustOffset(-LYRIC_OFFSET_STEP)"
      :title="t('setting.lyricOffsetDelay')"
    >
      <n-icon size="18" :component="RemoveRound" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { vButton } from "@/utils/buttonRole";
import { AddRound, RemoveRound, TranslateFilled } from "@vicons/material";
import { useI18n } from "vue-i18n";
import { LYRIC_OFFSET_STEP, useLyricControls } from "@/composables/useLyricControls";

const { t } = useI18n();

defineProps<{
  visible?: boolean;
}>();

const { languageOptions, selectedLanguage, offset, offsetDisplay, adjustOffset, resetOffset } =
  useLyricControls();
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
    opacity var(--duration-200) var(--ease-out),
    visibility var(--duration-200) var(--ease-out);

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
    opacity var(--duration-200) var(--ease-out),
    visibility var(--duration-200) var(--ease-out),
    background-color var(--duration-150) var(--ease-out),
    transform var(--duration-150) var(--ease-out);

  @media (hover: hover) {
    &:hover {
      opacity: 1 !important;
      background: rgba(255, 255, 255, 0.12);
    }
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
  border-radius: var(--radius-md);
  white-space: nowrap;
  text-align: center;
  line-height: 1.2;
  user-select: none;
  // Hidden by default
  visibility: hidden;
  opacity: 0;
  transition:
    opacity var(--duration-200) var(--ease-out),
    visibility var(--duration-200) var(--ease-out),
    background-color var(--duration-150) var(--ease-out);

  @media (hover: hover) {
    &:hover {
      opacity: 1 !important;
      background: rgba(255, 255, 255, 0.12);
    }
  }
}
</style>
