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
    </div>

    <div ref="queueListRef" class="queue-list">
      <div
        v-for="(item, index) in music.getPlaylists"
        :id="`desktop-queue-${index}`"
        :key="`${item.id}-${index}`"
        :class="['queue-song', { 'is-current': index === music.persistData.playSongIndex }]"
        role="button"
        tabindex="0"
        @click="changeQueueIndex(index)"
        @keydown.enter.prevent="changeQueueIndex(index)"
      >
        <div class="queue-index">
          <span v-if="index !== music.persistData.playSongIndex">{{ index + 1 }}</span>
          <div v-else class="playing-bars">
            <span class="line"></span>
            <span class="line"></span>
            <span class="line"></span>
          </div>
        </div>
        <img class="queue-cover" :src="getQueueCover(item)" alt="cover" />
        <div class="queue-info">
          <div class="queue-name text-hidden">{{ item.name }}</div>
          <div class="queue-artists text-hidden">{{ formatArtists(item.artist) }}</div>
        </div>
        <div class="queue-duration" v-if="item.time">{{ item.time }}</div>
        <button class="queue-remove" type="button" @click.stop="music.removeSong(index)">
          <n-icon size="20" :component="DeleteRound" />
        </button>
      </div>
      <div class="queue-empty" v-if="!music.getPlaylists.length">
        {{ $t("other.playlistEmpty") }}
      </div>
    </div>
  </aside>
</template>

<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from "vue";
import { DeleteRound, QueueMusicRound } from "@vicons/material";
import { musicStore } from "@/store";
import { soundStop } from "@/utils/AudioContext";

declare const $player: any;

type Artist = { name: string };
type QueueSong = {
  id: number;
  name: string;
  artist?: Artist[];
  album?: { picUrl?: string };
  time?: string;
};

defineProps<{
  show: boolean;
}>();

const music = musicStore();
const queueListRef = ref<HTMLElement | null>(null);
const scrollTimer = ref<number | null>(null);

const formatArtists = (artists: Artist[] = []) =>
  artists
    .filter(Boolean)
    .map((item) => item.name)
    .join(" / ");

const getQueueCover = (item: QueueSong) => {
  const picUrl = item.album?.picUrl;
  return picUrl ? picUrl.replace(/^http:/, "https:") + "?param=96y96" : "/images/pic/default.png";
};

const scrollCurrentQueueSong = () => {
  const list = queueListRef.value;
  if (!list) return;
  const current = list.querySelector<HTMLElement>(
    `#desktop-queue-${music.persistData.playSongIndex}`,
  );
  current?.scrollIntoView({ behavior: "smooth", block: "center" });
};

const changeQueueIndex = (index: number) => {
  if (music.persistData.playSongIndex === index) return;
  if (typeof $player !== "undefined") soundStop($player);
  music.persistData.playSongIndex = index;
  music.isLoadingSong = true;
  music.resetSongLyricState();
  music.setPlayState(true);
};

watch(
  () => [music.showPlayList, music.persistData.playSongIndex] as const,
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
  border-radius: 8px;
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
  min-width: 0;
  padding: 0 4px 14px;
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
  overflow-y: auto;
  overscroll-behavior: contain;
  padding: 2px 2px 8px;
  scrollbar-width: none;

  &::-webkit-scrollbar {
    display: none;
  }
}

.queue-song {
  min-height: 64px;
  display: grid;
  grid-template-columns: 30px 48px minmax(0, 1fr) auto 36px;
  align-items: center;
  gap: 10px;
  border-radius: 8px;
  padding: 8px 8px 8px 4px;
  margin-bottom: 8px;
  box-sizing: border-box;
  background: rgba(255, 255, 255, 0.075);
  border: 1px solid rgba(255, 255, 255, 0.08);
  cursor: pointer;
  transition:
    background-color 0.22s ease,
    border-color 0.22s ease,
    transform 0.18s ease;

  &:hover {
    background: rgba(255, 255, 255, 0.115);

    .queue-remove {
      opacity: 0.82;
    }
  }

  &:active {
    transform: scale(0.985);
  }

  &.is-current {
    background: color-mix(in srgb, var(--main-cover-color) 18%, transparent);
    border-color: color-mix(in srgb, var(--main-cover-color) 42%, transparent);
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
  width: 48px;
  height: 48px;
  border-radius: 7px;
  object-fit: cover;
  box-shadow: 0 8px 18px rgba(0, 0, 0, 0.24);
}

.queue-info {
  min-width: 0;

  .queue-name {
    font-weight: 650;
    font-size: 0.95rem;
    line-height: 1.2;
  }

  .queue-artists {
    margin-top: 5px;
    font-size: 0.78rem;
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
  width: 36px;
  height: 36px;
  border-radius: 50%;
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0;
  cursor: pointer;
  transition:
    opacity 0.2s ease,
    background-color 0.2s ease,
    transform 0.18s ease;

  &:hover,
  &:focus-visible {
    opacity: 1;
    background: rgba(255, 255, 255, 0.14);
  }

  &:active {
    transform: scale(0.94);
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
