<template>
  <aside :class="['desktop-queue-panel', { show }]" :aria-hidden="!show">
    <div class="queue-header">
      <div class="queue-title">
        <n-icon size="24" :component="QueueMusicRound" />
        <div class="queue-title-text">
          <span class="title">{{ $t("general.name.playlists") }}</span>
          <span class="count" v-if="music.getPlaylists.length">
            {{ $t("general.name.songSize", { size: music.getPlaylists.length }) }}
          </span>
        </div>
      </div>
      <button
        v-if="music.getPlaylists.length"
        class="queue-clear"
        type="button"
        @click="music.clearPlaylists()"
      >
        {{ $t("player.queue.clear") }}
      </button>
    </div>

    <n-virtual-list
      v-if="music.getPlaylists.length"
      ref="queueListRef"
      class="queue-list"
      :items="queueRows"
      :item-size="52"
      :item-resizable="true"
      key-field="key"
      :show-scrollbar="false"
    >
      <template #default="{ item: row }">
        <div
          :id="`desktop-queue-${row.index}`"
          :class="[
            'queue-song',
            {
              'is-current': row.index === music.persistData.playSongIndex,
              'row-odd': row.index % 2 === 0,
              'row-even': row.index % 2 === 1,
              'row-first': row.index === 0,
              'row-last': row.index === music.getPlaylists.length - 1,
            },
          ]"
          role="button"
          tabindex="0"
          @click="changeQueueIndex(row.index)"
          @keydown.enter.prevent="changeQueueIndex(row.index)"
        >
          <div class="queue-index">
            <span v-if="row.index !== music.persistData.playSongIndex">{{ row.index + 1 }}</span>
            <div v-else class="playing-bars">
              <span class="line"></span>
              <span class="line"></span>
              <span class="line"></span>
            </div>
          </div>
          <img class="queue-cover" :src="getQueueCover(row.item)" alt="cover" />
          <div class="queue-info">
            <div class="queue-name text-hidden">{{ row.item.name }}</div>
            <div class="queue-artists text-hidden">{{ formatArtists(row.item.artist) }}</div>
          </div>
          <div class="queue-duration" v-if="row.item.time">{{ row.item.time }}</div>
          <button class="queue-remove" type="button" @click.stop="music.removeSong(row.index)">
            <n-icon size="17" :component="DeleteRound" />
          </button>
        </div>
      </template>
    </n-virtual-list>
    <div class="queue-empty" v-else>
      {{ $t("other.playlistEmpty") }}
    </div>
  </aside>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { NVirtualList } from "naive-ui";
import { DeleteRound, QueueMusicRound } from "@vicons/material";
import { musicStore } from "@/store";
import { coverUrl } from "@/utils/coverUrl";

declare const $player: any;

type Artist = { name: string };
type QueueSong = {
  id: number;
  name: string;
  artist?: Artist[];
  album?: { picUrl?: string };
  time?: string;
};

const props = defineProps<{
  show: boolean;
}>();

const music = musicStore();
const queueListRef = ref<{
  scrollTo: (options: { index: number; behavior?: ScrollBehavior }) => void;
} | null>(null);
const scrollTimer = ref<number | null>(null);
const queueRows = computed(() =>
  music.getPlaylists.map((item: QueueSong, index: number) => ({
    item,
    index,
    key: `${item.id}-${index}`,
  })),
);

const formatArtists = (artists: Artist[] = []) =>
  artists
    .filter(Boolean)
    .map((item) => item.name)
    .join(" / ");

const getQueueCover = (item: QueueSong) => coverUrl(item.album?.picUrl, 96);

const scrollCurrentQueueSong = () => {
  queueListRef.value?.scrollTo({
    index: music.persistData.playSongIndex,
    behavior: "smooth",
  });
};

const changeQueueIndex = (index: number) => {
  music.selectPlaySongByIndex(index);
};

watch(
  () => [props.show, music.persistData.playSongIndex] as const,
  ([show]) => {
    if (scrollTimer.value) window.clearTimeout(scrollTimer.value);
    if (!show) return;
    nextTick(() => {
      scrollTimer.value = window.setTimeout(scrollCurrentQueueSong, 360);
    });
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
  z-index: 6;
  box-sizing: border-box;
  display: grid;
  grid-template-rows: auto minmax(0, 1fr);
  padding: 18px 14px 14px;
  border-radius: var(--radius-md);
  color: var(--main-cover-color);
  background: rgba(20, 20, 20, 0.2);
  border: 1px solid rgba(255, 255, 255, 0.12);
  box-shadow: 0 24px 72px rgba(0, 0, 0, 0.26);
  -webkit-backdrop-filter: blur(36px) saturate(1.2);
  backdrop-filter: blur(36px) saturate(1.2);
  opacity: 0;
  pointer-events: none;
  transform: translate3d(28px, 0, 0) scale(0.985);
  transform-origin: right center;
  transition:
    opacity 0.32s cubic-bezier(0.25, 1, 0.5, 1),
    transform 0.46s cubic-bezier(0.25, 1, 0.5, 1);
  will-change: opacity, transform;

  &.show {
    opacity: 1;
    pointer-events: auto;
    transform: translate3d(0, 0, 0) scale(1);
  }
}

.queue-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  min-width: 0;
  padding: 0 4px 14px;
}

.queue-clear {
  flex: 0 0 auto;
  padding: 6px 10px;
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: var(--radius-sm);
  color: inherit;
  background: rgba(255, 255, 255, 0.06);
  font: inherit;
  font-size: 0.76rem;
  cursor: pointer;
  opacity: 0.72;
  transition:
    opacity 0.16s ease,
    background-color 0.16s ease,
    border-color 0.16s ease;

  &:hover,
  &:focus-visible {
    opacity: 1;
    background: rgba(255, 255, 255, 0.12);
    border-color: rgba(255, 255, 255, 0.2);
    outline: none;
  }
}

.queue-title {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 10px;

  > .n-icon {
    flex-shrink: 0;
    opacity: 0.9;
  }
}

.queue-title-text {
  min-width: 0;
  display: flex;
  flex-direction: column;

  .title {
    font-size: 1.1rem;
    font-weight: 720;
    line-height: 1.2;
  }

  .count {
    margin-top: 3px;
    font-size: 0.78rem;
    opacity: 0.62;
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

// 与 QueuePanel / HistoryView 一致的斑马纹连续列表，仅将中性色换成
// 封面自适应的 --main-cover-color（面板叠在专辑封面之上）
.queue-song {
  min-height: 52px;
  display: grid;
  grid-template-columns: 26px 38px minmax(0, 1fr) auto 28px;
  align-items: center;
  gap: 10px;
  border-radius: 0;
  padding: 6px 8px;
  box-sizing: border-box;
  cursor: pointer;
  transition: background-color var(--duration-150) var(--ease-out);

  &.row-first {
    border-radius: var(--radius-md) var(--radius-md) 0 0;
  }

  &.row-last {
    border-radius: 0 0 var(--radius-md) var(--radius-md);
  }

  &.row-odd {
    background: color-mix(in srgb, var(--main-cover-color) 5%, transparent);
  }

  &.row-even {
    background: color-mix(in srgb, var(--main-cover-color) 9%, transparent);
  }

  &:hover {
    background: color-mix(in srgb, var(--main-cover-color) 14%, transparent);

    .queue-remove {
      opacity: 0.82;
    }
  }

  &.is-current {
    background: color-mix(in srgb, var(--main-cover-color) 22%, transparent);
  }
}

.queue-index {
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 0.78rem;
  opacity: 0.66;
  font-variant-numeric: tabular-nums;
}

.playing-bars {
  height: 18px;
  width: 18px;
  display: flex;
  align-items: flex-end;
  justify-content: center;
  gap: 3px;

  .line {
    width: 3px;
    min-height: 7px;
    border-radius: 3px;
    background: var(--main-cover-color);
    animation: queue-line-move 0.9s ease-in-out infinite;

    &:nth-child(2) {
      animation-delay: 0.12s;
    }

    &:nth-child(3) {
      animation-delay: 0.24s;
    }
  }
}

.queue-cover {
  width: 38px;
  height: 38px;
  border-radius: var(--radius-sm);
  object-fit: cover;
}

.queue-info {
  min-width: 0;

  .queue-name {
    font-weight: 650;
    font-size: 0.85rem;
    line-height: 1.25;
  }

  .queue-artists {
    margin-top: 2px;
    font-size: 0.75rem;
    opacity: 0.62;
  }
}

.queue-duration {
  font-size: 0.76rem;
  opacity: 0.54;
  font-variant-numeric: tabular-nums;
}

.queue-remove {
  appearance: none;
  border: none;
  background: transparent;
  color: var(--main-cover-color);
  width: 28px;
  height: 28px;
  border-radius: var(--radius-sm);
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0;
  cursor: pointer;
  transition:
    opacity var(--duration-200) var(--ease-out),
    background-color var(--duration-200) var(--ease-out);

  &:hover,
  &:focus-visible {
    opacity: 1;
    background: color-mix(in srgb, var(--main-cover-color) 14%, transparent);
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

@keyframes queue-line-move {
  0%,
  100% {
    height: 16px;
  }

  50% {
    height: 8px;
  }
}

@media (max-width: 1180px) {
  .desktop-queue-panel {
    right: 24px;
    width: min(390px, calc(100vw - 48px));
  }
}
</style>
