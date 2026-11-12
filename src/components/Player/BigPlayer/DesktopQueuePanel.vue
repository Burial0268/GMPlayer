<template>
  <aside
    :class="['desktop-queue-panel', { show }]"
    :aria-hidden="!show"
    :inert="!show"
    data-navigation-layer="queue-player"
  >
    <div class="queue-surface" aria-hidden="true"></div>
    <div class="queue-content">
      <PlayerQueueList
        v-if="music.getPlaylists.length"
        ref="queueListRef"
        class="queue-list"
        row-prefix="desktop-queue"
      />
      <div class="queue-empty" v-else>
        {{ $t("other.playlistEmpty") }}
      </div>

      <!-- 底部整宽定位按钮：网格第三行，常驻面板底边。 -->
      <button
        v-if="music.getPlaylists.length"
        class="queue-locate"
        type="button"
        @click="scrollCurrentQueueSong"
      >
        <n-icon size="18" :component="MyLocationRound" />
        <span>{{ $t("player.queue.locate") }}</span>
      </button>
    </div>
  </aside>
</template>

<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from "vue";
import { MyLocationRound } from "@vicons/material";
import { musicStore } from "@/store";
import PlayerQueueList from "./PlayerQueueList.vue";

const props = defineProps<{
  show: boolean;
}>();

const music = musicStore();
const queueListRef = ref<InstanceType<typeof PlayerQueueList> | null>(null);
const scrollTimer = ref<number | null>(null);
const scrollCurrentQueueSong = () => queueListRef.value?.scrollToCurrent();

watch(
  () => [props.show, music.persistData.playSongIndex] as const,
  ([show], [wasShown]) => {
    if (scrollTimer.value) window.clearTimeout(scrollTimer.value);
    if (!show) return;
    // 刚打开就对准：面板一直挂着，滑入过程中看到的已经是当前行。打开期间切歌则稍后跟过去。
    if (!wasShown) nextTick(scrollCurrentQueueSong);
    else scrollTimer.value = window.setTimeout(scrollCurrentQueueSong, 360);
  },
);

onBeforeUnmount(() => {
  if (scrollTimer.value) window.clearTimeout(scrollTimer.value);
});
</script>

<style lang="scss" scoped>
.desktop-queue-panel {
  position: absolute;
  top: clamp(86px, 10vh, 116px);
  right: clamp(24px, 4vw, 64px);
  bottom: clamp(88px, 12vh, 118px);
  width: clamp(340px, 28vw, 430px);
  box-sizing: border-box;
  pointer-events: none;

  // 面板自己不挂 opacity / transform / filter / z-index：那是 stacking context，
  // 里面的 plus-lighter 在滑入期间混不出去，落定才突然加亮。毛玻璃和文字拆成同级，
  // 各自套同一套显隐；文字自己带混合，自己动不隔断。
  &.show {
    pointer-events: auto;

    .queue-surface,
    .queue-content {
      opacity: 1;
      transform: translate3d(0, 0, 0) scale(1);
    }
  }
}

.queue-surface,
.queue-content {
  position: absolute;
  inset: 0;
  border-radius: var(--radius-md);
  opacity: 0;
  transform: translate3d(28px, 0, 0) scale(0.985);
  transform-origin: right center;
  transition:
    opacity 0.32s cubic-bezier(0.25, 1, 0.5, 1),
    transform 0.46s cubic-bezier(0.25, 1, 0.5, 1);
}

.queue-surface {
  z-index: 6;
  background: rgba(20, 20, 20, 0.2);
  border: 1px solid rgba(255, 255, 255, 0.12);
  box-shadow: 0 24px 72px rgba(0, 0, 0, 0.26);
  -webkit-backdrop-filter: blur(36px) saturate(1.2);
  backdrop-filter: blur(36px) saturate(1.2);
  pointer-events: none;
}

.queue-content {
  z-index: 6;
  display: grid;
  grid-template-rows: minmax(0, 1fr) auto;
  padding: 18px 14px 14px;
  box-sizing: border-box;
  color: var(--main-cover-mix-color, rgb(239, 239, 239));
  // 混合不挂在这块上：封面是它的子节点，会跟着被加亮。只选文字和图标。
  // The rows blend themselves (PlayerQueueList).
  :is(.queue-locate, .queue-empty),
  :deep(.n-icon),
  :deep(svg),
  :deep(path) {
    mix-blend-mode: plus-lighter;
  }
}

.queue-list {
  min-height: 0;
  overflow-x: clip;
  overscroll-behavior: contain;
  padding: 2px 2px 8px;
  contain: layout paint style;

  :deep(.v-vl) {
    overflow-x: hidden !important;
    scrollbar-width: none;
  }

  :deep(.v-vl::-webkit-scrollbar) {
    width: 0;
    height: 0;
  }
}

.queue-empty {
  height: 40vh;
  display: flex;
  align-items: center;
  justify-content: center;
  text-align: center;
  opacity: 0.56;
}

// 底部整宽定位按钮（网格第三行）。面板叠在专辑封面上，配色随 --main-cover-color。
.queue-locate {
  margin-top: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 11px 16px;
  border: 1px solid color-mix(in srgb, var(--main-cover-color) 20%, transparent);
  border-radius: var(--radius-md);
  color: var(--main-cover-mix-color, rgb(239, 239, 239));
  background: color-mix(in srgb, var(--main-cover-color) 12%, transparent);
  font: inherit;
  font-size: 0.85rem;
  font-weight: 640;
  cursor: pointer;
  transition:
    background-color 0.16s ease,
    border-color 0.16s ease,
    transform 0.16s ease;

  .n-icon {
    color: var(--main-cover-mix-color, rgb(239, 239, 239));
  }

  &:focus-visible {
    background: color-mix(in srgb, var(--main-cover-color) 20%, transparent);
    border-color: color-mix(in srgb, var(--main-cover-color) 32%, transparent);
  }

  @media (hover: hover) {
    &:hover {
      background: color-mix(in srgb, var(--main-cover-color) 20%, transparent);
      border-color: color-mix(in srgb, var(--main-cover-color) 32%, transparent);
    }
  }

  &:active {
    transform: scale(0.985);
  }
}

@media (max-width: 1180px) {
  .desktop-queue-panel {
    right: 24px;
    width: min(390px, calc(100vw - 48px));
  }
}
</style>
