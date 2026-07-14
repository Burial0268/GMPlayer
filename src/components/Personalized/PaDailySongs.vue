<template>
  <div class="daily-card-host">
    <article
      class="daily-card"
      role="link"
      tabindex="0"
      @click="openDailySongs"
      @keydown.enter.self="openDailySongs"
      @keydown.space.self.prevent="openDailySongs"
    >
      <div class="artwork-wrap">
        <img
          class="artwork"
          :src="coverUrl"
          :alt="$t('home.modules.dailySongs.title')"
          loading="lazy"
          decoding="async"
          @error="useFallbackCover"
        />

        <div class="date-badge" aria-hidden="true">
          <span class="date-day">{{ displayDay }}</span>
          <span class="date-copy">
            <span>{{ displayMonth }}</span>
            <span>{{ $t("home.modules.dailySongs.label") }}</span>
          </span>
        </div>
      </div>

      <div class="content">
        <div class="copy">
          <span class="eyebrow">{{ $t("home.modules.dailySongs.subtitle") }}</span>
          <h3>{{ $t("home.modules.dailySongs.title") }}</h3>
        </div>

        <button
          class="play-button"
          type="button"
          :aria-label="$t('home.modules.dailySongs.play')"
          :title="$t('home.modules.dailySongs.play')"
          @click.stop="playThisSong"
        >
          <n-icon :component="PlayArrowRound" size="25" />
        </button>
      </div>
    </article>
  </div>
</template>

<script setup>
import { getDailySongs } from "@/api/home";
import { useRouter } from "vue-router";
import { musicStore, userStore } from "@/store";
import { getDailySongsDate } from "@/utils/timeTools";
import { PlayArrowRound } from "@vicons/material";
import { useI18n } from "vue-i18n";

const FALLBACK_COVER = "/images/pic/pic.jpg";

const music = musicStore();
const user = userStore();
const router = useRouter();
const { locale, t } = useI18n();
const playStartIndex = ref(0);

const displayDay = computed(() => Number(getDailySongsDate().split("-")[2]));
const displayMonth = computed(() =>
  new Intl.DateTimeFormat(locale.value, { month: "short" }).format(new Date()).toUpperCase(),
);

const normalizeCover = (url) => {
  if (typeof url !== "string" || !url) return FALLBACK_COVER;
  return `${url.replace(/^http:/, "https:")}?param=1024y1024`;
};

const coverUrl = computed(() =>
  normalizeCover(music.getDailySongs[playStartIndex.value]?.album?.picUrl),
);

const openDailySongs = () => router.push("/dailySongs");

const resetPlayStartIndex = () => {
  playStartIndex.value = music.getDailySongs.length
    ? Math.floor(Math.random() * music.getDailySongs.length)
    : 0;
};

const useFallbackCover = (event) => {
  const image = event.currentTarget;
  if (image instanceof HTMLImageElement && !image.src.endsWith(FALLBACK_COVER)) {
    image.src = FALLBACK_COVER;
  }
};

const getDailySongsData = () => {
  resetPlayStartIndex();
  const dailySongsDate = getDailySongsDate();
  if (
    user.userLogin &&
    (music.getDailySongs.length === 0 || music.getDailySongsDate !== dailySongsDate)
  ) {
    getDailySongs().then((res) => {
      if (res.data.dailySongs) {
        music.setDailySongs(res.data.dailySongs, dailySongsDate);
        resetPlayStartIndex();
      } else {
        $message.error(t("home.modules.dailySongs.fetchFailed"));
      }
    });
  }
};

const playThisSong = () => {
  if (!user.userLogin) {
    $message.error(t("general.message.needLogin"));
    return;
  }
  if (music.getDailySongs.length === 0) {
    $message.error(t("home.modules.dailySongs.fetchRetry"));
    return;
  }

  const songId = music.getPlaySongData?.id;
  const isHas = music.getDailySongs.findIndex((song) => song.id === songId);
  music.setPersonalFmMode(false);
  music.setPlayState(true);
  if (isHas === -1) {
    music.setPlaylists(music.getDailySongs);
    music.addSongToPlaylists(music.getDailySongs[playStartIndex.value]);
  }
};

onMounted(getDailySongsData);
</script>

<style lang="scss" scoped>
.daily-card-host {
  container-name: daily-card;
  container-type: inline-size;
  width: 100%;
  height: 100%;
  min-width: 0;
}

.daily-card {
  position: relative;
  isolation: isolate;
  width: 100%;
  height: 100%;
  min-height: 300px;
  overflow: hidden;
  box-sizing: border-box;
  color: rgba(255, 255, 255, 0.94);
  border: 0;
  border-radius: var(--radius-panel);
  background: #18191c;
  box-shadow: none;
  cursor: pointer;

  &::after {
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

.artwork-wrap {
  position: absolute;
  inset: 0;
  overflow: hidden;
  border-radius: inherit;
  background: rgba(255, 255, 255, 0.05);
}

.artwork {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
  transition: transform var(--duration-400) var(--ease-out);
}

.date-badge {
  position: absolute;
  top: 12px;
  left: 12px;
  z-index: 2;
  display: flex;
  align-items: center;
  gap: 8px;
  max-width: calc(100% - 24px);
  padding: 7px 10px;
  color: rgba(255, 255, 255, 0.94);
  background: rgba(14, 14, 15, 0.72);
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: var(--radius-pill);
  -webkit-backdrop-filter: blur(12px);
  backdrop-filter: blur(12px);
}

.date-day {
  font-size: 22px;
  font-weight: 800;
  line-height: 1;
  letter-spacing: -0.05em;
}

.date-copy {
  display: flex;
  flex-direction: column;
  overflow: hidden;
  color: rgba(255, 255, 255, 0.68);
  font-size: 9px;
  font-weight: 700;
  line-height: 1.15;
  letter-spacing: 0.07em;
  text-transform: uppercase;
  white-space: nowrap;
}

.content {
  position: absolute;
  right: 0;
  bottom: 0;
  left: 0;
  z-index: 2;
  display: flex;
  align-items: flex-end;
  gap: 14px;
  min-width: 0;
  padding: 76px 18px 18px;
  background: linear-gradient(
    to bottom,
    transparent,
    rgba(8, 8, 9, 0.38) 28%,
    rgba(8, 8, 9, 0.88) 76%,
    rgba(8, 8, 9, 0.96)
  );
  border-radius: 0 0 var(--radius-panel) var(--radius-panel);
}

.copy {
  flex: 1;
  min-width: 0;
}

.eyebrow {
  display: block;
  overflow: hidden;
  color: rgba(255, 255, 255, 0.58);
  font-size: 11px;
  font-weight: 650;
  line-height: 1.35;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.copy h3 {
  margin: 3px 0 0;
  overflow: hidden;
  color: inherit;
  font-size: 19px;
  font-weight: 750;
  line-height: 1.2;
  letter-spacing: -0.02em;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.play-button {
  display: grid;
  flex: none;
  place-items: center;
  width: 50px;
  height: 50px;
  padding: 0;
  color: #111;
  background: rgba(255, 255, 255, 0.74);
  border: 1px solid rgba(255, 255, 255, 0.46);
  border-radius: var(--radius-pill);
  box-shadow:
    0 8px 20px rgb(0 0 0 / 16%),
    inset 0 1px 0 rgb(255 255 255 / 48%);
  -webkit-backdrop-filter: blur(18px) saturate(160%);
  backdrop-filter: blur(18px) saturate(160%);
  cursor: pointer;
  transition:
    background-color var(--duration-200) var(--ease-out),
    border-color var(--duration-200) var(--ease-out),
    transform var(--duration-200) var(--ease-out);
}

.daily-card:focus-visible,
.play-button:focus-visible {
  outline: 2px solid currentColor;
  outline-offset: 3px;
}

.daily-card:hover {
  .artwork {
    transform: scale(1.035);
  }

  .play-button {
    transform: translateY(-2px);
  }
}

.play-button:hover {
  background: rgba(255, 255, 255, 0.9);
  border-color: rgba(255, 255, 255, 0.68);
}

.play-button:active {
  transform: scale(0.94);
}

@container daily-card (max-width: 560px) {
  .daily-card {
    min-height: clamp(220px, 72cqi, 340px);
  }

  .content {
    padding: 64px 14px 14px;
  }
}

@container daily-card (max-width: 340px) {
  .daily-card {
    min-height: 270px;
  }

  .date-badge {
    top: 8px;
    left: 8px;
    padding: 7px 9px;
  }

  .content {
    gap: 10px;
  }

  .copy h3 {
    font-size: 17px;
  }

  .play-button {
    width: 46px;
    height: 46px;
  }
}

@container daily-card (max-width: 280px) {
  .date-copy span:last-child {
    display: none;
  }

  .play-button {
    width: 44px;
    height: 44px;
  }
}

@media (hover: none) {
  .daily-card:hover .artwork,
  .daily-card:hover .play-button {
    transform: none;
  }
}

@media (prefers-reduced-motion: reduce) {
  .artwork,
  .play-button {
    transition: none;
  }
}
</style>
