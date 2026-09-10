<template>
  <div class="artist-overview">
    <section class="overview-grid">
      <div class="latest-section" v-if="latestRelease || releaseLoading || releaseError">
        <button v-content-intro class="section-title" type="button" @click="goAlbums">
          <span>{{ $t("general.name.latestRelease") }}</span>
        </button>
        <PageLoadState
          v-if="releaseLoading || releaseError"
          :loading="releaseLoading"
          :error="releaseError"
          @retry="loadRelease"
        />
        <div
          v-else-if="latestRelease"
          v-content-intro
          class="latest-release"
          role="link"
          tabindex="0"
          :data-navigation-identity="`album:${latestRelease.id}`"
          @click="goAlbum(latestRelease.id, $event)"
          @keydown.enter="goAlbum(latestRelease.id, $event)"
        >
          <img
            class="latest-cover"
            data-navigation-cover
            :src="getCoverUrl(latestRelease.cover, 360)"
            alt="album"
            loading="lazy"
          />
          <div class="latest-meta">
            <div class="latest-date">{{ latestRelease.time }}</div>
            <div class="latest-name" data-navigation-title>{{ latestRelease.name }}</div>
            <div class="latest-type">{{ latestRelease.type || $t("general.name.album") }}</div>
          </div>
        </div>
      </div>

      <div class="ranking-section">
        <button
          v-content-intro
          class="section-title"
          type="button"
          @click="navigation.openPage(`/all-songs?id=${artistId}&page=1`, { origin: $event })"
        >
          <span>{{ $t("general.name.songRanking") }}</span>
          <n-icon :component="ChevronRightRound" />
        </button>
        <PageLoadState
          v-if="songsLoading || songsError"
          :loading="songsLoading"
          :error="songsError"
          @retry="loadSongs"
        />
        <div v-content-intro class="song-rank-grid" v-else-if="rankSongs.length">
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

    <n-space v-content-intro justify="center" v-if="artistData[0]">
      <n-button
        class="more"
        size="large"
        strong
        secondary
        round
        @click="navigation.openPage(`/all-songs?id=${artistId}&page=1`, { origin: $event })"
      >
        {{ $t("general.name.allSong") }}
      </n-button>
    </n-space>
  </div>
</template>

<script setup lang="ts">
import { getArtistSongs } from "@/api/artist";
import { getArtistAlbums } from "@/api/album";
import { useRoute } from "vue-router";
import { transformSongData } from "@/utils/ncm/transformSongData";
import { getLongTime } from "@/utils/timeTools";
import getCoverUrl from "@/utils/ncm/getCoverUrl";
import { usePlayAllSong } from "@/composables/usePlayAllSong";
import { ChevronRightRound, PlayArrowRound } from "@vicons/material";
import { useLayerNavigation } from "@/utils/navigation";
import PageLoadState from "@/components/Navigation/PageLoadState.vue";
import { useContentIntro } from "@/composables/useContentIntro";

interface AlbumOverview {
  id: number;
  cover: string;
  name: string;
  time: string;
  type?: string;
}

const navigation = useLayerNavigation();
const { vContentIntro } = useContentIntro();
const { playAllSong } = usePlayAllSong();

// 歌手数据
const artistId = useRoute().query.id;
const artistData = ref<any[]>([]);
const latestReleaseData = ref<AlbumOverview | null>(null);
const songsLoading = ref(true);
const songsError = ref(false);
const releaseLoading = ref(true);
const releaseError = ref(false);

const rankSongs = computed(() => artistData.value.slice(0, 12));
const latestRelease = computed(() => latestReleaseData.value);

// 获取歌手热门歌曲
const loadSongs = async () => {
  songsLoading.value = true;
  songsError.value = false;
  try {
    const res = await getArtistSongs(Number(artistId), { hiddenBar: true });
    artistData.value = res.hotSongs?.length ? transformSongData(res.hotSongs) : [];
  } catch (error) {
    songsError.value = true;
    console.warn("[artist] overview songs failed to load", error);
  } finally {
    songsLoading.value = false;
  }
};

const loadRelease = async () => {
  releaseLoading.value = true;
  releaseError.value = false;
  try {
    const res = await getArtistAlbums(Number(artistId), 30, 0, { hiddenBar: true });
    const latest = res.hotAlbums?.[0];
    latestReleaseData.value = latest
      ? {
          id: latest.id,
          cover: latest.picUrl,
          name: latest.name,
          time: getLongTime(latest.publishTime),
          type: latest.type,
        }
      : null;
  } catch (error) {
    releaseError.value = true;
    console.warn("[artist] latest release failed to load", error);
  } finally {
    releaseLoading.value = false;
  }
};

const playFrom = (index: number) => {
  const nextQueue = artistData.value.slice(index).concat(artistData.value.slice(0, index));
  playAllSong(nextQueue);
};

const goAlbum = (id: number, origin: Event) => {
  navigation.openPage(
    {
      path: "/album",
      query: { id },
    },
    { origin, kind: "card", identity: `album:${id}` },
  );
};

const goAlbums = () => {
  navigation.replacePage({
    path: "/artist/albums",
    query: {
      id: artistId,
      page: 1,
    },
  });
};

onMounted(() => {
  void loadSongs();
  void loadRelease();
});
</script>

<style lang="scss" scoped>
.artist-overview {
  container: artist-overview / inline-size;

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
    grid-template-columns: 1fr;
    gap: clamp(24px, 4vw, 44px);
    align-items: start;
  }

  .latest-section {
    container: latest-section / inline-size;
  }

  .ranking-section {
    container: ranking-section / inline-size;
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
    border-radius: var(--radius-md);
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
    grid-template-columns: 1fr;
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
    border-radius: var(--radius-sm);
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
    transition: all var(--duration-300) var(--ease-out);

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

  /* Side-by-side overview only when the content panel is wide enough */
  @container artist-overview (min-width: 820px) {
    .overview-grid {
      grid-template-columns: minmax(230px, 0.48fr) minmax(0, 1fr);
    }
  }

  /* Latest release: stack the cover above the album name when its column is cramped */
  @container latest-section (max-width: 340px) {
    .latest-release {
      grid-template-columns: 1fr;
      gap: 12px;
    }

    .latest-cover {
      max-width: 188px;
    }
  }

  /* Song ranking: step the column count down as the section narrows */
  @container ranking-section (min-width: 460px) {
    .song-rank-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }

  @container ranking-section (min-width: 700px) {
    .song-rank-grid {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
  }
}
</style>
