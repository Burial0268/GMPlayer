<template>
  <div class="artist-overview">
    <section class="overview-grid">
      <div class="latest-section" v-if="latestRelease">
        <button class="section-title" type="button" @click="goAlbums">
          <span>{{ $t("general.name.latestRelease") }}</span>
        </button>
        <div class="latest-release" @click="goAlbum(latestRelease.id)">
          <img
            class="latest-cover"
            :src="getCoverUrl(latestRelease.cover, 360)"
            alt="album"
            loading="lazy"
          />
          <div class="latest-meta">
            <div class="latest-date">{{ latestRelease.time }}</div>
            <div class="latest-name">{{ latestRelease.name }}</div>
            <div class="latest-type">{{ latestRelease.type || $t("general.name.album") }}</div>
          </div>
        </div>
      </div>

      <div class="ranking-section">
        <button class="section-title" type="button" @click="router.push(`/all-songs?id=${artistId}&page=1`)">
          <span>{{ $t("general.name.songRanking") }}</span>
          <n-icon :component="ChevronRightRound" />
        </button>
        <div class="song-rank-grid" v-if="rankSongs.length">
          <button
            v-for="(song, index) in rankSongs"
            :key="song.id"
            class="rank-row"
            type="button"
            @click="playFrom(index)"
          >
            <img
              class="rank-cover"
              :src="getCoverUrl(song.album?.picUrl, 80)"
              alt="cover"
              loading="lazy"
            />
            <div class="rank-meta">
              <div class="rank-name text-hidden">{{ song.name }}</div>
              <div class="rank-sub text-hidden">{{ song.album?.name }}</div>
            </div>
            <n-icon class="rank-play" :component="PlayArrowRound" />
          </button>
        </div>
        <n-empty v-else class="empty" />
      </div>
    </section>

    <n-space justify="center" v-if="artistData[0]">
      <n-button
        class="more"
        size="large"
        strong
        secondary
        round
        @click="router.push(`/all-songs?id=${artistId}&page=1`)"
      >
        {{ $t("general.name.allSong") }}
      </n-button>
    </n-space>
  </div>
</template>

<script setup lang="ts">
import { getArtistSongs } from "@/api/artist";
import { getArtistAlbums } from "@/api/album";
import { useRouter } from "vue-router";
import { transformSongData } from "@/utils/ncm/transformSongData";
import { getLongTime } from "@/utils/timeTools";
import getCoverUrl from "@/utils/ncm/getCoverUrl";
import { usePlayAllSong } from "@/composables/usePlayAllSong";
import { ChevronRightRound, PlayArrowRound } from "@vicons/material";

interface AlbumOverview {
  id: number;
  cover: string;
  name: string;
  time: string;
  type?: string;
}

const router = useRouter();
const { playAllSong } = usePlayAllSong();

// 歌手数据
const artistId = ref(router.currentRoute.value.query.id);
const artistData = ref<any[]>([]);
const latestReleaseData = ref<AlbumOverview | null>(null);

const rankSongs = computed(() => artistData.value.slice(0, 12));
const latestRelease = computed(() => latestReleaseData.value);

// 获取歌手热门歌曲
const getArtistSongsData = async (id: string | number | string[]) => {
  const res = await getArtistSongs(Number(id));
  artistData.value = res.hotSongs?.length ? transformSongData(res.hotSongs) : [];
};

const getArtistAlbumsData = async (id: string | number | string[]) => {
  const res = await getArtistAlbums(Number(id), 30, 0);
  const rawAlbums = res.hotAlbums ?? [];
  const latest = rawAlbums[0];
  latestReleaseData.value = latest
    ? {
        id: latest.id,
        cover: latest.picUrl,
        name: latest.name,
        time: getLongTime(latest.publishTime),
        type: latest.type,
      }
    : null;
};

const refreshArtistOverview = async (id: string | number | string[]) => {
  await Promise.all([getArtistSongsData(id), getArtistAlbumsData(id)]);
};

const playFrom = (index: number) => {
  const nextQueue = artistData.value.slice(index).concat(artistData.value.slice(0, index));
  playAllSong(nextQueue);
};

const goAlbum = (id: number) => {
  router.push({
    path: "/album",
    query: { id },
  });
};

const goAlbums = () => {
  router.push({
    path: "/artist/albums",
    query: {
      id: artistId.value,
      page: 1,
    },
  });
};

onMounted(() => {
  refreshArtistOverview(artistId.value);
});

// 监听路由参数变化
watch(
  () => router.currentRoute.value,
  (val) => {
    artistId.value = val.query.id;
    if (val.name === "ar-songs") {
      refreshArtistOverview(artistId.value);
    }
  },
);
</script>

<style lang="scss" scoped>
.artist-overview {
  .section-title {
    appearance: none;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin: 0 0 14px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--n-text-color);
    font-size: 19px;
    font-weight: 800;
    cursor: pointer;

    .n-icon {
      color: var(--n-text-color-3);
      font-size: 22px;
      transition: transform var(--duration-150) var(--ease-out);
    }

    &:hover {
      color: var(--main-color);

      .n-icon {
        transform: translateX(2px);
      }
    }
  }

  .overview-grid {
    display: grid;
    grid-template-columns: minmax(230px, 0.48fr) minmax(0, 1fr);
    gap: clamp(24px, 4vw, 44px);
    align-items: start;
  }

  .latest-release {
    display: grid;
    grid-template-columns: minmax(142px, 186px) minmax(0, 1fr);
    gap: 18px;
    align-items: center;
    cursor: pointer;
  }

  .latest-cover {
    width: 100%;
    aspect-ratio: 1;
    border-radius: 8px;
    object-fit: cover;
    box-shadow: 0 14px 28px rgb(0 0 0 / 12%);
  }

  .latest-meta {
    min-width: 0;
  }

  .latest-date,
  .latest-type,
  .album-time,
  .rank-sub {
    color: var(--n-text-color-3);
  }

  .latest-date,
  .latest-type {
    font-size: 13px;
  }

  .latest-name {
    margin: 6px 0;
    font-size: 18px;
    font-weight: 650;
    line-height: 1.35;
    color: var(--n-text-color);
  }

  .song-rank-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    column-gap: 22px;
  }

  .rank-row {
    appearance: none;
    min-width: 0;
    height: 54px;
    display: grid;
    grid-template-columns: 44px minmax(0, 1fr) 24px;
    align-items: center;
    gap: 10px;
    padding: 5px 0;
    border: 0;
    border-top: 1px solid var(--acrylic-border, rgba(0, 0, 0, 0.08));
    background: transparent;
    color: inherit;
    text-align: left;
    cursor: pointer;

    &:hover {
      .rank-name,
      .rank-play {
        color: var(--main-color);
      }
    }
  }

  .rank-cover {
    width: 44px;
    height: 44px;
    border-radius: 6px;
    object-fit: cover;
  }

  .rank-meta {
    min-width: 0;
  }

  .rank-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--n-text-color);
    transition: color var(--duration-150) var(--ease-out);
  }

  .rank-sub {
    margin-top: 2px;
    font-size: 12px;
  }

  .rank-play {
    justify-self: end;
    color: var(--n-text-color-3);
    opacity: 0.82;
    transition: color var(--duration-150) var(--ease-out);
  }

  .more {
    margin-top: 40px;
    width: 140px;
    font-size: 16px;
    transition: all 0.3s;

    &:hover {
      background-color: var(--main-second-color);
      color: var(--main-color);
    }

    &:active {
      transform: scale(0.95);
    }
  }

  .empty {
    margin: 24px 0;
  }

  @media (max-width: 1180px) {
    .overview-grid {
      grid-template-columns: 1fr;
    }

    .song-rank-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

  }

  @media (max-width: 680px) {
    .latest-release {
      grid-template-columns: 118px minmax(0, 1fr);
    }

    .song-rank-grid {
      grid-template-columns: 1fr;
    }

  }
}
</style>
