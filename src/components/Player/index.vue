<template>
  <LayoutGroup id="mobile-player-layout">
    <Transition name="show">
      <div
        v-show="music.getPlaylists[0] && music.showPlayBar"
        class="player"
        data-mobile-player-bg
        @click.stop="handleMiniPlayerClick"
        @touchstart.passive="handleMiniTouchStart"
        @touchmove.passive="handleMiniTouchMove"
        @touchend.passive="handleMiniTouchEnd"
        @touchcancel="resetMiniTouch"
      >
        <MiniPlayerProgress />
        <div class="all">
          <div class="data">
            <div class="pic" data-mobile-player-artwork @click.stop="handleMiniArtworkClick">
              <img
                :src="coverUrl(music.getPlaySongData?.album?.picUrl, 50)"
                decoding="async"
                alt="pic"
              />
            </div>
            <div class="name">
              <div class="song text-hidden" @click.stop="openSongDetail">
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
                  <n-spin :size="28" stroke="var(--player-accent-color, var(--main-color))" />
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
                  role="button"
                  tabindex="0"
                  :aria-pressed="music.showPlayList"
                  :aria-label="$t('general.name.playlists')"
                  @pointerdown.stop="armPlaylistToggle"
                  @pointercancel="clearPlaylistToggleIntent"
                  @click.stop="togglePlaylist"
                  @keydown.enter.prevent="togglePlaylist"
                  @keydown.space.prevent="togglePlaylist"
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
      </div>
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
  VolumeOffRound,
  VolumeMuteRound,
  VolumeDownRound,
  VolumeUpRound,
  ThumbDownRound,
  FavoriteBorderRound,
  FavoriteRound,
  PlaylistAddRound,
  PlaylistPlayRound,
} from "@vicons/material";
import { PlayCycle, PlayOnce, ShuffleOne } from "@icon-park/vue-next";
import { storeToRefs } from "pinia";
import {
  musicStore,
  settingStore,
  siteStore,
  listenTogetherStore,
  localLibraryStore,
} from "@/store";
import {
  createSound,
  setVolume,
  setSeek,
  fadePlayOrPause,
  getAutoMixEngine,
  getAudioPreloader,
  isNativeAdvanceHoldActiveFor,
  adoptNativeBackendSession,
  SoundManager,
} from "@/utils/AudioContext";
import { useRouter } from "vue-router";
import { debounce } from "throttle-debounce";
import { useI18n } from "vue-i18n";
import { isTauri } from "@/utils/tauri";
import {
  NativeRustSound,
  isAudioBackendRuntimeAvailable,
  announceNativeTrack,
} from "@/utils/tauri/audio/nativeRustSound";
import {
  toTrackDisplay,
  publishNativeManifest,
} from "@/utils/AudioContext/NativeManifestPublisher";
import { windowManager } from "@/utils/tauri/window/manager";
import {
  broadcastPlayerLyrics,
  broadcastPlayerSettings,
  broadcastPlayerState,
  broadcastPlayerTime,
  setupMainPlayerCommunication,
} from "@/utils/tauri/player/communication";
import { useNativeMediaControls } from "@/composables/useNativeMediaControls";
import AddPlaylist from "@/components/DataModal/AddPlaylist.vue";
import PlayListDrawer from "@/components/DataModal/PlayListDrawer.vue";
import ListenTogetherModal from "@/components/DataModal/ListenTogetherModal.vue";
import ListenTogetherStatus from "./ListenTogetherStatus.vue";
import AllArtists from "@/components/DataList/AllArtists.vue";
import OverflowMarquee from "@/components/Common/OverflowMarquee.vue";
import BigPlayer from "./BigPlayer/index.vue";
import MiniPlayerProgress from "./MiniPlayerProgress.vue";
import { shallowRef, watch } from "vue";
import { parseLyricData as parseLyric } from "@/utils/LyricsProcessor";
import { lyricFetcher } from "@/utils/lyricFetcher";
import { localLyricFor } from "@/utils/localLibrary";
import { onLocalTracksChanged } from "@/utils/localLibraryMutations";
import { detectWordTimedLyricFormat } from "@/utils/LyricsProcessor/timeUtils";
import { coverUrl } from "@/utils/coverUrl";

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
// Audio controller identity must stay raw: SoundManager compares instances by reference.
// A normal ref deeply proxies class instances and makes the active sound look stale.
const player = shallowRef(null);

// Async generation tracker — each getPlaySongData call increments this.
// Stale async callbacks (URL fetches, availability checks) compare their captured
// generation against the current value and bail out if a newer request superseded them.
let _songLoadGeneration = 0;
const failedAutoSkipSongIds = new Set();
let failedAutoSkipQueueKey = "";
/**
 * Descriptor from `adoptNativeBackendSession()`, consumed once by the matching
 * `getPlaySongData` call. Non-null only during boot, and only when the Rust
 * backend outlived the WebView and is still playing a track we can name.
 */
let pendingBackendAttach = null;

/** Unsubscribe for the local-library edit signal; see `applyLocalTrackChange`. */
let unsubscribeLocalChanges = null;

// 获取歌曲播放数据
const getPlaySongData = async (data, level = setting.songLevel, requestedGeneration = null) => {
  const generation = requestedGeneration ?? ++_songLoadGeneration;
  if (generation !== _songLoadGeneration) return;
  try {
    if (!data || !data.id) {
      console.error("[Player] getPlaySongData called with invalid data:", data);
      if (generation === _songLoadGeneration && !music.getPlaySongData) {
        music.isLoadingSong = false;
        music.loadingStage = "idle";
        music.setPlayState(false);
      }
      return;
    }
    const { id, fee, pc } = data;
    const isCurrentRequest = () =>
      generation === _songLoadGeneration && Number(music.getPlaySongData?.id) === Number(id);
    console.log(
      `[Player] getPlaySongData called for ID: ${id}, Fee: ${fee}, PC: ${pc}, Level: ${level}`,
    );

    const autoMix = getAutoMixEngine();

    // Boot adoption: the Rust backend survived a WebView reload and is already
    // playing this exact track. Attach to it instead of resolving a fresh URL —
    // that would restart playback from the stale persisted position.
    if (pendingBackendAttach && Number(pendingBackendAttach.songId) === Number(id)) {
      const attach = pendingBackendAttach;
      pendingBackendAttach = null;
      console.log(`[Player] Attaching to live backend playback for ID: ${id}`);
      player.value = createSound(attach.sourceUrl, attach.isPlaying, undefined, {
        songId: id,
        attachIdentity: attach.identity,
      });
      loadLyricFor(data, id);
      return;
    }

    // Backend-initiated native advance (queue-window prefill): the active
    // NativeRustSound is already playing this song — reuse it instead of
    // resolving a fresh URL and re-creating the sound (which would restart
    // playback from 0).
    if (isNativeAdvanceHoldActiveFor(id)) {
      console.log("[Player] Native advance adopted, only fetching lyrics");
      if (window.$player) {
        player.value = window.$player;
      }
      music.isLoadingSong = false;
      music.loadingStage = "idle";
      loadLyricFor(data, id);
      return;
    }

    // AutoMix owns only its actual incoming/current song. Manual selection of
    // a different song during finishing/handoff must cancel the transition and
    // continue through normal URL resolution.
    if (autoMix.isHandoffActive()) {
      const requestedSongId = Number(id);
      const activeSoundSongId = SoundManager.getSongId(window.$player);
      const incomingSongId = Number(music.autoMixState.incomingSongId);
      if (requestedSongId === activeSoundSongId || requestedSongId === incomingSongId) {
        console.log("[Player] AutoMix handoff owns requested song, only fetching lyrics");
        if (window.$player) {
          player.value = window.$player;
        }
        music.isLoadingSong = false;
        music.loadingStage = "idle";
        loadLyricFor(data, id);
        return;
      }
      autoMix.cancelCrossfade();
    }

    // Local file. Split off *before* the preloader and before
    // `resolveSongUrl`: there is nothing to resolve (the locator is the source),
    // and a local id is a negative hash — sending it to Netease would be a
    // request for a track that does not exist.
    if (data.local?.uri) {
      const uri = String(data.local.uri);
      announceNativeTrack({ provider: "local", path: uri }, toTrackDisplay(data));
      console.log(`[Player] Creating sound instance for local file: ${uri}`);
      player.value = createSound(uri, true, undefined, { songId: id });
      loadLocalLyric(data);
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
      if (!isCurrentRequest()) {
        preloadedSound.unload();
        return;
      }
      console.log(`[Player] Using preloaded audio for: ${id}`);
      player.value = createSound("", true, preloadedSound, {
        songId: id,
      });
      // Preloaded sound is already loaded — 'load' event won't fire again,
      // so clear loading state immediately to avoid stuck spinner.
      music.isLoadingSong = false;
      music.loadingStage = "idle";
      fetchAndParseLyric(id);
      return;
    }

    // Swap the OS media session to the new track *before* resolving its URL.
    // Resolution is a network round trip and the download that follows is
    // another; without this SMTC / MediaSession keeps showing the previous
    // track for that whole window, so pressing next looks like nothing
    // happened. The backend shows it as not-yet-playing until the load lands.
    announceNativeTrack({ provider: "netease", id: String(id) }, toTrackDisplay(data));

    // Unified URL resolution (NCM + trial detection + UNM fallback + kuwo proxy)
    // `local` is carried even though the branch above already returned for a
    // local file: the narrowed object is the only thing the resolver sees, and a
    // caller that silently drops the dispatch key is how this went wrong once.
    const result = await resolveSongUrl({ id, fee, pc, name: data.name, local: data.local }, level);
    if (!isCurrentRequest()) return;

    if (result) {
      failedAutoSkipSongIds.clear();
      failedAutoSkipQueueKey = "";
      console.log(`[Player] Creating sound instance with ${result.source} URL: ${result.url}`);
      player.value = createSound(result.url, true, undefined, {
        songId: id,
      });
    } else {
      console.warn(`[Player] No URL resolved for ${id}`);
      $message.warning(t("general.message.playError"));
      const queueKey = music.getPlaylists.map((song) => Number(song.id)).join(",");
      if (queueKey !== failedAutoSkipQueueKey) {
        failedAutoSkipSongIds.clear();
        failedAutoSkipQueueKey = queueKey;
      }
      failedAutoSkipSongIds.add(Number(id));
      const queueSongIds = new Set(music.getPlaylists.map((song) => Number(song.id)));
      const allQueueSongsFailed =
        queueSongIds.size === 0 ||
        [...queueSongIds].every((songId) => failedAutoSkipSongIds.has(songId));
      if (allQueueSongsFailed) {
        music.isLoadingSong = false;
        music.loadingStage = "error";
        music.setPlayState(false);
      } else {
        music.setPlaySongIndex("next");
      }
    }

    // 获取歌词
    if (isCurrentRequest()) fetchAndParseLyric(id);
  } catch (err) {
    if (
      generation !== _songLoadGeneration ||
      Number(music.getPlaySongData?.id) !== Number(data?.id)
    ) {
      return;
    }
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
const songChange = debounce(500, (val, generation) => {
  getPlaySongData(val, setting.songLevel, generation);
});

const setupPlayerCommunication = () => {
  setupMainPlayerCommunication({
    seek(time) {
      if (player.value) setSeek(player.value, time);
    },
  }).catch((err) => {
    console.error("[Player] Failed to setup player communication:", err);
  });
};

// 一起听歌：把本地列表变更推给房间。
//
// 一起听是「一份列表，两个消费者」—— 任何一方加歌 / 删歌 / 插播下一首，两边都要
// 跟上。此前只有房主、且只在切歌时才整表上报一次，房客加的歌根本传不出去。
//
// 签名只在房间里才计算：不在房间时 getter 不触碰 playlists，也就不登记依赖，
// 对普通播放路径零开销。
const roomPlaylistSignature = () => {
  if (!listenTogether.isInRoom) return "";
  const list = music.persistData.playlists;
  let signature = `${list.length}`;
  for (let i = 0; i < list.length; i++) signature += `,${list[i]?.id}`;
  return signature;
};

const syncRoomPlaylist = debounce(400, () => {
  if (!listenTogether.isInRoom) return;
  // 回推抑制靠 store 里的 lastPlaylistSignature（内容比对），不靠这里的时序 ——
  // 这个回调是 debounce 之后才跑的，任何「正在处理远端命令」的标志早已清掉。
  void listenTogether.syncCurrentPlaylist();
});

watch(roomPlaylistSignature, (val, oldVal) => {
  // 空签名 = 不在房间，进出房间本身不该触发上报。
  if (!val || !oldVal || val === oldVal) return;
  syncRoomPlaylist();
});

onMounted(() => {
  // 挂载方法
  window.$getPlaySongData = getPlaySongData;

  // 随机模式的顺序装在队列里，所以在读索引的人（下面的恢复播放、后端会话采纳）
  // 之前先把队列摆正。老版本升级上来时才真的会动，之后都是空转。
  music.ensureShuffledInRandomMode();

  const startRestoredPlayback = () => {
    if (music.getPlaylists[0] && music.getPlaySongData) {
      const generation = ++_songLoadGeneration;
      getPlaySongData(music.getPlaySongData, setting.songLevel, generation);
    }
  };

  if (isTauri()) {
    // The Rust process — and playback — outlives the WebView on Android: the
    // page is destroyed and reloaded while audio keeps going. Ask the backend
    // what it is playing BEFORE touching the restored (stale) snapshot,
    // otherwise we resolve a URL for the track that was playing when the app
    // was backgrounded and cut off live audio.
    adoptNativeBackendSession()
      .then((adopted) => {
        pendingBackendAttach = adopted;
      })
      .catch((err) => {
        console.warn("[Player] Backend session adoption failed:", err);
        pendingBackendAttach = null;
      })
      .finally(() => {
        // Adoption moves playSongIndex, which fires the sync song watcher and
        // schedules its own debounced load. Drop that one and drive the load
        // from here so the attach descriptor is consumed exactly once.
        songChange.cancel({ upcomingOnly: true });
        startRestoredPlayback();
        // `getPlaySongData` consumes the descriptor synchronously when the ids
        // match. Anything left over describes a track we are not loading, and
        // must not be applied to some later selection of the same song.
        pendingBackendAttach = null;
      });
  } else {
    startRestoredPlayback();
  }

  // Tauri: wire up tray control listeners + state broadcasting
  if (isTauri()) {
    setupPlayerCommunication();
  }

  // 一起听歌：URL 邀请优先（用户的显式动作），否则尝试恢复重载前的房间。
  // 只在主窗口执行 —— 从窗口走 slave-main.ts / SlaveApp.vue，不挂载本组件。
  setTimeout(() => {
    void listenTogether.joinFromUrl().then((joined) => {
      if (!joined) void listenTogether.resumePersistedRoom();
    });
  }, 1000);

  // 本地曲目被编辑后，把播放态跟上。挂在这里而不是 store 里：`store/musicData`
  // 已经 import 了 localLibrary store，反向再引就成环；而本组件只在主窗口挂载，
  // 所以队列仍然只有一个写者。
  unsubscribeLocalChanges = onLocalTracksChanged((change) => {
    void applyLocalTrackChange(change);
  });
});

onUnmounted(() => {
  unsubscribeLocalChanges?.();
  unsubscribeLocalChanges = null;
});

// 监听当前音乐数据变化
watch(
  () => (music ? music.getPlaySongData : null),
  (val, oldVal) => {
    // 以歌曲 ID 判定是否切歌：队列被整体替换时对象引用必然变化，
    // 但同一首歌不应重新加载；不同的歌（即使处于相同索引）必须加载。
    if (val?.id !== oldVal?.id) {
      const generation = ++_songLoadGeneration;
      songChange.cancel({ upcomingOnly: true });
      if (!val?.id) {
        failedAutoSkipSongIds.clear();
        failedAutoSkipQueueKey = "";
        player.value = null;
        window.document.title =
          sessionStorage.getItem("siteTitle") ?? import.meta.env.VITE_SITE_TITLE;
      }
      // During AutoMix crossfade, don't reset time — adoptIncomingSound handles it.
      // Resetting here causes duration=0 because the incoming sound's play() is async
      // and checkAudioTime only updates when playing() returns true.
      const autoMix = getAutoMixEngine();
      if (!autoMix.isHandoffActive()) {
        music.setPlaySongTime({ currentTime: 0, duration: 0 });
      }
      if (val?.id) songChange(val, generation);
      broadcastPlayerState();

      // 一起听歌：发送切歌命令（房主和房客均可）
      if (listenTogether.isInRoom && val?.id && !listenTogether.isProcessingRemoteCommand) {
        listenTogether.sendPlayCommand("GOTO");
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
  { flush: "sync" },
);

// Tauri: cover palette extraction completes asynchronously after song metadata changes.
// Broadcast the refreshed accent color instead of waiting for a later play/pause/time event.
watch(
  () => site.songPicColor,
  (val, oldVal) => {
    if (val === oldVal) return;
    broadcastPlayerState();
  },
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
    const currentSongId = Number(music.getPlaySongData?.id);
    // `!== 0`, not `> 0`: a local file's id is a negative hash of its path, so a
    // positive-only test refused to start playback for every imported track and
    // immediately set the state back to paused.
    if (val && (!Number.isFinite(currentSongId) || currentSongId === 0)) {
      music.setPlayState(false);
      return;
    }
    if (window.$player && player.value !== window.$player) {
      player.value = window.$player;
    }

    if (val && player.value && SoundManager.getSongId(player.value) !== currentSongId) {
      console.warn("[Player] Refusing to resume a sound bound to another song", {
        currentSongId,
        soundSongId: SoundManager.getSongId(player.value),
      });
      if (!music.isLoadingSong && music.getPlaySongData) {
        music.isLoadingSong = true;
        const generation = ++_songLoadGeneration;
        songChange.cancel({ upcomingOnly: true });
        void getPlaySongData(music.getPlaySongData, setting.songLevel, generation);
      }
      return;
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
    broadcastPlayerTime(true);
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
    broadcastPlayerTime();
  },
);

// Tauri: keep slave windows from holding stale loading state while lyrics arrive later.
watch(
  () => music.isLoadingSong,
  () => {
    broadcastPlayerState();
    broadcastPlayerTime(true);
  },
);

// Tauri: broadcast lyric data when songLyric changes
watch(
  () => music.songLyric,
  () => {
    broadcastPlayerLyrics(true);
    broadcastPlayerTime(true);
  },
);

// Queue emptied: no track is live any more, so let the lyric cache drop the
// processed-line graph it keeps warm for the playing song. Deliberately keyed
// on the queue going empty rather than on resetSongLyricState(), which also
// fires on every ordinary track switch (where the graph is about to be needed).
watch(
  () => music.getPlaySongData?.id ?? null,
  (id) => {
    if (id === null) lyricFetcher.releaseDerived();
  },
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
    broadcastPlayerSettings();
    // Re-process and re-broadcast lyrics when display settings change
    broadcastPlayerLyrics(true);
  },
);

// Tauri: render-only lyric settings; don't re-process lyrics for them.
watch(
  () => [setting.desktopLyricsFontSizeOffset, setting.hidePassedLines],
  () => {
    broadcastPlayerSettings();
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
    nextTick(() => broadcastPlayerLyrics(true));
  } catch (err) {
    console.error(`[Player] Failed to fetch lyric for ${id}:`, err);
    const defaultResult = parseLyric(null);
    music.setPlaySongLyric(defaultResult);
    nextTick(() => broadcastPlayerLyrics(true));
  }
};

/**
 * The Netease detail page for the track in the mini player.
 *
 * Inert for a local file. `/song?id=-N` would fire three requests on arrival
 * (detail, comments, similar playlists) about an id that exists nowhere and land
 * on an empty page; there is no local equivalent to route to instead.
 */
const openSongDetail = () => {
  const data = music.getPlaySongData;
  if (!data || data.local?.uri) return;
  router.push(`/song?id=${data.id}`);
};

/**
 * Lyrics for whichever kind of track this is.
 *
 * The three adoption branches above — a boot attach to a backend that kept
 * playing, a backend-initiated native advance, an AutoMix handoff — are reached
 * with a local row exactly as often as with a Netease one, and they run *before*
 * the local dispatch because they must not re-resolve a source at all. Handing a
 * negative id to `lyricFetcher` there did two wrong things at once: it asked
 * Netease about a track that does not exist, and it left the sidecar `.lrc`
 * unread, so the lyric panel stayed empty for precisely the tracks whose lyrics
 * are sitting next to the file. The branches after the local dispatch keep
 * calling `fetchAndParseLyric` directly — a local row cannot reach them.
 */
const loadLyricFor = (data, id) => {
  if (data?.local?.uri) {
    void loadLocalLyric(data);
    return;
  }
  fetchAndParseLyric(id);
};

/**
 * Lyrics for a local file: an import, then the embedded tag, then a sibling
 * `.lrc`/`.ttml` — plus the stored translation and romanisation for whichever won.
 *
 * Deliberately separate from `fetchAndParseLyric`, which stays Netease-only —
 * `lyricFetcher` is keyed on a Netease song id and caches against it, so a
 * negative local id would either miss forever or collide with a real track.
 * Parsed through the same `parseLyric`, so a local `.lrc` renders in AMLL exactly
 * like a fetched one.
 */
const loadLocalLyric = async (data) => {
  const key = data?.local?.uri;
  const requestedId = Number(data?.id);
  try {
    const lyric = key ? await localLyricFor(String(key)) : null;
    // The user may have skipped on while the file was being read.
    if (Number(music.getPlaySongData?.id) !== requestedId) return;
    music.setPlaySongLyric(parseLyric(await localLyricPayload(lyric)));
  } catch (err) {
    console.error("[Player] Failed to read local lyrics:", err);
    if (Number(music.getPlaySongData?.id) !== requestedId) return;
    music.setPlaySongLyric(parseLyric(null));
  }
  nextTick(() => broadcastPlayerLyrics(true));
};

/**
 * Shape a local lyric into what `parseLyricData` accepts.
 *
 * `code: 200` is not decoration — the parser returns an empty result for
 * anything else, so a perfectly good `.lrc` would render as "no lyrics".
 *
 * Three families, and each needs a *plain* LRC alongside whatever rich form it
 * has: `parseLyricData` fills the line-level view from `lrc` and the word-level
 * view from `yrc`/`ttml` independently, so shipping only the rich half leaves the
 * mini player and the plain lyric list empty. Both rich families are therefore
 * parsed here and flattened back to LRC lines.
 *
 * Rust's `kind` is only a hint (it comes from the file extension). The word-timed
 * dialect is decided by `detectWordTimedLyricFormat` on the content, which is the
 * same rule the Netease path uses — one implementation, not two.
 */
const localLyricPayload = async (lyric) => {
  const raw = typeof lyric?.text === "string" ? lyric.text.trim() : "";
  if (!raw) return null;

  // Translation and romanisation ride along whatever the main lyric turns out to
  // be: `parseLyricData` aligns them onto the LRC *and* the word-timed view, so
  // they belong on all three shapes below rather than only on the plain one.
  const companions = {
    tlyric: lyric?.tlyric ? { lyric: lyric.tlyric } : null,
    romalrc: lyric?.romalrc ? { lyric: lyric.romalrc } : null,
  };

  const looksTtml = lyric?.kind === "ttml" || raw.startsWith("<");
  if (looksTtml) {
    try {
      const { parseTTML } = await import("@applemusic-like-lyrics/lyric");
      const lines = parseTTML(raw)?.lines ?? [];
      if (!lines.length) return null;
      return {
        code: 200,
        lrc: { lyric: linesToLrc(lines) },
        hasTTML: true,
        ttml: lines,
        ...companions,
      };
    } catch (err) {
      console.warn("[Player] local TTML could not be parsed, ignoring:", err);
      return null;
    }
  }

  const wordFormat = detectWordTimedLyricFormat(raw);
  if (lyric?.kind === "word" || wordFormat) {
    try {
      const amll = await import("@applemusic-like-lyrics/lyric");
      const parse =
        wordFormat === "qrc"
          ? amll.parseQrc
          : wordFormat === "eslrc"
            ? amll.parseEslrc
            : amll.parseYrc;
      const lines = parse(raw) ?? [];
      // A file labelled word-timed whose content is not: fall through to LRC
      // rather than showing nothing.
      if (lines.length) {
        return {
          code: 200,
          lrc: { lyric: linesToLrc(lines) },
          yrc: { lyric: raw },
          ...companions,
        };
      }
    } catch (err) {
      console.warn("[Player] local word-timed lyric could not be parsed:", err);
    }
  }

  return { code: 200, lrc: { lyric: raw }, ...companions };
};

/** Flatten parsed word-timed lines back into plain LRC text. */
const linesToLrc = (lines) =>
  lines
    .filter((line) => line.words?.length)
    .map((line) => {
      const timeMs = line.words[0].startTime;
      const minutes = String(Math.floor(timeMs / 60000)).padStart(2, "0");
      const seconds = ((timeMs % 60000) / 1000).toFixed(2).padStart(5, "0");
      return `[${minutes}:${seconds}]${line.words.map((word) => word.word).join("")}`;
    })
    .join("\n");

/**
 * A local track's data changed under us — a tag correction, a picked cover, an
 * imported lyric — and the playing state has to follow.
 *
 * Nothing else re-reads it. A queued row is a whole `SongData` snapshot taken when
 * it was queued, and `refreshRows` runs once, from `App.vue` at startup; the mini
 * player, the queue panel and every slave window render off that snapshot.
 *
 * Split by kind because the two halves have nothing in common: a lyric import does
 * not touch the index row (the import is stored beside it), and a cover or tag
 * change does not touch the loaded lyric.
 */
const applyLocalTrackChange = async (change) => {
  // A track that did not exist a moment ago is neither queued nor playing, so
  // there is nothing on this side to follow. The list views are the ones that care,
  // and `store/localLibrary` handles them.
  if (change.kind === "added") return;

  const keys = new Set(change.keys);

  if (change.kind === "lyric") {
    // Only the playing track's lyric is ever loaded, so it is the only one to redo.
    // `setPlaySongLyric` then reaches the slave windows through the existing
    // `watch(() => music.songLyric)`, which broadcasts.
    const playing = music.getPlaySongData;
    const uri = playing?.local?.uri;
    if (typeof uri === "string" && keys.has(uri)) await loadLocalLyric(playing);
    return;
  }

  // Tags and covers live on the row. Skip the round trip entirely when the edit
  // was made on a track that is not queued — the common case for a bulk album
  // apply, and for editing while listening to something else.
  const touchesQueue = music.persistData.playlists.some((song) => {
    const uri = song?.local?.uri;
    return typeof uri === "string" && keys.has(uri);
  });
  if (!touchesQueue) return;

  const refreshed = await localLibraryStore().refreshRows(music.persistData.playlists);
  // Replaced, not mutated: queue entries are `markRaw`'d (see `utils/rawEntry`), so
  // writing a field on one reaches no reactivity at all.
  if (refreshed !== music.persistData.playlists) {
    music.persistData.playlists = refreshed;
  }
  // The store watchers only fire on a *song id* change, and a local id is derived
  // from the path — an edit cannot change it. So both of these have to be explicit.
  broadcastPlayerState();
  // The OS media session resolves its metadata from the manifest, not the store.
  // Re-announcing the playing track is a no-op by design (`set_announcement`
  // refuses an identity that is already current, to keep the live projection from
  // stuttering backwards), so a republish is the only route in.
  publishNativeManifest({ force: true });
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
  transition: all var(--duration-300) cubic-bezier(0.65, 0.05, 0.36, 1);
}

.show-enter-from,
.show-leave-to {
  transform: translateY(80px);
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity var(--duration-150) var(--ease-in-out);
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
  --player-page-accent-rgb: var(--content-panel-accent-rgb, var(--app-shell-rgb, 242, 242, 244));
  --player-accent-color: color-mix(
    in srgb,
    var(--main-color) 72%,
    rgb(var(--player-page-accent-rgb)) 28%
  );
  --player-accent-strong: color-mix(
    in srgb,
    var(--main-color) 58%,
    rgb(var(--player-page-accent-rgb)) 42%
  );
  --player-rail-color: color-mix(
    in srgb,
    rgb(var(--player-page-accent-rgb)) 22%,
    var(--n-border-color) 78%
  );
  // 外壳底色保持素色，与 sidebar / QueuePanel 完全一致(var(--app-shell-bg))，不再叠加封面取色，
  // 让 sidebar · 播放栏 · 队列 连成同一块连续的中性外壳。
  --player-surface-bg: var(--app-shell-bg, var(--layout-bg, #fff));
  --player-surface-border: var(--acrylic-border, rgba(0, 0, 0, 0.06));
  --player-time-chip-bg: var(--app-shell-bg, var(--layout-bg, #fff));
  --player-time-chip-border: var(--n-border-color);
  --player-data-edge-inset: 14px;
  --player-control-edge-inset: 14px;
  --player-slider-edge-inset: 0px;

  height: var(--app-player-bar-height);
  position: fixed;
  bottom: 0;
  left: var(--sidebar-width, 240px);
  width: calc(100% - var(--sidebar-width, 240px) - var(--player-right-inset, 0px));
  z-index: 2;
  transition:
    left 0.22s ease-in-out,
    width 0.22s ease-in-out,
    opacity 0.18s ease,
    translate 0.22s ease;

  // Default surface for Web and mobile runtimes.
  background-color: var(
    --player-surface-bg,
    var(--app-shell-bg, var(--layout-bg, #fff))
  ) !important;
  border-top: 1px solid var(--player-surface-border);

  // Mobile: player sits above tab bar, no sidebar
  @media (max-width: 768px) {
    // Sit on top of the tab bar. --app-tab-bar-height already includes
    // --app-safe-area-bottom (the tab bar reserves the home indicator strip inside its
    // own box), so adding the inset again here would offset the mini player by a second
    // copy of it.
    bottom: var(--app-tab-bar-height);
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
    --player-slider-edge-inset: 0px;
    // The mini bar and the tab bar share one glass surface (.bottom-glass in App.vue),
    // so neither paints its own fill here — that shared layer is what removes the color
    // step and the divider line that used to sit between them.
    background-color: transparent !important;
    border: none !important;
    outline: none !important;
    box-shadow: none;
    overflow: visible !important;
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
          transition: all var(--duration-300) var(--ease-out);

          &:hover {
            color: var(--player-accent-color);
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
        color: var(--player-accent-color);
        cursor: pointer;
        padding: 4px;
        border-radius: var(--radius-pill);
        transform: scale(1);
        transition: all var(--duration-300) var(--ease-out);

        &:hover {
          color: var(--n-color-embedded);
          background-color: var(--player-accent-color);
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
        color: var(--player-accent-color);
        margin: 0 12px;
        cursor: pointer;
        transform: scale(1);
        transition: all var(--duration-300) var(--ease-out);
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
      color: var(--player-accent-color);
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
        transition: all var(--duration-300) var(--ease-out);

        @media (min-width: 640px) {
          &:hover {
            background-color: var(--player-accent-color);
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

        &:focus-visible {
          outline: none;
          border-radius: var(--radius-md);
          box-shadow: var(--focus-ring);
        }

        &.open {
          .n-icon {
            background-color: var(--player-accent-color);
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
          --n-fill-color: var(--player-accent-color);
          --n-fill-color-hover: var(--player-accent-color);
          --n-handle-color: var(--player-accent-color);
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
