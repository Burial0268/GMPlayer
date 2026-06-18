<template>
  <LayoutGroup id="mobile-player-layout">
    <Transition name="show">
      <n-card
        v-show="music.getPlaylists[0] && music.showPlayBar"
        class="player"
        data-mobile-player-bg
        content-style="padding: 0"
        @click.stop="handleMiniPlayerClick"
        @touchstart.passive="handleMiniTouchStart"
        @touchmove.passive="handleMiniTouchMove"
        @touchend.passive="handleMiniTouchEnd"
        @touchcancel="resetMiniTouch"
      >
        <div class="slider">
          <span>{{ music.getPlaySongTime.songTimePlayed }}</span>
          <vue-slider
            v-model="music.getPlaySongTime.barMoveDistance"
            @drag-start="sliderDragStart"
            @dragging="sliderDragging"
            @drag-end="sliderDragEnd"
            @change="songTimeSliderUpdate"
            @click.stop
            :tooltip="'active'"
            :lazy="true"
            :use-keyboard="false"
          >
            <template v-slot:tooltip>
              <div class="slider-tooltip">
                {{
                  getSongPlayingTime(
                    (music.getPlaySongTime.duration / 100) * music.getPlaySongTime.barMoveDistance,
                  )
                }}
              </div>
            </template>
          </vue-slider>
          <span>{{ music.getPlaySongTime.songTimeDuration }}</span>
        </div>
        <div class="all">
          <div class="data">
            <div class="pic" data-mobile-player-artwork @click.stop="handleMiniArtworkClick">
              <img
                :src="
                  music.getPlaySongData
                    ? music.getPlaySongData.album.picUrl.replace(/^http:/, 'https:') +
                      '?param=50y50'
                    : '/images/pic/default.png'
                "
                alt="pic"
              />
            </div>
            <div class="name">
              <div
                class="song text-hidden"
                @click.stop="router.push(`/song?id=${music.getPlaySongData.id}`)"
              >
                {{ music.getPlaySongData ? music.getPlaySongData.name : $t("other.noSong") }}
              </div>
              <!-- 显示歌手或歌词 -->
              <div class="artisrOrLrc" v-if="music.getPlaySongData">
                <Transition name="fade" mode="out-in">
                  <template v-if="setting.bottomLyricShow">
                    <Transition name="mini-lyric" mode="out-in">
                      <n-text
                        v-if="miniLyricLine"
                        :key="miniLyricLine.key"
                        :class="['lrc', { 'is-marquee': miniLrcOverflow }]"
                        :depth="3"
                      >
                        <span ref="lrcMeasureRef" class="lrc-measure-content">
                          <span
                            v-for="item in miniLyricLine.words"
                            :key="item.key"
                            class="lrc-word"
                          >
                            {{ item.content }}
                          </span>
                        </span>
                        <OverflowMarquee
                          v-if="miniLrcOverflow"
                          class="mini-lrc-marquee"
                          :speed="34"
                        >
                          <span class="lrc-marquee-content">
                            <span
                              v-for="item in miniLyricLine.words"
                              :key="item.key"
                              class="lrc-word"
                            >
                              {{ item.content }}
                            </span>
                          </span>
                        </OverflowMarquee>
                      </n-text>
                      <AllArtists
                        v-else
                        key="artists"
                        class="text-hidden"
                        :artistsData="music.getPlaySongData.artist"
                      />
                    </Transition>
                  </template>
                  <template v-else>
                    <AllArtists class="text-hidden" :artistsData="music.getPlaySongData.artist" />
                  </template>
                </Transition>
              </div>
            </div>
          </div>
          <div class="control">
            <n-icon
              v-if="!music.getPersonalFmMode"
              class="prev"
              size="30"
              :component="SkipPreviousRound"
              @click.stop="music.setPlaySongIndex('prev')"
            />
            <n-icon
              v-else
              class="dislike"
              size="20"
              :component="ThumbDownRound"
              @click="music.setFmDislike(music.getPersonalFmData.id)"
            />
            <div
              class="play-state"
              @click.stop="music.getLoadingState ? null : music.setPlayState(!music.getPlayState)"
            >
              <AnimatePresence mode="wait">
                <Motion
                  v-if="music.getLoadingState"
                  key="loading"
                  :initial="{ opacity: 0, scale: 0.8 }"
                  :animate="{ opacity: 1, scale: 1 }"
                  :exit="{ opacity: 0, scale: 0.8 }"
                  :transition="{ duration: 0.2 }"
                  class="play-state-inner"
                >
                  <n-spin :size="28" stroke="var(--main-color)" />
                </Motion>
                <Motion
                  v-else
                  :key="music.getPlayState ? 'pause' : 'play'"
                  :initial="{ opacity: 0, scale: 0.8 }"
                  :animate="{ opacity: 1, scale: 1 }"
                  :exit="{ opacity: 0, scale: 0.8 }"
                  :transition="{ duration: 0.2 }"
                  class="play-state-inner"
                >
                  <n-icon
                    size="46"
                    :component="music.getPlayState ? PauseCircleFilled : PlayCircleFilled"
                  />
                </Motion>
              </AnimatePresence>
            </div>
            <n-icon
              class="next"
              size="30"
              :component="SkipNextRound"
              @click.stop="music.setPlaySongIndex('next')"
            />
          </div>
          <div :class="music.getPersonalFmMode ? 'menu fm' : 'menu'">
            <n-popover v-if="music.getPlaySongData" trigger="hover" :keep-alive-on-hover="false">
              <template #trigger>
                <div class="like">
                  <n-icon
                    class="like-icon"
                    size="24"
                    :component="
                      music.getSongIsLike(music.getPlaySongData.id)
                        ? FavoriteRound
                        : FavoriteBorderRound
                    "
                    @click.stop="
                      music.getSongIsLike(music.getPlaySongData.id)
                        ? music.changeLikeList(music.getPlaySongData.id, false)
                        : music.changeLikeList(music.getPlaySongData.id, true)
                    "
                  />
                </div>
              </template>
              {{
                music.getSongIsLike(music.getPlaySongData.id)
                  ? $t("menu.cancelCollection")
                  : $t("menu.collection")
              }}
            </n-popover>
            <n-popover trigger="hover" :keep-alive-on-hover="false">
              <template #trigger>
                <div class="add-playlist">
                  <n-icon
                    class="add-icon"
                    size="30"
                    :component="PlaylistAddRound"
                    @click.stop="addPlayListRef.openAddToPlaylist(music.getPlaySongData.id)"
                  />
                </div>
              </template>
              {{ $t("menu.add") }}
            </n-popover>
            <n-dropdown
              trigger="hover"
              :options="patternOptions"
              :show-arrow="true"
              @select="patternClick"
            >
              <div class="pattern">
                <n-icon
                  :component="
                    persistData.playSongMode === 'normal'
                      ? PlayCycle
                      : persistData.playSongMode === 'random'
                        ? ShuffleOne
                        : PlayOnce
                  "
                  @click.stop="music.setPlaySongMode()"
                />
              </div>
            </n-dropdown>
            <n-popover trigger="hover" :keep-alive-on-hover="false">
              <template #trigger>
                <div
                  :class="music.showPlayList ? 'playlist open' : 'playlist'"
                  @pointerdown.stop="armPlaylistToggle"
                  @pointercancel="clearPlaylistToggleIntent"
                  @click.stop="togglePlaylist"
                >
                  <n-icon size="30" :component="PlaylistPlayRound" />
                </div>
              </template>
              {{ $t("general.name.playlists") }}
            </n-popover>
            <!-- 一起听歌 -->
            <ListenTogetherStatus @click="showListenTogetherModal = true" />
            <div class="volume">
              <n-popover trigger="hover" placement="top-start" :keep-alive-on-hover="false">
                <template #trigger>
                  <n-icon
                    size="28"
                    :component="
                      persistData.playVolume == 0
                        ? VolumeOffRound
                        : persistData.playVolume < 0.4
                          ? VolumeMuteRound
                          : persistData.playVolume < 0.7
                            ? VolumeDownRound
                            : VolumeUpRound
                    "
                    @click.stop="volumeMute"
                  />
                </template>
                {{
                  persistData.playVolume > 0 ? $t("general.name.mute") : $t("general.name.unmute")
                }}
              </n-popover>
              <n-slider
                class="volmePg"
                v-model:value="persistData.playVolume"
                :tooltip="false"
                :min="0"
                :max="1"
                :step="0.01"
                @click.stop
              />
            </div>
          </div>
        </div>
      </n-card>
    </Transition>
    <!-- 播放列表 -->
    <PlayListDrawer ref="PlayListDrawerRef" />
    <!-- 添加到歌单 -->
    <AddPlaylist ref="addPlayListRef" />
    <!-- 一起听歌 -->
    <ListenTogetherModal v-model:show="showListenTogetherModal" />
    <!-- 播放器 -->
    <BigPlayer ref="bigPlayerRef" />
  </LayoutGroup>
</template>

<script setup>
import { getMusicDetail } from "@/api/song";
import { resolveSongUrl } from "@/utils/AudioContext/resolveSongUrl";
import { Motion, AnimatePresence, LayoutGroup } from "motion-v";
import { NIcon } from "naive-ui";
import {
  PlayCircleFilled,
  PauseCircleFilled,
  SkipNextRound,
  SkipPreviousRound,
  PlaylistPlayRound,
  VolumeOffRound,
  VolumeMuteRound,
  VolumeDownRound,
  VolumeUpRound,
  ThumbDownRound,
  FavoriteBorderRound,
  FavoriteRound,
  PlaylistAddRound,
} from "@vicons/material";
import { PlayCycle, PlayOnce, ShuffleOne } from "@icon-park/vue-next";
import { storeToRefs } from "pinia";
import { musicStore, settingStore, siteStore, listenTogetherStore } from "@/store";
import {
  createSound,
  setVolume,
  setSeek,
  fadePlayOrPause,
  getAutoMixEngine,
  getAudioPreloader,
} from "@/utils/AudioContext";
import { getSongPlayingTime } from "@/utils/timeTools";
import { useRouter } from "vue-router";
import { debounce, throttle } from "throttle-debounce";
import { useI18n } from "vue-i18n";
import { isTauri } from "@/utils/tauri";
import { NativeRustSound, isAudioBackendRuntimeAvailable } from "@/utils/tauri/NativeRustSound";
import { windowManager } from "@/utils/tauri/windowManager";
import { useNativeMediaControls } from "@/composables/useNativeMediaControls";
import VueSlider from "vue-slider-component";
import AddPlaylist from "@/components/DataModal/AddPlaylist.vue";
import PlayListDrawer from "@/components/DataModal/PlayListDrawer.vue";
import ListenTogetherModal from "@/components/DataModal/ListenTogetherModal.vue";
import ListenTogetherStatus from "./ListenTogetherStatus.vue";
import AllArtists from "@/components/DataList/AllArtists.vue";
import OverflowMarquee from "@/components/Common/OverflowMarquee.vue";
import BigPlayer from "./BigPlayer/index.vue";
import "vue-slider-component/theme/default.css";
import { watch, toRaw } from "vue";
import { parseLyricData as parseLyric } from "@/utils/LyricsProcessor";
import { preprocessLyrics, getProcessedLyrics } from "@/utils/LyricsProcessor";
import { lyricFetcher } from "@/utils/lyricFetcher";

const { t } = useI18n();
const router = useRouter();
const setting = settingStore();
const music = musicStore();
const site = siteStore();
const listenTogether = listenTogetherStore();
const { persistData } = storeToRefs(music);
useNativeMediaControls();
const addPlayListRef = ref(null);
const PlayListDrawerRef = ref(null);
const lrcMeasureRef = ref(null);
const miniLrcOverflow = ref(false);
const bigPlayerRef = ref(null);

let miniTouchState = null;
let suppressMiniClick = false;

const clamp = (value, min, max) => Math.min(max, Math.max(min, value));
const PLAYER_BRIDGE_TARGETS = ["mini-player", "desktop-lyrics", "taskbar-lyric"];
let playlistToggleIntent = null;
let playlistToggleIntentTimer = null;

const clearPlaylistToggleIntent = () => {
  playlistToggleIntent = null;
  if (playlistToggleIntentTimer !== null) {
    window.clearTimeout(playlistToggleIntentTimer);
    playlistToggleIntentTimer = null;
  }
};

const armPlaylistToggle = () => {
  clearPlaylistToggleIntent();
  playlistToggleIntent = !music.showPlayList;
  playlistToggleIntentTimer = window.setTimeout(clearPlaylistToggleIntent, 800);
};

const togglePlaylist = () => {
  const nextShowState = playlistToggleIntent ?? !music.showPlayList;
  clearPlaylistToggleIntent();
  music.showPlayList = nextShowState;
};

function emitPlayerBridgeEvent(eventName, payload, targetLabel) {
  const tauriEvent = window.__TAURI__?.event;
  if (!tauriEvent) return;

  tauriEvent.emit(eventName, payload).catch(() => {});

  const targets = targetLabel ? [targetLabel] : PLAYER_BRIDGE_TARGETS;
  for (const label of targets) {
    tauriEvent.emitTo(label, eventName, payload).catch(() => {});
  }
}

const getMobilePlayerTransitionDistance = () =>
  Math.min(760, Math.max(460, window.innerHeight * 0.72 || 560));
const MINI_OPEN_FLING_VELOCITY = -0.42;

const miniLyricLine = computed(() => {
  const lyric = music.getPlaySongLyric;
  const index = music.getPlaySongLyricIndex;
  if (!lyric?.lrc?.length || index === -1) return null;

  if (setting.showYrc && lyric.hasYrc && lyric.yrc?.[index]?.content?.length) {
    return {
      key: `yrc-${index}`,
      words: lyric.yrc[index].content.map((item, wordIndex) => ({
        key: item.time ?? wordIndex,
        content: item.content,
      })),
    };
  }

  const content = lyric.lrc?.[index]?.content;
  if (!content) return null;

  return {
    key: `lrc-${index}`,
    words: [{ key: index, content }],
  };
});

const updateMiniLrcOverflow = () => {
  const textEl = lrcMeasureRef.value;
  const containerEl = textEl?.parentElement;
  if (!textEl || !containerEl) {
    miniLrcOverflow.value = false;
    return;
  }
  miniLrcOverflow.value = textEl.scrollWidth > containerEl.clientWidth + 1;
};

const getMiniSharedFrames = () => {
  const artworkEl = document.querySelector("[data-mobile-player-artwork]");
  const backgroundEl = document.querySelector("[data-mobile-player-bg]");
  return {
    artwork: artworkEl?.getBoundingClientRect(),
    background: backgroundEl?.getBoundingClientRect(),
  };
};

const openBigPlayerFromMini = () => {
  if (music.showBigPlayer) return;
  const frames = getMiniSharedFrames();
  const result = bigPlayerRef.value?.openMobileFromMini?.(frames);
  if (result && typeof result.then === "function") return;
  music.setBigPlayerState(true);
};

const openMiniPlayer = () => {
  if (setting.bottomClick) openBigPlayerFromMini();
};

const handleMiniPlayerClick = () => {
  if (suppressMiniClick) {
    suppressMiniClick = false;
    return;
  }
  openMiniPlayer();
};

const handleMiniArtworkClick = () => {
  if (suppressMiniClick) {
    suppressMiniClick = false;
    return;
  }
  openBigPlayerFromMini();
};

const resetMiniTouch = () => {
  miniTouchState = null;
};

const releaseMiniClickSuppression = () => {
  window.setTimeout(() => {
    suppressMiniClick = false;
  }, 240);
};

const handleMiniTouchStart = (event) => {
  if (music.showBigPlayer || !music.getPlaylists[0] || !music.showPlayBar) return;
  const touch = event.changedTouches?.[0];
  if (!touch) return;
  miniTouchState = {
    x: touch.clientX,
    y: touch.clientY,
    lastY: touch.clientY,
    lastTime: performance.now(),
    velocityY: 0,
    dragging: false,
  };
};

const handleMiniTouchMove = (event) => {
  const start = miniTouchState;
  const touch = event.changedTouches?.[0];
  if (!start || !touch) return;

  const deltaX = touch.clientX - start.x;
  const deltaY = touch.clientY - start.y;
  const now = performance.now();
  const elapsed = Math.max(1, now - start.lastTime);
  start.velocityY = (touch.clientY - start.lastY) / elapsed;
  start.lastY = touch.clientY;
  start.lastTime = now;
  if (!start.dragging) {
    if (Math.abs(deltaY) < 8) return;
    if (deltaY >= 0 || Math.abs(deltaY) < Math.abs(deltaX) * 1.15) {
      resetMiniTouch();
      return;
    }
    start.dragging = true;
    suppressMiniClick = true;
    const frames = getMiniSharedFrames();
    bigPlayerRef.value?.beginMobileInteractiveOpen(frames);
  }

  const progress = clamp(-deltaY / getMobilePlayerTransitionDistance(), 0, 1);
  bigPlayerRef.value?.updateMobileInteractiveProgress(progress);
};

const handleMiniTouchEnd = () => {
  const start = miniTouchState;
  if (start?.dragging) {
    const forceOpen = start.velocityY < MINI_OPEN_FLING_VELOCITY;
    bigPlayerRef.value?.finishMobileInteractiveOpen(forceOpen ? true : undefined);
    releaseMiniClickSuppression();
  }
  resetMiniTouch();
};

// 一起听歌模态框
const showListenTogetherModal = ref(false);

// 音频标签
const player = ref(null);

// Async generation tracker — each getPlaySongData call increments this.
// Stale async callbacks (URL fetches, availability checks) compare their captured
// generation against the current value and bail out if a newer request superseded them.
let _songLoadGeneration = 0;

// 获取歌曲播放数据
const getPlaySongData = async (data, level = setting.songLevel) => {
  const generation = ++_songLoadGeneration;
  try {
    if (!data || !data.id) {
      console.error("[Player] getPlaySongData called with invalid data:", data);
      return;
    }
    const { id, fee, pc } = data;
    console.log(
      `[Player] getPlaySongData called for ID: ${id}, Fee: ${fee}, PC: ${pc}, Level: ${level}`,
    );

    // If AutoMix is crossfading, it already handles sound creation — only fetch lyrics
    const autoMix = getAutoMixEngine();
    if (autoMix.isHandoffActive()) {
      console.log("[Player] AutoMix crossfade active, only fetching lyrics");
      // Sync player ref to the incoming sound set by AutoMix
      if (window.$player) {
        player.value = window.$player;
      }
      fetchAndParseLyric(id);
      return;
    }

    // Check audio preloader — if the next song was preloaded, use it directly.
    // NOTE: When the Rust audio backend is available (native or WASM), skip
    // the preloader. Consuming a preloaded `BufferedSound` would silently
    // switch the audio pipeline back to the legacy Web Audio path.
    const preloader = getAudioPreloader();
    const backendAvailable = isAudioBackendRuntimeAvailable();
    const preloadedSound = !backendAvailable ? preloader.consume(id) : null;
    if (preloadedSound) {
      console.log(`[Player] Using preloaded audio for: ${id}`);
      player.value = createSound("", true, preloadedSound);
      // Preloaded sound is already loaded — 'load' event won't fire again,
      // so clear loading state immediately to avoid stuck spinner.
      music.isLoadingSong = false;
      fetchAndParseLyric(id);
      return;
    }

    // Unified URL resolution (NCM + trial detection + UNM fallback + kuwo proxy)
    const result = await resolveSongUrl({ id, fee, pc, name: data.name }, level);
    if (generation !== _songLoadGeneration) return; // stale check

    if (result) {
      console.log(`[Player] Creating sound instance with ${result.source} URL: ${result.url}`);
      player.value = createSound(result.url);
    } else {
      console.warn(`[Player] No URL resolved for ${id}`);
      $message.warning(t("general.message.playError"));
      music.setPlaySongIndex("next");
    }

    // 获取歌词
    fetchAndParseLyric(id);
  } catch (err) {
    if (generation !== _songLoadGeneration) return;
    console.error("[Player] Error in getPlaySongData:", err);
    if (music.getPlaylists[0] && music.getPlayState) {
      $message.warning(t("general.message.playError"));
      music.setPlaySongIndex("next");
    }
  }
};

// 图标渲染
const renderIcon = (icon) => {
  return () => {
    return h(
      NIcon,
      { style: { transform: "translateX(1px)" } },
      {
        default: () => icon,
      },
    );
  };
};

// 歌曲进度条更新
const isSliderDragging = ref(false);
const pendingSliderPercent = ref(null);
const normalizeSliderPercent = (val) => {
  const raw = Array.isArray(val) ? val[0] : val;
  const num = Number(raw);
  if (!Number.isFinite(num)) return null;
  return Math.max(0, Math.min(100, num));
};
const previewSliderTime = (percent) => {
  const duration = music.getPlaySongTime?.duration;
  if (!duration) return;
  music.setPlaySongTime({
    currentTime: (duration / 100) * percent,
    duration,
  });
};
const sliderDragStart = () => {
  isSliderDragging.value = true;
  pendingSliderPercent.value = normalizeSliderPercent(music.getPlaySongTime.barMoveDistance);
};
const sliderDragging = (val) => {
  const percent = normalizeSliderPercent(val);
  if (percent === null) return;
  pendingSliderPercent.value = percent;
  previewSliderTime(percent);
};
const sliderDragEnd = () => {
  isSliderDragging.value = false;
};
const songTimeSliderUpdate = (val) => {
  if (player.value && music.getPlaySongTime?.duration) {
    const percent = normalizeSliderPercent(
      isSliderDragging.value ? (pendingSliderPercent.value ?? val) : val,
    );
    if (percent === null) return;
    isSliderDragging.value = false;
    pendingSliderPercent.value = null;
    const currentTime = (music.getPlaySongTime.duration / 100) * percent;
    setSeek(player.value, currentTime);
    // 一起听歌：发送进度跳转命令（房主和房客均可）
    if (listenTogether.isInRoom) {
      listenTogether.sendPlayCommand("seek", Math.floor(currentTime * 1000));
    }
  }
};

// 静音事件
const volumeMute = () => {
  if (persistData.value.playVolume > 0) {
    persistData.value.playVolumeMute = persistData.value.playVolume;
    persistData.value.playVolume = 0;
  } else {
    persistData.value.playVolume = persistData.value.playVolumeMute;
  }
};

// 播放模式数据
const patternOptions = ref([
  {
    label: t("general.name.random"),
    key: "random",
    icon: renderIcon(h(ShuffleOne)),
  },
  {
    label: t("general.name.single"),
    key: "single",
    icon: renderIcon(h(PlayOnce)),
  },
  {
    label: t("general.name.normal"),
    key: "normal",
    icon: renderIcon(h(PlayCycle)),
  },
]);

// 播放模式点击
const patternClick = (val) => {
  music.setPlaySongMode(val);
};

// 歌曲更换事件
const songChange = debounce(500, (val) => {
  if (val === undefined) {
    window.document.title = sessionStorage.getItem("siteTitle") ?? import.meta.env.VITE_SITE_TITLE;
  }
  // 加载数据
  getPlaySongData(val);
});

// Tauri: broadcast player state to other windows (tray popup, mini player, desktop lyrics)
const broadcastPlayerState = () => {
  if (!isTauri()) return;
  const songData = music.getPlaySongData;
  const playTime = music.getPlaySongTime;
  const payload = {
    title: songData?.name || "",
    artist: songData?.artist?.map((a) => a.name).join(", ") || "",
    artistList: songData?.artist?.map((a) => ({ id: a.id, name: a.name })) || [],
    coverUrl: songData?.album?.picUrl
      ? songData.album.picUrl.replace(/^http:/, "https:") + "?param=128y128"
      : "",
    coverUrlLarge: songData?.album?.picUrl
      ? songData.album.picUrl.replace(/^http:/, "https:") + "?param=512y512"
      : "",
    songId: songData?.id || null,
    isPlaying: music.getPlayState,
    isLoading: music.isLoadingSong,
    isLiked: songData ? music.getSongIsLike(songData.id) : false,
    accentColor: site.songPicColor || "",
    currentTime: playTime?.currentTime || 0,
    duration: playTime?.duration || 0,
    volume: persistData.value.playVolume,
    playMode: music.persistData.playSongMode || "normal",
  };
  emitPlayerBridgeEvent("player-state-update", payload);

  // Update native window effect tint color with accent blend
  if (site.songPicColor) {
    const m = site.songPicColor.match(/(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/);
    if (m) {
      const isDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
      const base = isDark ? 30 : 240;
      const r = Math.round(base * 0.85 + parseInt(m[1]) * 0.15);
      const g = Math.round(base * 0.85 + parseInt(m[2]) * 0.15);
      const b = Math.round(base * 0.85 + parseInt(m[3]) * 0.15);
      windowManager.setWindowEffectColor("tray-popup", r, g, b, 200);
    }
  }
};

// Tauri: broadcast time update to slave windows (~20fps, throttled 50ms)
const broadcastTimeUpdate = throttle(50, () => {
  if (!isTauri() || !music.getPlayState) return;
  emitPlayerBridgeEvent("player-time-update", {
    currentTime: music.getPlaySongTime.currentTime,
    lyricIndex: music.playSongLyricIndex,
  });
});

// Tauri: broadcast lyric data to slave windows
const broadcastLyricData = () => {
  if (!isTauri()) return;
  const songData = music.getPlaySongData;
  if (!songData) return;

  const rawSongLyric = toRaw(music.songLyric);
  if (!rawSongLyric || !rawSongLyric.lrc) return;

  // Process lyrics for AMLL
  let amllLines = [];
  try {
    preprocessLyrics(rawSongLyric, {
      showYrc: setting.showYrc,
      showRoma: setting.showRoma,
      showTransl: setting.showTransl,
    });
    amllLines = getProcessedLyrics(rawSongLyric, {
      showYrc: setting.showYrc,
      showRoma: setting.showRoma,
      showTransl: setting.showTransl,
    });

    // Sanitize based on settings
    if (!setting.showTransl || !setting.showRoma) {
      for (let i = 0; i < amllLines.length; i++) {
        const line = amllLines[i];
        if (!setting.showTransl) line.translatedLyric = "";
        if (!setting.showRoma) {
          line.romanLyric = "";
          const words = line.words;
          for (let j = 0; j < words.length; j++) {
            words[j].romanWord = "";
          }
        }
      }
    }
  } catch (err) {
    console.error("[Player] Failed to process lyrics for broadcast:", err);
  }

  const payload = {
    songId: songData.id,
    lrc: rawSongLyric.lrc.map((l) => ({ time: l.time, content: l.content })),
    amllLines,
    hasYrc: rawSongLyric.hasYrc || false,
    hasLrcTran: rawSongLyric.hasLrcTran || false,
    hasLrcRoma: rawSongLyric.hasLrcRoma || false,
  };
  emitPlayerBridgeEvent("player-lyric-update", payload);
};

// Tauri: broadcast settings to slave windows
const broadcastSettings = () => {
  if (!isTauri()) return;
  const payload = {
    lyricTimeOffset: setting.lyricTimeOffset,
    lyricsFontSize: setting.lyricsFontSize,
    lyricFont: setting.lyricFont,
    lyricFontWeight: setting.lyricFontWeight,
    lyricLetterSpacing: setting.lyricLetterSpacing,
    lyricLineHeight: setting.lyricLineHeight,
    lyricsBlur: setting.lyricsBlur,
    lyricsBlock: setting.lyricsBlock,
    lyricsPosition: setting.lyricsPosition,
    showYrc: setting.showYrc,
    showYrcAnimation: setting.showYrcAnimation,
    showTransl: setting.showTransl,
    showRoma: setting.showRoma,
    springParams: setting.springParams,
  };
  emitPlayerBridgeEvent("player-settings-update", payload);
};

// Tauri: push full state snapshot to a newly opened slave window
const broadcastFullState = (targetLabel) => {
  if (!isTauri()) return;
  const statePayload = {
    title: music.getPlaySongData?.name || "",
    artist: music.getPlaySongData?.artist?.map((a) => a.name).join(", ") || "",
    artistList: music.getPlaySongData?.artist?.map((a) => ({ id: a.id, name: a.name })) || [],
    coverUrl: music.getPlaySongData?.album?.picUrl
      ? music.getPlaySongData.album.picUrl.replace(/^http:/, "https:") + "?param=128y128"
      : "",
    coverUrlLarge: music.getPlaySongData?.album?.picUrl
      ? music.getPlaySongData.album.picUrl.replace(/^http:/, "https:") + "?param=512y512"
      : "",
    songId: music.getPlaySongData?.id || null,
    isPlaying: music.getPlayState,
    isLoading: music.isLoadingSong,
    isLiked: music.getPlaySongData ? music.getSongIsLike(music.getPlaySongData.id) : false,
    accentColor: siteStore().songPicColor || "",
    currentTime: music.getPlaySongTime.currentTime,
    duration: music.getPlaySongTime.duration,
    volume: persistData.value.playVolume,
    playMode: music.persistData.playSongMode || "normal",
  };

  // Build lyric payload
  let lyricPayload = null;
  const rawSongLyric = toRaw(music.songLyric);
  if (rawSongLyric && rawSongLyric.lrc && rawSongLyric.lrc.length > 0 && music.getPlaySongData) {
    let amllLines = [];
    try {
      preprocessLyrics(rawSongLyric, {
        showYrc: setting.showYrc,
        showRoma: setting.showRoma,
        showTransl: setting.showTransl,
      });
      amllLines = getProcessedLyrics(rawSongLyric, {
        showYrc: setting.showYrc,
        showRoma: setting.showRoma,
        showTransl: setting.showTransl,
      });
      if (!setting.showTransl || !setting.showRoma) {
        for (let i = 0; i < amllLines.length; i++) {
          const line = amllLines[i];
          if (!setting.showTransl) line.translatedLyric = "";
          if (!setting.showRoma) {
            line.romanLyric = "";
            const words = line.words;
            for (let j = 0; j < words.length; j++) {
              words[j].romanWord = "";
            }
          }
        }
      }
    } catch (err) {
      console.error("[Player] Failed to process lyrics for full state:", err);
    }
    lyricPayload = {
      songId: music.getPlaySongData.id,
      lrc: rawSongLyric.lrc.map((l) => ({ time: l.time, content: l.content })),
      amllLines,
      hasYrc: rawSongLyric.hasYrc || false,
      hasLrcTran: rawSongLyric.hasLrcTran || false,
      hasLrcRoma: rawSongLyric.hasLrcRoma || false,
    };
  }

  const settingsPayload = {
    lyricTimeOffset: setting.lyricTimeOffset,
    lyricsFontSize: setting.lyricsFontSize,
    lyricFont: setting.lyricFont,
    lyricFontWeight: setting.lyricFontWeight,
    lyricLetterSpacing: setting.lyricLetterSpacing,
    lyricLineHeight: setting.lyricLineHeight,
    lyricsBlur: setting.lyricsBlur,
    lyricsBlock: setting.lyricsBlock,
    lyricsPosition: setting.lyricsPosition,
    showYrc: setting.showYrc,
    showYrcAnimation: setting.showYrcAnimation,
    showTransl: setting.showTransl,
    showRoma: setting.showRoma,
    springParams: setting.springParams,
  };

  const payload = {
    state: statePayload,
    time: {
      currentTime: music.getPlaySongTime.currentTime,
      lyricIndex: music.playSongLyricIndex,
    },
    lyric: lyricPayload,
    settings: settingsPayload,
  };

  emitPlayerBridgeEvent("player-full-state", payload, targetLabel);
};

// Tauri: set up tray + slave control event listeners
const setupTrayListeners = async () => {
  const tauri = window.__TAURI__;
  if (!tauri) return;

  // Listen for tray play/pause
  await tauri.event.listen("tray-play-pause", () => {
    music.setPlayState(!music.getPlayState);
  });

  // Listen for tray prev/next track
  await tauri.event.listen("tray-prev-track", () => {
    music.setPlaySongIndex("prev");
  });
  await tauri.event.listen("tray-next-track", () => {
    music.setPlaySongIndex("next");
  });

  // Listen for tray popup opened — push fresh state
  await tauri.event.listen("tray-popup-opened", () => {
    broadcastPlayerState();
  });

  // Listen for tray play mode cycle
  await tauri.event.listen("tray-cycle-play-mode", () => {
    music.setPlaySongMode();
    broadcastPlayerState();
  });

  // Listen for like song from tray
  await tauri.event.listen("tray-like-song", async () => {
    const songData = music.getPlaySongData;
    if (songData) {
      await music.changeLikeList(songData.id, !music.getSongIsLike(songData.id));
      broadcastPlayerState();
    }
  });

  // ── Slave window command listeners ──────────────────────────────────

  await tauri.event.listen("slave-play-pause", () => {
    music.setPlayState(!music.getPlayState);
  });

  await tauri.event.listen("slave-prev-track", () => {
    music.setPlaySongIndex("prev");
  });

  await tauri.event.listen("slave-next-track", () => {
    music.setPlaySongIndex("next");
  });

  await tauri.event.listen("slave-seek", (e) => {
    const { time } = e.payload;
    if (player.value && typeof time === "number") {
      setSeek(player.value, time);
    }
  });

  await tauri.event.listen("slave-volume", (e) => {
    const { volume } = e.payload;
    if (typeof volume === "number") {
      persistData.value.playVolume = Math.max(0, Math.min(1, volume));
    }
  });

  await tauri.event.listen("slave-cycle-play-mode", () => {
    music.setPlaySongMode();
    broadcastPlayerState();
  });

  // Listen for like song from slave windows
  await tauri.event.listen("slave-like-song", async () => {
    const songData = music.getPlaySongData;
    if (songData) {
      await music.changeLikeList(songData.id, !music.getSongIsLike(songData.id));
      broadcastPlayerState();
    }
  });

  // Listen for lyrics font size changes from desktop lyrics controls
  await tauri.event.listen("slave-set-lyrics-font-size", (e) => {
    const { size } = e.payload;
    if (typeof size === "number") {
      setting.lyricsFontSize = Math.max(2, Math.min(6, size));
      // Broadcast settings update to all slave windows
      broadcastSettings();
    }
  });

  // Slave window opened — push full state snapshot
  await tauri.event.listen("slave-window-opened", (event) => {
    const payload = event.payload || {};
    broadcastFullState(payload.label);
  });
};

onMounted(() => {
  // 挂载方法
  window.$getPlaySongData = getPlaySongData;
  // 获取音乐数据
  if (music.getPlaylists[0] && music.getPlaySongData) {
    getPlaySongData(music.getPlaySongData);
  }

  // Tauri: wire up tray control listeners + state broadcasting
  if (isTauri()) {
    setupTrayListeners();
  }

  // 一起听歌：从 URL 参数自动加入房间
  setTimeout(() => {
    listenTogether.joinFromUrl();
  }, 1000);
});

// 监听当前音乐数据变化
watch(
  () => (music ? music.getPlaySongData : null),
  (val, oldVal) => {
    if (val !== oldVal) {
      // During AutoMix crossfade, don't reset time — adoptIncomingSound handles it.
      // Resetting here causes duration=0 because the incoming sound's play() is async
      // and checkAudioTime only updates when playing() returns true.
      const autoMix = getAutoMixEngine();
      if (!autoMix.isHandoffActive()) {
        music.setPlaySongTime({ currentTime: 0, duration: 0 });
      }
      songChange(val);
      broadcastPlayerState();

      // 一起听歌：发送切歌命令（房主和房客均可）
      if (listenTogether.isInRoom && val?.id && !listenTogether.isProcessingRemoteCommand) {
        listenTogether.sendPlayCommand("GOTO");
        // 仅房主同步播放列表
        if (listenTogether.isHost) {
          listenTogether.syncCurrentPlaylist();
        }
      }

      // Update tray tooltip with current song info
      if (isTauri()) {
        if (val?.name) {
          const artistNames = val.artist?.map((a) => a.name).join(", ") || "";
          const tooltip = artistNames ? `${val.name} - ${artistNames}` : val.name;
          windowManager.setTrayTooltip(tooltip).catch(() => {
            // Silently fail if tray update fails
          });
        } else {
          // Reset to default when no song is playing
          windowManager.setTrayTooltip("GMPlayer").catch(() => {});
        }
      }
    }
  },
  { deep: true },
);

// 监听当前音量数据变化
watch(
  () => persistData.value.playVolume,
  (val) => {
    // Sync player ref if AutoMix changed the underlying sound
    if (window.$player && player.value !== window.$player) {
      player.value = window.$player;
    }
    if (player.value) setVolume(player.value, val);
  },
);

// 监听当前音乐状态变化
watch(
  () => music.getPlayState,
  (val) => {
    console.log(`[Player] Play state changed to: ${val}. Player instance:`, player.value);
    if (window.$player && player.value !== window.$player) {
      player.value = window.$player;
    }

    let nativePlaybackHandled = false;
    if (player.value instanceof NativeRustSound && !music.isLoadingSong) {
      const autoMix = getAutoMixEngine();
      if (!autoMix.isCrossfading() && typeof player.value.playing === "function") {
        const isPlaying = player.value.playing();
        if (val && !isPlaying) {
          fadePlayOrPause(player.value, "play", persistData.value.playVolume);
        } else if (!val && isPlaying) {
          fadePlayOrPause(player.value, "pause", persistData.value.playVolume);
        }
        nativePlaybackHandled = true;
      }
    }

    broadcastPlayerState();
    // Also broadcast time on play state change for slave windows
    if (isTauri()) {
      emitPlayerBridgeEvent("player-time-update", {
        currentTime: music.getPlaySongTime.currentTime,
        lyricIndex: music.playSongLyricIndex,
      });
    }
    // 一起听歌：发送播放状态同步（房主和房客均可）
    if (listenTogether.isInRoom && !listenTogether.isProcessingRemoteCommand) {
      listenTogether.sendPlayCommand(val ? "PLAY" : "PAUSE");
    }
    nextTick().then(() => {
      if (music.getPlayState !== val) return;
      if (nativePlaybackHandled) return;
      // During AutoMix crossfade, CrossfadeManager controls gain scheduling.
      // fadePlayOrPause's fade(0, volume, 300) would cancel CrossfadeManager's
      // scheduled gain ramp and do a fast 300ms ramp instead, breaking the crossfade.
      const autoMix = getAutoMixEngine();
      if (autoMix.isCrossfading()) {
        if (!val) {
          const frozen = autoMix.pauseCrossfade();
          if (frozen) {
            // Crossfade is frozen — it handles pause directly
            console.log("[Player] AutoMix crossfade frozen (paused)");
            return;
          }
          // Crossfade was in setup phase and got cancelled.
          // Fall through to normal fadePlayOrPause below.
          console.log(
            "[Player] AutoMix crossfade cancelled during setup, falling through to normal pause",
          );
        } else {
          autoMix.resumeCrossfade();
          if (autoMix.isCrossfading()) {
            console.log("[Player] AutoMix crossfade resumed");
            return;
          }
          // Crossfade no longer active — fall through to normal resume
        }
      }
      // Sync player ref if AutoMix changed the underlying sound
      if (window.$player && player.value !== window.$player) {
        player.value = window.$player;
      }
      if (player.value && !music.isLoadingSong) {
        const hPlayer = player.value; // Assuming player.value is the Howl instance
        if (typeof hPlayer.playing !== "function") {
          console.error(
            "[Player] player.value is not a valid NativeSound instance or 'playing' method missing",
            hPlayer,
          );
          return;
        }
        const isPlaying = hPlayer.playing();
        console.log(`[Player] Current NativeSound playing state: ${isPlaying}`);

        if (val && !isPlaying) {
          console.log("[Player] Calling fadePlayOrPause with 'play'");
          fadePlayOrPause(player.value, "play", persistData.value.playVolume);
        } else if (!val && isPlaying) {
          console.log("[Player] Calling fadePlayOrPause with 'pause'");
          fadePlayOrPause(player.value, "pause", persistData.value.playVolume);
        } else {
          console.log(
            "[Player] fadePlayOrPause skipped, already in desired state or player not ready.",
          );
        }
      } else {
        console.warn(
          `[Player] Skipping fadePlayOrPause. Player: ${player.value}, isLoadingSong: ${music.isLoadingSong}`,
        );
      }
    });
  },
);

// Tauri: broadcast time update when currentTime changes
watch(
  () => music.getPlaySongTime.currentTime,
  () => {
    broadcastTimeUpdate();
  },
);

// Tauri: broadcast lyric data when songLyric changes
watch(
  () => music.songLyric,
  () => {
    broadcastLyricData();
  },
  { deep: true },
);

// Tauri: broadcast settings when lyric-related settings change
watch(
  () => [
    setting.lyricTimeOffset,
    setting.lyricsFontSize,
    setting.lyricFont,
    setting.lyricFontWeight,
    setting.lyricLetterSpacing,
    setting.lyricLineHeight,
    setting.lyricsBlur,
    setting.lyricsBlock,
    setting.lyricsPosition,
    setting.showYrc,
    setting.showYrcAnimation,
    setting.showTransl,
    setting.showRoma,
  ],
  () => {
    broadcastSettings();
    // Re-process and re-broadcast lyrics when display settings change
    broadcastLyricData();
  },
);

const fetchAndParseLyric = async (id) => {
  try {
    const { result, stale } = await lyricFetcher.fetchLyric(id);
    if (stale) {
      console.log(`[Player] Lyric fetch for ${id} is stale, discarding`);
      return;
    }
    music.setPlaySongLyric(result);
  } catch (err) {
    console.error(`[Player] Failed to fetch lyric for ${id}:`, err);
    const defaultResult = parseLyric(null);
    defaultResult.formattedLrc = "";
    music.setPlaySongLyric(defaultResult);
  }
};

watch(
  () => [
    miniLyricLine.value?.key,
    miniLyricLine.value?.words.map((item) => item.content).join(""),
    setting.bottomLyricShow,
    setting.showYrc,
  ],
  () => {
    miniLrcOverflow.value = false;
    nextTick(updateMiniLrcOverflow);
  },
  { immediate: true },
);

watch(
  () => music.showBigPlayer,
  () => nextTick(updateMiniLrcOverflow),
);
</script>

<style lang="scss" scoped>
.show-enter-active,
.show-leave-active {
  transform: translateY(0);
  transition: all 0.3s cubic-bezier(0.65, 0.05, 0.36, 1);
}

.show-enter-from,
.show-leave-to {
  transform: translateY(80px);
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.15s cubic-bezier(0.4, 0, 0.2, 1);
}

.mini-lyric-enter-active,
.mini-lyric-leave-active {
  transition: opacity 0.08s linear;
}

.fade-enter-from,
.fade-leave-to,
.mini-lyric-enter-from,
.mini-lyric-leave-to {
  opacity: 0;
}

.player {
  --player-data-edge-inset: 14px;
  --player-control-edge-inset: 14px;
  --player-slider-edge-inset: 0px;

  height: 70px;
  position: fixed;
  bottom: 0;
  left: var(--sidebar-width, 240px);
  width: calc(100% - var(--sidebar-width, 240px) - var(--player-right-inset, 0px));
  z-index: 2;
  transition:
    left 0.3s ease,
    width 0.3s ease,
    opacity 0.18s ease,
    translate 0.22s ease;

  // Acrylic background — override Naive UI card bg
  background-color: var(
    --player-surface-bg,
    var(--app-shell-bg, var(--layout-bg, #fff))
  ) !important;
  border-top: 1px solid
    var(--mobile-mini-player-surface-border, var(--acrylic-border, rgba(0, 0, 0, 0.04)));
  box-shadow: var(--mobile-mini-player-surface-shadow, none);

  // Mobile: player sits above tab bar, no sidebar
  @media (max-width: 768px) {
    bottom: 56px;
    left: 0;
    width: 100%;
    --n-border-color: transparent !important;
    --n-border-radius: 0 !important;
    z-index: var(--mobile-mini-player-z-index, 2);
    pointer-events: var(--mobile-mini-player-pointer-events, auto);
    touch-action: pan-x;
    isolation: isolate;
    --player-data-edge-inset: 12px;
    --player-control-edge-inset: 12px;
    --player-slider-edge-inset: 12px;
    background-color: transparent !important;
    border: none !important;
    outline: none !important;
    box-shadow: none;
    overflow: visible !important;

    &::before {
      content: "";
      position: absolute;
      inset: 0;
      z-index: -1;
      background-color: var(
        --mobile-mini-player-surface-bg,
        var(--acrylic-bg, rgba(255, 255, 255, 0.45))
      );
      box-shadow: var(--mobile-mini-player-surface-shadow, 0 -10px 28px rgb(0 0 0 / 10%));
      opacity: var(--mobile-mini-player-surface-opacity, 1);
      transform: translate3d(0, var(--mobile-mini-player-mask-y, 0px), 0);
      pointer-events: none;
      will-change: opacity, transform;
    }

    :deep(.n-card__content) {
      position: static;
      border: none !important;
      outline: none !important;
    }
  }

  .slider {
    position: absolute;
    top: -12px;
    left: var(--player-slider-edge-inset, 0px);
    right: var(--player-slider-edge-inset, 0px);
    display: flex;
    align-items: center;
    justify-content: space-between;
    z-index: 2;
    opacity: var(--mobile-mini-player-chrome-opacity, var(--mobile-mini-player-ui-opacity, 1));
    transform: translateY(var(--mobile-mini-player-ui-y, 0px));
    will-change: opacity, transform;

    @media (max-width: 640px) {
      top: -8px;

      > {
        span {
          display: none;
        }
      }
    }

    > {
      span {
        font-size: 12px;
        white-space: nowrap;
        background-color: var(--n-color);
        outline: 1px solid var(--n-border-color);
        padding: 2px 8px;
        border-radius: var(--radius-pill);
        margin: 0 2px;
      }
    }

    .vue-slider {
      width: 100% !important;
      height: 3px !important;
      cursor: pointer;

      .slider-tooltip {
        font-size: 12px;
        white-space: nowrap;
        background-color: var(--n-color);
        outline: 1px solid var(--n-border-color);
        padding: 2px 8px;
        border-radius: var(--radius-pill);
      }

      :deep(.vue-slider-rail) {
        background-color: var(--n-border-color);
        border-radius: var(--radius-pill);

        .vue-slider-process {
          background-color: var(--main-color);
        }

        .vue-slider-dot {
          width: 12px !important;
          height: 12px !important;
        }

        .vue-slider-dot-handle-focus {
          box-shadow: 0px 0px 1px 2px var(--main-color);
        }
      }
    }
  }

  .all {
    height: 100%;
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    align-items: center;
    max-width: 1400px;
    margin: 0 auto;
    position: relative;
    z-index: 1;

    .data {
      display: flex;
      flex-direction: row;
      align-items: center;
      min-width: 0;
      overflow: hidden;
      position: relative;
      z-index: 4;
      box-sizing: border-box;
      padding-left: var(--player-data-edge-inset, 14px);
      transform: translate3d(0, var(--mobile-mini-player-root-y, 0px), 0);
      will-change: transform;

      .pic {
        width: 50px;
        height: 50px;
        min-width: 50px;
        border-radius: var(--radius-md) !important;
        clip-path: inset(0 round var(--radius-md));
        overflow: hidden;
        margin-right: 12px;
        position: relative;
        z-index: 4;
        box-shadow: 0 6px 8px -2px rgb(0 0 0 / 16%);
        cursor: pointer;
        opacity: var(--mobile-mini-player-artwork-opacity, 1);
        will-change: opacity;

        img {
          width: 100%;
          height: 100%;
          border-radius: inherit;
          object-fit: cover;
          display: block;
        }
      }

      .name {
        min-width: 0;
        overflow: hidden;
        position: relative;
        z-index: 4;
        opacity: var(--mobile-mini-player-text-opacity, var(--mobile-mini-player-ui-opacity, 1));
        transform: translateY(var(--mobile-mini-player-text-y, 0px));
        will-change: opacity, transform;

        .song {
          font-size: 16px;
          font-weight: bold;
          cursor: pointer;
          transition: all 0.3s;

          &:hover {
            color: var(--main-color);
          }
        }

        .artisrOrLrc {
          font-size: 12px;
          margin-top: var(--mobile-mini-player-detail-margin, 2px);
          line-height: 1.3;
          max-height: var(--mobile-mini-player-detail-height, 1.3em);
          opacity: var(--mobile-mini-player-detail-opacity, 1);
          overflow: hidden;
          will-change: opacity, max-height, margin-top;

          .lrc {
            display: block !important;
            position: relative;
            width: 100%;
            height: 1.3em;
            line-height: 1.3;
            white-space: nowrap;
            overflow: hidden;
            word-break: normal;

            .lrc-measure-content {
              display: inline-block;
              white-space: nowrap;
            }

            &.is-marquee {
              .lrc-measure-content {
                position: absolute;
                visibility: hidden;
                pointer-events: none;
              }
            }

            .mini-lrc-marquee {
              width: 100%;
              height: 1.3em;
              line-height: 1.3;
              color: inherit;

              :deep(.overflow-marquee__group) {
                align-items: center;
                height: 1.3em;
                line-height: 1.3;
                min-width: max-content;
                white-space: nowrap;
              }
            }

            .lrc-marquee-content {
              display: inline-block;
              padding-right: 2em;
              white-space: nowrap;
            }

            .lrc-word {
              display: inline;
            }
          }
        }
      }
    }

    .control {
      display: flex;
      flex-direction: row;
      align-items: center;
      justify-content: center;
      position: relative;
      z-index: 3;
      opacity: var(--mobile-mini-player-chrome-opacity, var(--mobile-mini-player-ui-opacity, 1));
      transform: translateY(var(--mobile-mini-player-ui-y, 0px));
      will-change: opacity, transform;

      .next,
      .prev,
      .dislike {
        color: var(--main-color);
        cursor: pointer;
        padding: 4px;
        border-radius: var(--radius-pill);
        transform: scale(1);
        transition: all 0.3s;

        &:hover {
          color: var(--n-color-embedded);
          background-color: var(--main-color);
        }

        &:active {
          transform: scale(0.9);
        }
      }

      .dislike {
        padding: 9px;
      }

      .play-state {
        width: 46px;
        height: 46px;
        color: var(--main-color);
        margin: 0 12px;
        cursor: pointer;
        transform: scale(1);
        transition: all 0.3s;
        display: flex;
        align-items: center;
        justify-content: center;
        position: relative;

        .play-state-inner {
          display: flex;
          align-items: center;
          justify-content: center;
          position: absolute;
        }

        &:hover {
          transform: scale(1.1);
        }

        &:active {
          transform: scale(1);
        }
      }
    }

    .menu {
      position: relative;
      height: 100%;
      display: flex;
      flex-direction: row;
      align-items: center;
      justify-content: flex-end;
      color: var(--main-color);
      z-index: 3;
      box-sizing: border-box;
      padding-right: var(--player-control-edge-inset, 14px);
      opacity: var(--mobile-mini-player-chrome-opacity, var(--mobile-mini-player-ui-opacity, 1));
      transform: translateY(var(--mobile-mini-player-ui-y, 0px));
      will-change: opacity, transform;

      @media (max-width: 640px) {
        .volume,
        .like,
        .add-playlist,
        .pattern {
          display: none !important;
        }
      }

      &.fm {
        .pattern,
        .playlist {
          display: none;
        }
      }

      .n-icon {
        padding: 4px;
        border-radius: var(--radius-md);
        cursor: pointer;
        transition: all 0.3s;

        @media (min-width: 640px) {
          &:hover {
            background-color: var(--main-color);
            color: var(--n-color-embedded);
          }
        }

        &:active {
          transform: scale(0.95);
        }
      }

      .like {
        display: flex;
        align-items: center;
        justify-content: center;

        .n-icon {
          padding: 7px;
          margin-top: 1px;
        }
      }

      .add-playlist {
        margin-left: 8px;
        display: flex;
        align-items: center;
        justify-content: center;
      }

      .pattern {
        margin-left: 8px;

        .n-icon {
          font-size: 22px;
          padding: 8px;
        }
      }

      .playlist {
        margin-left: 8px;
        display: flex;
        align-items: center;
        justify-content: center;

        &.open {
          .n-icon {
            background-color: var(--main-color);
            color: var(--n-color-embedded);
          }
        }
      }

      .volume {
        display: flex;
        align-items: center;
        flex-direction: row;
        margin-left: 8px;
        width: 100px;

        .n-icon {
          margin-right: 6px;
        }

        .volmePg {
          --n-handle-size: 12px;
          --n-rail-height: 3px;
        }
      }
    }

    @media (max-width: 620px) {
      display: flex;
      flex-direction: row;
      justify-content: space-between;

      .data {
        .time {
          display: none;
        }
      }

      .control {
        margin-left: auto;

        .prev,
        .next {
          display: none;
        }

        .play-state {
          margin: 0;
        }
      }
    }
  }
}
</style>
