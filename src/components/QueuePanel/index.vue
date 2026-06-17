<template>
  <aside class="queue-panel">
    <section class="queue-now">
      <div class="queue-title">{{ $t("player.queue.nowPlaying") }}</div>
      <div v-if="currentSong" class="now-card">
        <img
          class="now-cover"
          :src="
            currentSong.album?.picUrl
              ? currentSong.album.picUrl.replace(/^http:/, 'https:') + '?param=80y80'
              : '/images/pic/default.png'
          "
          alt="cover"
        />
        <div class="now-meta">
          <div class="now-name text-hidden">{{ currentSong.name }}</div>
          <AllArtists class="now-artists text-hidden" :artistsData="currentSong.artist" />
        </div>
      </div>
      <div v-else class="queue-empty">{{ $t("other.playlistEmpty") }}</div>
    </section>

    <section class="queue-list-section">
      <div class="queue-list-header">
        <span>{{ $t("player.queue.playingNext") }}</span>
        <span v-if="music.getPlaylists.length" class="queue-count">
          {{ $t("general.name.songSize", { size: music.getPlaylists.length }) }}
        </span>
      </div>
      <n-scrollbar class="queue-scroll">
        <div v-if="music.getPlaylists.length" class="queue-list">
          <div
            v-for="(item, index) in music.getPlaylists"
            :key="`${item.id}-${index}`"
            :class="['queue-row', { active: index === music.persistData.playSongIndex }]"
            @click="changeIndex(index)"
          >
            <div class="queue-index">
              <span v-if="index !== music.persistData.playSongIndex">{{ index + 1 }}</span>
              <div v-else class="queue-bars">
                <span v-for="bar in 3" :key="bar" :style="{ animationDelay: `${bar * 0.12}s` }" />
              </div>
            </div>
            <img
              class="queue-cover"
              :src="
                item.album?.picUrl
                  ? item.album.picUrl.replace(/^http:/, 'https:') + '?param=60y60'
                  : '/images/pic/default.png'
              "
              alt="cover"
              loading="lazy"
            />
            <div class="queue-meta">
              <div class="queue-name text-hidden">{{ item.name }}</div>
              <AllArtists class="queue-artists text-hidden" :artistsData="item.artist" />
            </div>
            <n-icon
              class="queue-remove"
              :size="17"
              :component="DeleteFour"
              @click.stop="music.removeSong(index)"
            />
          </div>
        </div>
        <div v-else class="queue-empty">{{ $t("other.playlistEmpty") }}</div>
      </n-scrollbar>
    </section>
  </aside>
</template>

<script setup>
import { NIcon, NScrollbar } from "naive-ui";
import { DeleteFour } from "@icon-park/vue-next";
import { musicStore } from "@/store";
import { soundStop } from "@/utils/AudioContext";
import AllArtists from "@/components/DataList/AllArtists.vue";

const music = musicStore();

const currentSong = computed(() => music.getPlaySongData);

const changeIndex = (index) => {
  if (index === music.persistData.playSongIndex) return;
  if (typeof $player !== "undefined") soundStop($player);
  music.persistData.playSongIndex = index;
  music.isLoadingSong = true;
  music.setPlayState(true);
};
</script>

<style lang="scss" scoped>
.queue-panel {
  height: 100%;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background-color: var(--app-shell-bg, var(--layout-bg, #fff));
}

.queue-now {
  padding: 16px 14px 12px;
  border-bottom: 1px solid var(--acrylic-border, rgba(0, 0, 0, 0.06));
}

.queue-title,
.queue-list-header {
  font-size: 13px;
  font-weight: 700;
  color: var(--n-text-color);
}

.now-card {
  display: grid;
  grid-template-columns: 42px minmax(0, 1fr);
  gap: 10px;
  align-items: center;
  margin-top: 12px;
  padding: 8px;
  border-radius: var(--radius-md);
  background-color: color-mix(in srgb, var(--n-text-color) 6%, transparent);
}

.now-cover,
.queue-cover {
  width: 42px;
  height: 42px;
  border-radius: var(--radius-sm);
  object-fit: cover;
}

.now-meta,
.queue-meta {
  min-width: 0;
}

.now-name,
.queue-name {
  font-size: 13px;
  font-weight: 650;
  color: var(--n-text-color);
}

.now-artists,
.queue-artists {
  margin-top: 2px;
  font-size: 12px;
  color: var(--n-text-color-3);
}

.queue-list-section {
  min-height: 0;
  flex: 1;
  display: flex;
  flex-direction: column;
}

.queue-list-header {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 10px;
  padding: 14px 14px 8px;
}

.queue-count {
  flex: 0 0 auto;
  font-size: 11px;
  font-weight: 500;
  color: var(--n-text-color-3);
}

.queue-scroll {
  min-height: 0;
  flex: 1;
}

.queue-list {
  padding: 0 8px 12px;
}

.queue-row {
  display: grid;
  grid-template-columns: 26px 34px minmax(0, 1fr) 28px;
  align-items: center;
  gap: 8px;
  min-height: 48px;
  padding: 5px 6px;
  border-radius: var(--radius-md);
  cursor: pointer;
  color: var(--n-text-color-2);
  transition:
    background-color 0.16s ease,
    color 0.16s ease;

  &:nth-child(odd) {
    background-color: color-mix(in srgb, var(--n-text-color) 3%, transparent);
  }

  &:nth-child(even) {
    background-color: color-mix(in srgb, var(--n-text-color) 5%, transparent);
  }

  & + .queue-row {
    margin-top: 2px;
  }

  &:hover {
    background-color: color-mix(in srgb, var(--n-text-color) 9%, transparent);

    .queue-remove {
      opacity: 1;
    }
  }

  &.active {
    color: var(--main-color);
    background-color: color-mix(in srgb, var(--main-color) 14%, transparent);

    .queue-name,
    .queue-artists,
    .queue-index,
    .queue-remove {
      color: var(--main-color);
    }
  }
}

.queue-index {
  text-align: center;
  font-size: 11px;
  color: var(--n-text-color-3);
}

.queue-cover {
  width: 34px;
  height: 34px;
}

.queue-remove {
  justify-self: center;
  padding: 5px;
  border-radius: var(--radius-sm);
  color: var(--n-text-color-3);
  opacity: 0;
  transition:
    opacity 0.16s ease,
    background-color 0.16s ease;

  &:hover {
    background-color: color-mix(in srgb, var(--n-text-color) 10%, transparent);
  }
}

.queue-empty {
  padding: 14px;
  font-size: 12px;
  color: var(--n-text-color-3);
}

.queue-bars {
  height: 16px;
  display: flex;
  align-items: end;
  justify-content: center;
  gap: 2px;

  span {
    width: 3px;
    height: 10px;
    border-radius: var(--radius-pill);
    background-color: currentColor;
    animation: queue-bar 0.9s ease-in-out infinite;
  }
}

@keyframes queue-bar {
  0%,
  100% {
    height: 8px;
  }

  50% {
    height: 15px;
  }
}
</style>
