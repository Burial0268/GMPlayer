<template>
  <Transition name="show">
    <n-card
      v-show="music.getPlaylists[0] && music.showPlayBar"
      class="player"
      content-style="padding: 0"
      @click.stop="setting.bottomClick ? music.setBigPlayerState(true) : null"
    >
      <div class="slider">
        <span>{{ music.getPlaySongTime.songTimePlayed }}</span>
        <vue-slider
          v-model="music.getPlaySongTime.barMoveDistance"
          @drag-start="music.setPlayState(false)"
          @drag-end="sliderDragEnd"
          @click.stop="songTimeSliderUpdate(music.getPlaySongTime.barMoveDistance)"
          :tooltip="'active'"
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
          <div class="pic" @click.stop="music.setBigPlayerState(true)">
            <img
              :src="
                music.getPlaySongData
                  ? music.getPlaySongData.album.picUrl.replace(/^http:/, 'https:') + '?param=50y50'
                  : '/images/pic/default.png'
              "
              alt="pic"
            />
            <n-icon class="open" size="30" :component="KeyboardArrowUpFilled" />
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
                  <Transition name="fade" mode="out-in">
                    <n-text
                      v-if="
                        music.getPlaySongLyric?.lrc?.length &&
                        setting.showYrc &&
                        music.getPlaySongLyricIndex != -1 &&
                        music.getPlaySongLyric.hasYrc
                      "
                      :key="'yrc-' + music.getPlaySongLyricIndex"
                      class="lrc text-hidden"
                    >
                      <n-text
                        v-for="item in music.getPlaySongLyric.yrc[music.getPlaySongLyricIndex]
                          .content"
                        :key="item"
                        :depth="3"
                      >
                        {{ item.content }}
                      </n-text>
                    </n-text>
                    <n-text
                      v-else-if="
                        music.getPlaySongLyric?.lrc?.length && music.getPlaySongLyricIndex != -1
                      "
                      :key="'lrc-' + music.getPlaySongLyricIndex"
                      class="lrc text-hidden"
                      :depth="3"
                    >
                      {{ music.getPlaySongLyric.lrc[music.getPlaySongLyricIndex]?.content }}
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
              <div :class="music.showPlayList ? 'playlist open' : 'playlist'">
                <n-icon
                  size="30"
                  :component="PlaylistPlayRound"
                  @click.stop="music.showPlayList = !music.showPlayList"
                />
              </div>
            </template>
            {{ $t("general.name.playlists") }}
          </n-popover>
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
              {{ persistData.playVolume > 0 ? $t("general.name.mute") : $t("general.name.unmute") }}
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
  <!-- 播放器 -->
  <BigPlayer />
</template>

<script setup>
import {
  getMusicUrl,
  getMusicNumUrl,
  getMusicDetail,
  getUnifiedLyric,
} from "@/api/song";
import { Motion, AnimatePresence } from "motion-v";
import { NIcon } from "naive-ui";
import {
  KeyboardArrowUpFilled,
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
import { musicStore, settingStore } from "@/store";
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
import { debounce } from "throttle-debounce";
import { useI18n } from "vue-i18n";
import VueSlider from "vue-slider-component";
import AddPlaylist from "@/components/DataModal/AddPlaylist.vue";
import PlayListDrawer from "@/components/DataModal/PlayListDrawer.vue";
import AllArtists from "@/components/DataList/AllArtists.vue";
import BigPlayer from "./BigPlayer/index.vue";
import "vue-slider-component/theme/default.css";
import { watch } from "vue";
import { parseLyricData as parseLyric, formatAsLrc as formatToLrc } from "@/utils/LyricsProcessor";

const { t } = useI18n();
const router = useRouter();
const setting = settingStore();
const music = musicStore();
const { persistData } = storeToRefs(music);
const addPlayListRef = ref(null);
const PlayListDrawerRef = ref(null);

// UNM 是否存在
const useUnmServerHas = import.meta.env.VITE_UNM_API ? true : false;

// 音频标签
const player = ref(null);

// Async generation tracker — each getPlaySongData call increments this.
// Stale async callbacks (URL fetches, availability checks) compare their captured
// generation against the current value and bail out if a newer request superseded them.
let _songLoadGeneration = 0;

// 获取歌曲播放数据
const getPlaySongData = (data, level = setting.songLevel) => {
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
    if (autoMix.isCrossfading()) {
      console.log("[Player] AutoMix crossfade active, only fetching lyrics");
      // Sync player ref to the incoming sound set by AutoMix
      if (window.$player) {
        player.value = window.$player;
      }
      fetchAndParseLyric(id);
      return;
    }

    // Check audio preloader — if the next song was preloaded, use it directly
    const preloader = getAudioPreloader();
    const preloadedSound = preloader.consume(id);
    if (preloadedSound) {
      console.log(`[Player] Using preloaded audio for: ${id}`);
      player.value = createSound("", true, preloadedSound);
      // Preloaded sound is already loaded — 'load' event won't fire again,
      // so clear loading state immediately to avoid stuck spinner.
      music.isLoadingSong = false;
      fetchAndParseLyric(id);
      return;
    }

    // VIP 歌曲或需要购买专辑
    if (useUnmServerHas && setting.useUnmServer && !pc && (fee === 1 || fee === 4)) {
      console.log("[Player] Attempting UNM server for VIP/paid song.");
      getMusicNumUrlData(id, generation);
    }
    // 免费或无版权 — fetch URL directly (avoids checkMusicCanUse race condition
    // where the login session may not be established yet on page reload, causing
    // the availability check to fail and all songs to fallback to UNM).
    else {
      console.log(`[Player] Fetching official URL directly for ${id}.`);
      getMusicUrl(id, level)
        .then((res) => {
          if (generation !== _songLoadGeneration) return;
          console.log(`[Player] getMusicUrl response for ${id}:`, res);
          if (res.data && res.data[0] && res.data[0].url) {
            const url = res.data[0].url.replace(/^http:/, "https:");
            console.log(`[Player] Creating sound instance with official URL: ${url}`);
            player.value = createSound(url);
          } else {
            console.warn(`[Player] No official URL for ${id}, trying UNM fallback.`);
            if (useUnmServerHas && setting.useUnmServer) {
              getMusicNumUrlData(id, generation);
            } else {
              $message.warning(t("general.message.playError"));
              music.setPlaySongIndex("next");
            }
          }
        })
        .catch((err) => {
          if (generation !== _songLoadGeneration) return;
          console.error(`[Player] Error fetching official URL for ${id}:`, err);
          if (useUnmServerHas && setting.useUnmServer) {
            console.warn(`[Player] Official URL fetch failed for ${id}, falling back to UNM.`);
            getMusicNumUrlData(id, generation);
          } else {
            $message.warning(t("general.message.playError"));
            music.setPlaySongIndex("next");
          }
        });
    }
    // 获取歌词 (using the new unified function)
    fetchAndParseLyric(id);
  } catch (err) {
    console.error("[Player] Error in getPlaySongData main block:", err);
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

// 网易云解灰
const getMusicNumUrlData = (id, generation) => {
  console.log(`[Player] getMusicNumUrlData called for ID: ${id}`);
  getMusicNumUrl(id)
    .then((res) => {
      if (generation !== _songLoadGeneration) return;
      console.log(`[Player] getMusicNumUrl response for ${id}:`, res);
      if (res.code === 200 && res.data && res.data.url) {
        const songUrl = res.data.url.replace(/^http:/, "");
        // 匹配酷我域名
        const pattern = /kuwo\\.cn/i;
        if (pattern.test(songUrl) && res.data?.proxyUrl) {
          const proxyUrl = res.data.proxyUrl; // Assuming proxyUrl doesn't need https replace
          console.log(`[Player] Creating sound instance with UNM Proxy URL: ${proxyUrl}`);
          player.value = createSound(proxyUrl);
        } else {
          console.log(`[Player] Creating sound instance with UNM URL: ${songUrl}`);
          player.value = createSound(songUrl);
        }
      } else {
        console.error(`[Player] Invalid data from getMusicNumUrl for ${id}:`, res);
        $message.warning(t("general.message.playError"));
        music.setPlaySongIndex("next");
      }
    })
    .catch((err) => {
      if (generation !== _songLoadGeneration) return;
      console.error(`[Player] Error in getMusicNumUrl request for ${id}:`, err);
      $message.warning(t("general.message.playError"));
      music.setPlaySongIndex("next");
    });
};

// 歌曲进度条更新
const sliderDragEnd = () => {
  songTimeSliderUpdate(music.getPlaySongTime.barMoveDistance);
  music.setPlayState(true);
};
const songTimeSliderUpdate = (val) => {
  if (player.value && music.getPlaySongTime?.duration) {
    const currentTime = (music.getPlaySongTime.duration / 100) * val;
    setSeek(player.value, currentTime);
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

onMounted(() => {
  // 挂载方法
  window.$getPlaySongData = getPlaySongData;
  // 获取音乐数据
  if (music.getPlaylists[0] && music.getPlaySongData) {
    getPlaySongData(music.getPlaySongData);
  }
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
      if (!autoMix.isCrossfading()) {
        music.setPlaySongTime({ currentTime: 0, duration: 0 });
      }
      songChange(val);
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
    nextTick().then(() => {
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

const fetchAndParseLyric = (id) => {
  const useTTMLRepo = setting.useTTMLRepo;

  getUnifiedLyric(id, useTTMLRepo)
    .then((lyricData) => {
      console.log(
        `[Player] Unified Lyric data received for ${id} (using TTML repo: ${useTTMLRepo})`,
      );

      const parsedResult = parseLyric(lyricData);
      console.log(`[Player] Parsed lyric result for ${id}`);

      // 将解析后的歌词转换为标准LRC格式
      const formattedLrc = formatToLrc(parsedResult);
      console.log(`[Player] Formatted LRC for ${id}`);

      // 添加格式化后的LRC到结果中
      parsedResult.formattedLrc = formattedLrc;

      music.setPlaySongLyric(parsedResult);
    })
    .catch((err) => {
      console.error(`[Player] Failed to get unified lyric for ${id}:`, err);
      const defaultResult = parseLyric(null);
      defaultResult.formattedLrc = ""; // 确保默认结果也有formattedLrc字段
      music.setPlaySongLyric(defaultResult);
    });
};
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
  transition: opacity 0.3s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

.player {
  height: 70px;
  position: fixed;
  bottom: 0;
  left: 0;
  z-index: 2;

  .slider {
    position: absolute;
    top: -12px;
    left: 0;
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;

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
        border-radius: 25px;
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
        border-radius: 25px;
      }

      :deep(.vue-slider-rail) {
        background-color: var(--n-border-color);
        border-radius: 25px;

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

    .data {
      display: flex;
      flex-direction: row;
      align-items: center;

      .pic {
        width: 50px;
        height: 50px;
        min-width: 50px;
        border-radius: 8px;
        overflow: hidden;
        margin-right: 12px;
        position: relative;
        box-shadow: 0 6px 8px -2px rgb(0 0 0 / 16%);
        cursor: pointer;

        img {
          width: 100%;
          height: 100%;
          transform: scale(1);
          filter: blur(0) brightness(1);
          transition: all 0.3s;
        }

        .open {
          position: absolute;
          top: calc(50% - 15px);
          left: calc(50% - 15px);
          width: 30px;
          height: 30px;
          color: #fff;
          opacity: 0;
          transform: scale(0.6);
          transition: all 0.3s;
        }

        &:hover {
          img {
            transform: scale(1.1);
            filter: blur(6px) brightness(0.8);
          }

          .open {
            opacity: 1;
            transform: scale(1);
          }
        }
      }

      .name {
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
          margin-top: 2px;
        }
      }
    }

    .control {
      display: flex;
      flex-direction: row;
      align-items: center;
      justify-content: center;

      .next,
      .prev,
      .dislike {
        color: var(--main-color);
        cursor: pointer;
        padding: 4px;
        border-radius: 50%;
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
        border-radius: 8px;
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
