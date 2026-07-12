<template>
  <div class="home">
    <header class="home-header home-section">
      <div>
        <span class="date">{{ dateText }}</span>
        <h1>{{ greetingText }}</h1>
      </div>
    </header>

    <section v-if="setting.bannerShow" class="banner-section home-section">
      <Banner />
    </section>

    <section class="featured-section home-section">
      <div class="section-heading">
        <div>
          <span class="section-kicker">{{ $t("home.kicker.featured") }}</span>
          <h2>{{ $t("home.title.forYou") }}</h2>
        </div>
      </div>

      <div class="featured-grid">
        <PaDailySongs class="daily-feature" />
        <PaPersonalFm class="fm-feature" />
        <PaRadar class="radar-feature" />
        <PaLikeSongs class="like-feature" />
      </div>
    </section>

    <section class="feed-section home-section">
      <div class="section-heading feed-heading">
        <div>
          <span class="section-kicker">{{ $t("home.kicker.explore") }}</span>
          <h2>{{ $t("home.title.exclusive") }}</h2>
        </div>
        <nav class="feed-links" :aria-label="$t('home.title.exclusive')">
          <button type="button" @click="router.push('/discover/playlists?page=1')">
            {{ $t("home.title.playlists") }}
          </button>
          <button type="button" @click="router.push('/discover/artists?page=1')">
            {{ $t("home.title.artists") }}
          </button>
          <button type="button" @click="router.push('/new-album?page=1')">
            {{ $t("home.title.newAlbum") }}
          </button>
        </nav>
      </div>

      <div v-if="isLoading" class="feed-grid" aria-hidden="true">
        <n-skeleton
          v-for="index in 8"
          :key="index"
          class="feed-skeleton"
          :class="{ 'is-wide': index === 1 || index === 6 }"
          :sharp="false"
        />
      </div>

      <div v-else class="feed-grid">
        <article v-if="newSongsData.length" v-masonry-item class="hot-tracks-card">
          <header class="hot-tracks-card__header">
            <div>
              <span>{{ $t("home.modules.hotTracks.eyebrow") }}</span>
              <h3>{{ $t("home.modules.hotTracks.title") }}</h3>
              <p>{{ $t("home.modules.hotTracks.subtitle") }}</p>
            </div>
          </header>
          <div class="hot-tracks-card__list">
            <button
              v-for="(song, index) in newSongsData.slice(0, 5)"
              :key="song.id"
              type="button"
              class="hot-track"
              @click="playHotTrack(song)"
            >
              <span class="hot-track__index">{{ String(index + 1).padStart(2, "0") }}</span>
              <img
                :src="normalizeCover(song.album?.picUrl, 160)"
                :alt="song.name"
                loading="lazy"
                @error="useFallbackCover"
              />
              <span class="hot-track__copy">
                <strong>{{ song.name }}</strong>
                <small>{{ getArtistNames(song) }}</small>
              </span>
              <n-icon class="hot-track__play" :component="PlayArrowRound" size="20" />
            </button>
          </div>
        </article>

        <article
          v-for="(item, index) in streamItems"
          :key="`${item.type}-${item.id}`"
          v-masonry-item
          class="feed-card"
          :class="[item.type, getCardLayout(index, item.type)]"
          role="link"
          tabindex="0"
          @click="openStreamItem(item)"
          @keydown.enter="openStreamItem(item)"
          @keydown.space.prevent="openStreamItem(item)"
        >
          <div class="feed-artwork">
            <img
              :src="item.cover"
              :alt="item.name"
              loading="lazy"
              decoding="async"
              @error="useFallbackCover"
            />
            <span class="type-label">{{ getTypeLabel(item.type) }}</span>
            <span v-if="item.type !== 'artist'" class="open-cue" aria-hidden="true">↗</span>
            <div v-if="item.type !== 'artist'" class="media-overlay">
              <h3>{{ item.name }}</h3>
              <span v-if="item.meta">{{ item.meta }}</span>
            </div>
          </div>
          <div v-if="item.type === 'artist'" class="feed-copy">
            <h3>{{ item.name }}</h3>
            <span v-if="item.meta">{{ item.meta }}</span>
          </div>
        </article>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import gsap from "gsap";
import { PlayArrowRound } from "@vicons/material";
import { getArtistList } from "@/api/artist";
import { getNewAlbum, getPersonalized } from "@/api/home";
import Banner from "@/components/Banner/index.vue";
import PaDailySongs from "@/components/Personalized/PaDailySongs.vue";
import PaLikeSongs from "@/components/Personalized/PaLikeSongs.vue";
import PaPersonalFm from "@/components/Personalized/PaPersonalFm.vue";
import PaRadar from "@/components/Personalized/PaRadar.vue";
import { musicStore, settingStore } from "@/store";
import type { SongData } from "@/store/musicTypes";
import { formatNumber, getSongTime } from "@/utils/timeTools";

type StreamItemType = "playlist" | "album" | "artist";

interface StreamItem {
  type: StreamItemType;
  id: number;
  cover: string;
  name: string;
  meta?: string;
}

const FALLBACK_COVER = "/images/pic/pic.jpg";

const setting = settingStore();
const music = musicStore();
const router = useRouter();
const { t, locale } = useI18n();
const playlistsData = ref<StreamItem[]>([]);
const albumsData = ref<StreamItem[]>([]);
const artistsData = ref<StreamItem[]>([]);
const newSongsData = ref<SongData[]>([]);
const isLoading = ref(true);
let masonryObserver: ResizeObserver | null = null;

const updateMasonrySpan = (element: HTMLElement) => {
  const rowHeight = 8;
  const rowGap = 8;
  const span = Math.ceil((element.getBoundingClientRect().height + rowGap) / (rowHeight + rowGap));
  element.style.gridRowEnd = `span ${span}`;
};

const vMasonryItem = {
  mounted(element: HTMLElement) {
    masonryObserver?.observe(element);
    requestAnimationFrame(() => updateMasonrySpan(element));
  },
  updated(element: HTMLElement) {
    requestAnimationFrame(() => updateMasonrySpan(element));
  },
  unmounted(element: HTMLElement) {
    masonryObserver?.unobserve(element);
  },
};

const greetingText = computed(() => {
  const hour = new Date().getHours();
  if (hour >= 5 && hour < 12) return t("home.greeting.morning");
  if (hour >= 12 && hour < 18) return t("home.greeting.afternoon");
  return t("home.greeting.evening");
});

const dateText = computed(() =>
  new Intl.DateTimeFormat(locale.value, {
    month: "long",
    day: "numeric",
    weekday: "long",
  }).format(new Date()),
);

const streamItems = computed<StreamItem[]>(() => {
  const items: StreamItem[] = [];
  const maxLength = Math.max(
    playlistsData.value.length,
    albumsData.value.length,
    artistsData.value.length,
  );

  for (let index = 0; index < maxLength; index += 1) {
    if (playlistsData.value[index]) items.push(playlistsData.value[index]);
    if (albumsData.value[index]) items.push(albumsData.value[index]);
    if (artistsData.value[index]) items.push(artistsData.value[index]);
  }
  return items;
});

const normalizeCover = (url?: string, size = 480) => {
  if (!url) return FALLBACK_COVER;
  return `${url.replace(/^http:/, "https:")}?param=${size}y${size}`;
};

const useFallbackCover = (event: Event) => {
  const image = event.currentTarget;
  if (image instanceof HTMLImageElement && !image.src.endsWith(FALLBACK_COVER)) {
    image.src = FALLBACK_COVER;
  }
};

const getStreamData = async () => {
  isLoading.value = true;
  const [playlists, albums, artists, newSongs] = await Promise.allSettled([
    getPersonalized(null, 12),
    getNewAlbum(),
    getArtistList(-1, -1, 8),
    getPersonalized("newsong", 10),
  ]);

  if (playlists.status === "fulfilled") {
    playlistsData.value = (playlists.value.result ?? []).map((item: any) => ({
      type: "playlist",
      id: item.id,
      cover: normalizeCover(item.picUrl, 720),
      name: item.name,
      meta: `▶ ${formatNumber(item.playCount)}`,
    }));
  }
  if (albums.status === "fulfilled") {
    albumsData.value = (albums.value.albums ?? []).slice(0, 12).map((item: any) => ({
      type: "album",
      id: item.id,
      cover: normalizeCover(item.picUrl, 720),
      name: item.name,
      meta: item.artist?.name,
    }));
  }
  if (artists.status === "fulfilled") {
    artistsData.value = (artists.value.artists ?? []).map((item: any) => ({
      type: "artist",
      id: item.id,
      cover: normalizeCover(item.img1v1Url, 320),
      name: item.name,
      meta: t("general.name.songSize", { size: item.musicSize }),
    }));
  }
  if (newSongs.status === "fulfilled") {
    newSongsData.value = (newSongs.value.result ?? []).map((item: any, index: number) => {
      const rawSong = item.song ?? item;
      const album = rawSong.album ?? rawSong.al ?? {};
      return {
        id: item.id ?? rawSong.id,
        name: item.name ?? rawSong.name,
        artist: rawSong.artists ?? rawSong.ar ?? [],
        album: {
          ...album,
          picUrl: album.picUrl ?? item.picUrl ?? FALLBACK_COVER,
        },
        alia: rawSong.alias ?? rawSong.alia ?? [],
        time: getSongTime(rawSong.duration ?? rawSong.dt ?? 0),
        fee: rawSong.fee ?? 0,
        mv: rawSong.mvid ?? rawSong.mv ?? null,
        num: index + 1,
      };
    });
  }
  isLoading.value = false;
};

const getArtistNames = (song: SongData) =>
  song.artist
    ?.map((artist) => artist.name)
    .filter(Boolean)
    .join(" / ") || "-";

const playHotTrack = (song: SongData) => {
  music.setPersonalFmMode(false);
  music.setPlaylists(newSongsData.value);
  music.addSongToPlaylists(song);
  music.setPlayState(true);
};

const getTypeLabel = (type: StreamItemType) => {
  if (type === "album") return t("general.name.album");
  if (type === "artist") return t("home.title.artists");
  return t("general.name.playlist");
};

const getCardLayout = (index: number, type: StreamItemType) => {
  if (type === "artist") return "is-compact";
  if (index === 0 || index % 9 === 0) return "is-wide";
  if (index % 5 === 2) return "is-tall";
  return "is-standard";
};

const openStreamItem = (item: StreamItem) => {
  if (item.type === "playlist") router.push(`/playlist?id=${item.id}&page=1`);
  else if (item.type === "album") router.push(`/album?id=${item.id}`);
  else router.push(`/artist?id=${item.id}`);
};

onMounted(() => {
  masonryObserver = new ResizeObserver((entries) => {
    entries.forEach((entry) => updateMasonrySpan(entry.target as HTMLElement));
  });
  if (typeof $setSiteTitle !== "undefined") $setSiteTitle(import.meta.env.VITE_SITE_TITLE);
  if (typeof $scrollToTop !== "undefined") $scrollToTop();
  void getStreamData();

  gsap.from(".home-section", {
    opacity: 0,
    y: 24,
    duration: 0.48,
    stagger: 0.1,
    ease: "power2.out",
  });
});

onBeforeUnmount(() => {
  masonryObserver?.disconnect();
  masonryObserver = null;
});
</script>

<style lang="scss" scoped>
.home {
  container: home / inline-size;
  display: flex;
  flex-direction: column;
  gap: 42px;
  padding: 4px 0 42px;
}

.home-header {
  .date {
    color: var(--n-text-color-3);
    font-size: 12px;
    font-weight: 650;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  h1 {
    margin: 8px 0 0;
    font-size: clamp(34px, 5vw, 54px);
    font-weight: 820;
    line-height: 1;
    letter-spacing: -0.045em;
  }
}

.banner-section {
  overflow: hidden;
  border-radius: var(--radius-panel);
}

.section-heading {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 20px;
  margin-bottom: 18px;

  h2 {
    margin: 4px 0 0;
    font-size: clamp(24px, 3vw, 32px);
    font-weight: 760;
    line-height: 1.1;
    letter-spacing: -0.035em;
  }
}

.section-kicker {
  color: var(--n-text-color-3);
  font-size: 10px;
  font-weight: 750;
  letter-spacing: 0.13em;
  text-transform: uppercase;
}

.featured-grid {
  display: grid;
  grid-template-areas:
    "daily fm"
    "radar like";
  grid-template-columns: minmax(0, 7fr) minmax(0, 5fr);
  grid-template-rows: minmax(410px, auto) 118px;
  gap: 16px;
  align-items: stretch;

  > * {
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }

  .daily-feature {
    grid-area: daily;
  }

  .fm-feature {
    grid-area: fm;
  }

  .radar-feature {
    grid-area: radar;
  }

  .like-feature {
    grid-area: like;
  }
}

.feed-heading {
  margin-bottom: 20px;
}

.feed-links {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 6px;

  button {
    padding: 7px 11px;
    color: var(--n-text-color-2);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    border: 0;
    border-radius: var(--radius-pill);
    background: color-mix(in srgb, var(--n-text-color) 6%, transparent);
    cursor: pointer;
    transition:
      color var(--duration-200) var(--ease-out),
      background-color var(--duration-200) var(--ease-out);

    &:hover,
    &:focus-visible {
      color: var(--n-text-color);
      background: color-mix(in srgb, var(--n-text-color) 11%, transparent);
    }
  }
}

.feed-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  grid-auto-flow: dense;
  grid-auto-rows: 8px;
  align-items: start;
  gap: 8px 16px;
}

.feed-card {
  display: block;
  grid-column: span 1;
  width: 100%;
  min-width: 0;
  cursor: pointer;
  outline: none;

  .feed-artwork {
    position: relative;
    isolation: isolate;
    overflow: hidden;
    aspect-ratio: 1;
    border-radius: var(--radius-panel);
    background: color-mix(in srgb, var(--n-text-color) 6%, transparent);
  }

  img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    transition: transform var(--duration-400) var(--ease-out);
  }

  .type-label,
  .open-cue {
    position: absolute;
    z-index: 1;
    color: #fff;
    background: rgba(12, 12, 12, 0.58);
    border: 1px solid rgba(255, 255, 255, 0.13);
    backdrop-filter: blur(10px);
  }

  .type-label {
    top: 10px;
    left: 10px;
    padding: 5px 8px;
    border-radius: var(--radius-pill);
    font-size: 10px;
    font-weight: 700;
  }

  .open-cue {
    right: 10px;
    bottom: 10px;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: var(--radius-pill);
    opacity: 0;
    transform: translateY(5px);
    transition:
      opacity var(--duration-200) var(--ease-out),
      transform var(--duration-200) var(--ease-out);
  }

  .feed-copy {
    padding: 10px 2px 0;

    h3 {
      display: -webkit-box;
      margin: 0;
      overflow: hidden;
      font-size: 14px;
      font-weight: 650;
      line-height: 1.4;
      -webkit-box-orient: vertical;
      -webkit-line-clamp: 2;
      line-clamp: 2;
    }

    span {
      display: block;
      margin-top: 3px;
      overflow: hidden;
      color: var(--n-text-color-3);
      font-size: 12px;
      white-space: nowrap;
      text-overflow: ellipsis;
    }
  }

  &.is-wide {
    grid-column: span 2;

    .feed-artwork {
      aspect-ratio: 16 / 9;
    }
  }

  &.is-tall .feed-artwork {
    aspect-ratio: 4 / 5;
  }

  &.is-compact {
    display: grid;
    grid-template-columns: 68px minmax(0, 1fr);
    align-items: center;
    gap: 12px;
    padding: 10px;
    box-sizing: border-box;
    border: 1px solid color-mix(in srgb, var(--n-text-color) 7%, transparent);
    border-radius: var(--radius-panel);
    background: color-mix(in srgb, var(--n-text-color) 4%, transparent);

    .feed-artwork {
      width: 68px;
      height: 68px;
      aspect-ratio: 1;
      border-radius: var(--radius-pill);
    }

    .feed-copy {
      min-width: 0;
      padding: 0;
    }

    .type-label {
      display: none;
    }
  }

  &.album,
  &.playlist {
    .feed-artwork {
      color: rgba(255, 255, 255, 0.94);
      border: 0;
      background: #18191c;
      box-shadow: none;
      filter: none;
      box-sizing: border-box;

      &::before {
        content: "";
        position: absolute;
        inset: 0;
        z-index: 3;
        pointer-events: none;
        border: 1px solid rgba(255, 255, 255, 0.34);
        border-radius: inherit;
        box-sizing: border-box;
        opacity: 0.72;
        mix-blend-mode: soft-light;
      }
    }

    .open-cue {
      top: 10px;
      right: 10px;
      bottom: auto;
    }

    &:hover .media-overlay h3,
    &:focus-visible .media-overlay h3 {
      color: #fff;
    }
  }

  &:hover,
  &:focus-visible {
    img {
      transform: scale(1.025);
    }

    .open-cue {
      opacity: 1;
      transform: translateY(0);
    }

    .feed-copy h3 {
      color: var(--main-color);
    }
  }
}

.media-overlay {
  position: absolute;
  right: 0;
  bottom: 0;
  left: 0;
  z-index: 1;
  min-width: 0;
  padding: 72px 14px 14px;
  color: #fff;
  background: linear-gradient(
    to bottom,
    transparent,
    rgba(8, 8, 9, 0.34) 30%,
    rgba(8, 8, 9, 0.88) 76%,
    rgba(8, 8, 9, 0.96)
  );

  h3 {
    display: -webkit-box;
    margin: 0;
    overflow: hidden;
    font-size: clamp(14px, 1.25vw, 18px);
    font-weight: 700;
    line-height: 1.28;
    letter-spacing: -0.02em;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }

  span {
    display: block;
    margin-top: 4px;
    overflow: hidden;
    color: rgba(255, 255, 255, 0.66);
    font-size: 12px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
}

.feed-skeleton {
  grid-row: span 32;
  width: 100%;
  height: 260px;
  border-radius: var(--radius-panel);

  &.is-wide {
    grid-column: span 2;
    grid-row: span 24;
    height: 190px;
  }
}

.hot-tracks-card {
  grid-column: span 1;
  min-width: 0;
  padding: 16px 12px 10px;
  box-sizing: border-box;
  border: 1px solid color-mix(in srgb, var(--n-text-color) 8%, transparent);
  border-radius: var(--radius-panel);
  background: color-mix(in srgb, var(--n-text-color) 4%, transparent);
}

.hot-tracks-card__header {
  padding: 2px 6px 12px;

  span {
    color: var(--n-text-color-3);
    font-size: 9px;
    font-weight: 750;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  h3 {
    margin: 4px 0 0;
    font-size: 20px;
    font-weight: 740;
    line-height: 1.2;
    letter-spacing: -0.025em;
  }

  p {
    margin: 4px 0 0;
    color: var(--n-text-color-3);
    font-size: 11px;
  }
}

.hot-tracks-card__list {
  display: flex;
  flex-direction: column;
}

.hot-track {
  display: grid;
  grid-template-columns: 22px 44px minmax(0, 1fr) 28px;
  align-items: center;
  gap: 9px;
  width: 100%;
  min-width: 0;
  padding: 7px 6px;
  color: inherit;
  font: inherit;
  text-align: left;
  border: 0;
  border-top: 1px solid color-mix(in srgb, var(--n-text-color) 7%, transparent);
  background: transparent;
  cursor: pointer;
  transition: background-color var(--duration-200) var(--ease-out);

  img {
    display: block;
    width: 44px;
    height: 44px;
    object-fit: cover;
    border-radius: 6px;
  }

  &:hover,
  &:focus-visible {
    background: color-mix(in srgb, var(--n-text-color) 6%, transparent);

    .hot-track__play,
    strong {
      color: var(--main-color);
    }
  }
}

.hot-track__index {
  color: var(--n-text-color-3);
  font-size: 10px;
  font-variant-numeric: tabular-nums;
}

.hot-track__copy {
  display: flex;
  flex-direction: column;
  min-width: 0;

  strong,
  small {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  strong {
    font-size: 13px;
    font-weight: 620;
    transition: color var(--duration-200) var(--ease-out);
  }

  small {
    margin-top: 2px;
    color: var(--n-text-color-3);
    font-size: 11px;
  }
}

.hot-track__play {
  color: var(--n-text-color-3);
  transition: color var(--duration-200) var(--ease-out);
}

@container home (max-width: 1100px) {
  .featured-grid {
    grid-template-rows: minmax(380px, auto) 118px;
  }

  .feed-grid {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }
}

@container home (max-width: 880px) {
  .home {
    gap: 34px;
  }

  .featured-grid {
    grid-template-areas:
      "daily"
      "fm"
      "radar"
      "like";
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto;

    .daily-feature,
    .fm-feature,
    .radar-feature,
    .like-feature {
      width: 100%;
    }

    .daily-feature {
      min-height: 320px;
    }

    .fm-feature {
      min-height: 168px;
    }

    .radar-feature,
    .like-feature {
      min-height: 96px;
    }
  }
}

@container home (max-width: 760px) {
  .section-heading {
    align-items: flex-start;
    flex-direction: column;
    gap: 12px;
  }

  .feed-heading {
    margin-bottom: 16px;
  }

  .feed-links {
    width: 100%;
    justify-content: flex-start;
    flex-wrap: nowrap;
    padding-bottom: 3px;
    overflow-x: auto;
    scrollbar-width: none;

    &::-webkit-scrollbar {
      display: none;
    }

    button {
      flex: none;
      min-height: 36px;
    }
  }

  .feed-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
    column-gap: 12px;
  }
}

@container home (max-width: 520px) {
  .home {
    gap: 28px;
    padding-bottom: 28px;
  }

  .home-header h1 {
    margin-top: 6px;
    font-size: clamp(30px, 10vw, 36px);
  }

  .section-heading {
    margin-bottom: 14px;

    h2 {
      font-size: 24px;
    }
  }

  .featured-grid {
    gap: 12px;

    .daily-feature {
      min-height: 280px;
    }
  }

  .feed-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
    column-gap: 10px;
  }

  .feed-card {
    &.is-compact {
      grid-column: span 2;
    }

    &.is-wide {
      grid-column: span 2;

      .feed-artwork {
        aspect-ratio: 4 / 3;
      }
    }

    .type-label {
      top: 7px;
      left: 7px;
    }

    .feed-copy h3 {
      font-size: 13px;
    }

    &.album .open-cue,
    &.playlist .open-cue {
      top: 7px;
      right: 7px;
    }
  }

  .media-overlay {
    padding: 54px 10px 10px;

    h3 {
      font-size: 14px;
    }

    span {
      font-size: 11px;
    }
  }

  .feed-skeleton.is-wide {
    grid-column: span 2;
  }

  .hot-tracks-card {
    grid-column: span 2;
    padding-inline: 10px;
  }

  .hot-track {
    grid-template-columns: 20px 44px minmax(0, 1fr) 28px;

    img {
      width: 44px;
      height: 44px;
    }
  }

  .hot-track__play {
    display: inline-flex;
  }
}

@container home (max-width: 360px) {
  .feed-grid {
    grid-template-columns: minmax(0, 1fr);
    column-gap: 0;
  }

  .feed-card.is-compact,
  .feed-card.is-wide,
  .feed-skeleton.is-wide,
  .hot-tracks-card {
    grid-column: span 1;
  }

  .hot-track {
    grid-template-columns: 18px 40px minmax(0, 1fr);

    img {
      width: 40px;
      height: 40px;
    }
  }

  .hot-track__play {
    display: none;
  }
}

@media (prefers-reduced-motion: reduce) {
  .feed-card img,
  .feed-card .open-cue {
    transition: none;
  }
}
</style>
