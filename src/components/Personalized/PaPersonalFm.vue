<template>
  <div class="personal-fm-host">
    <Transition mode="out-in">
      <article v-if="fmData?.id" class="personal-fm">
        <div class="artwork-wrap">
          <img
            v-if="coverUrl && !coverFailed"
            class="artwork"
            :src="coverUrl"
            :alt="fmData.name || $t('home.modules.personalFm.title')"
            loading="lazy"
            decoding="async"
            @error="coverFailed = true"
          />
          <div v-else class="artwork artwork-fallback" aria-hidden="true">
            <n-icon :component="RadioFilled" />
          </div>

          <div class="station-label">
            <n-icon :component="RadioFilled" />
            <span>{{ $t("home.modules.personalFm.title") }}</span>
          </div>
        </div>

        <div class="content">
          <div class="track-info">
            <button
              class="track-name text-hidden"
              type="button"
              :aria-label="fmData.name"
              @click.stop="router.push(`/song?id=${fmData.id}`)"
            >
              {{ fmData.name }}
            </button>
            <AllArtists class="artists text-hidden" :artistsData="fmData.artist || []" />
            <span v-if="!user.userLogin" class="tip">
              {{ $t("home.modules.personalFm.subtitle") }}
            </span>
          </div>

          <div class="controls" @click.stop>
            <button
              class="control-button secondary"
              type="button"
              :aria-label="$t('home.modules.personalFm.dislike')"
              :title="$t('home.modules.personalFm.dislike')"
              @click.stop="music.setFmDislike(fmData.id)"
            >
              <n-icon :component="ThumbDownRound" />
            </button>
            <button
              class="control-button primary"
              type="button"
              :aria-label="
                isFmPlaying
                  ? $t('home.modules.personalFm.pause')
                  : $t('home.modules.personalFm.play')
              "
              :title="
                isFmPlaying
                  ? $t('home.modules.personalFm.pause')
                  : $t('home.modules.personalFm.play')
              "
              @click.stop="fmPlayOrPause"
            >
              <n-icon :component="isFmPlaying ? PauseRound : PlayArrowRound" />
            </button>
            <button
              class="control-button secondary"
              type="button"
              :aria-label="$t('home.modules.personalFm.next')"
              :title="$t('home.modules.personalFm.next')"
              @click.stop="fmNext"
            >
              <n-icon :component="SkipNextRound" />
            </button>
          </div>
        </div>
      </article>
      <n-skeleton v-else class="personal-fm skeleton" />
    </Transition>
  </div>
</template>

<script setup>
import { computed, ref, watch } from "vue";
import { useRouter } from "vue-router";
import {
  PauseRound,
  PlayArrowRound,
  RadioFilled,
  SkipNextRound,
  ThumbDownRound,
} from "@vicons/material";
import AllArtists from "@/components/DataList/AllArtists.vue";
import { musicStore, userStore } from "@/store";

const music = musicStore();
const user = userStore();
const router = useRouter();
const coverFailed = ref(false);

const fmData = computed(() => music.getPersonalFmData);
const coverUrl = computed(() => {
  const picUrl = fmData.value?.album?.picUrl;
  if (typeof picUrl !== "string" || !picUrl) return "";
  return `${picUrl.replace(/^http:/, "https:")}?param=1024y1024`;
});
const isFmPlaying = computed(() => music.getPersonalFmMode && music.getPlayState);

watch(coverUrl, () => {
  coverFailed.value = false;
});

const fmPlayOrPause = () => {
  if (music.getPersonalFmMode) {
    music.setPlayState(!music.getPlayState);
  } else {
    music.setPersonalFmMode(true);
    music.setPlayState(true);
  }
};

const fmNext = () => {
  music.setPersonalFmMode(true);
  music.setPlaySongIndex("next");
};

onMounted(() => {
  if (!music.getPersonalFmData?.id) music.setPersonalFmData();
});
</script>

<style lang="scss" scoped>
.personal-fm-host {
  container-name: personal-fm;
  container-type: inline-size;
  width: 100%;
  height: 100%;
  min-width: 0;
}

.personal-fm {
  position: relative;
  height: 100%;
  min-height: 300px;
  overflow: hidden;
  color: rgba(255, 255, 255, 0.94);
  background: #18191c;
  border: 1px solid rgba(255, 255, 255, 0.07);
  border-radius: var(--radius-panel);
  box-shadow: none;
  filter: none;
  box-sizing: border-box;
}

.artwork-wrap {
  position: absolute;
  inset: 0;
  overflow: hidden;
  background: rgba(255, 255, 255, 0.05);
}

.artwork {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.artwork-fallback {
  display: grid;
  place-items: center;
  color: rgba(255, 255, 255, 0.5);
  font-size: clamp(42px, 8vw, 72px);
  background: linear-gradient(145deg, #34363a, #202124);
}

.station-label {
  position: absolute;
  top: 12px;
  left: 12px;
  display: flex;
  align-items: center;
  gap: 6px;
  max-width: calc(100% - 24px);
  padding: 7px 10px;
  overflow: hidden;
  font-size: 12px;
  font-weight: 650;
  white-space: nowrap;
  text-overflow: ellipsis;
  z-index: 2;
  background: rgba(14, 14, 15, 0.72);
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 999px;
  -webkit-backdrop-filter: blur(12px);
  backdrop-filter: blur(12px);
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
  padding: 72px 18px 18px;
  background: linear-gradient(
    to bottom,
    transparent,
    rgba(8, 8, 9, 0.38) 28%,
    rgba(8, 8, 9, 0.88) 76%,
    rgba(8, 8, 9, 0.96)
  );
}

.track-info {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}

.track-name {
  width: fit-content;
  max-width: 100%;
  padding: 0;
  color: inherit;
  font: inherit;
  font-size: 18px;
  font-weight: 700;
  line-height: 1.3;
  text-align: left;
  background: none;
  border: 0;
  cursor: pointer;
}

.track-name:hover {
  text-decoration: underline;
  text-underline-offset: 3px;
}

.track-name:focus-visible,
.control-button:focus-visible {
  outline: 2px solid currentColor;
  outline-offset: 3px;
}

.artists {
  flex-wrap: nowrap;
  min-width: 0;
  font-size: 13px;

  :deep(.artist) {
    display: inline-block;
    white-space: nowrap;

    .name {
      color: rgba(255, 255, 255, 0.64);

      &:hover {
        color: rgba(255, 255, 255, 0.92);
      }
    }
  }
}

.tip {
  margin-top: 2px;
  overflow: hidden;
  color: rgba(255, 255, 255, 0.42);
  font-size: 11px;
  line-height: 1.35;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.controls {
  display: flex;
  flex: none;
  align-items: center;
  gap: 7px;
}

.control-button {
  display: grid;
  flex: none;
  place-items: center;
  padding: 0;
  color: inherit;
  border: 0;
  border-radius: 999px;
  cursor: pointer;
  transition:
    color 160ms ease,
    background-color 160ms ease,
    border-color 160ms ease,
    transform 160ms ease;
}

.control-button:hover {
  transform: translateY(-1px);
}

.control-button:active {
  transform: scale(0.94);
}

.control-button.secondary {
  width: 44px;
  height: 44px;
  color: rgba(255, 255, 255, 0.84);
  font-size: 20px;
  background: rgba(24, 24, 24, 0.46);
  border: 1px solid rgba(255, 255, 255, 0.14);
  box-shadow:
    0 6px 16px rgb(0 0 0 / 14%),
    inset 0 1px 0 rgb(255 255 255 / 12%);
  -webkit-backdrop-filter: blur(16px) saturate(150%);
  backdrop-filter: blur(16px) saturate(150%);
}

.control-button.secondary:hover {
  color: #fff;
  background: rgba(36, 36, 36, 0.62);
  border-color: rgba(255, 255, 255, 0.22);
}

.control-button.primary {
  width: 50px;
  height: 50px;
  color: #111;
  font-size: 28px;
  background: rgba(255, 255, 255, 0.74);
  border: 1px solid rgba(255, 255, 255, 0.46);
  box-shadow:
    0 8px 20px rgb(0 0 0 / 16%),
    inset 0 1px 0 rgb(255 255 255 / 48%);
  -webkit-backdrop-filter: blur(18px) saturate(160%);
  backdrop-filter: blur(18px) saturate(160%);
}

.control-button.primary:hover {
  background: rgba(255, 255, 255, 0.9);
  border-color: rgba(255, 255, 255, 0.68);
}

.skeleton {
  height: 100%;
  border-radius: var(--radius-panel);
}

@container personal-fm (max-width: 560px) {
  .personal-fm {
    min-height: clamp(220px, 72cqi, 340px);
  }

  .content {
    padding: 64px 14px 14px;
  }
}

@container personal-fm (max-width: 440px) {
  .content {
    flex-direction: column;
    align-items: stretch;
    gap: 10px;
    padding-top: 76px;
  }

  .controls {
    justify-content: flex-start;
  }
}

@container personal-fm (max-width: 340px) {
  .personal-fm {
    min-height: 270px;
  }

  .station-label {
    top: 8px;
    left: 8px;
    padding: 7px;
  }

  .track-name {
    font-size: 16px;
  }

  .controls {
    gap: 6px;
  }

  .tip {
    display: none;
  }
}

@container personal-fm (max-width: 280px) {
  .station-label span {
    display: none;
  }
}

@media (hover: none) {
  .control-button:hover {
    transform: none;
  }
}

@media (prefers-reduced-motion: reduce) {
  .control-button {
    transition: none;
  }
}
</style>
