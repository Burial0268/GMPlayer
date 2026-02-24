<template>
  <!-- AMLL .thumb — 抽屉把手 -->
  <div class="mobile-thumb" @click="$emit('close')">
    <div class="handle-bar"></div>
  </div>

  <!-- AMLL .lyricLayout — Layer 2: 紧凑封面信息 + 歌词 -->
  <div class="mobile-lyric-layout">
    <div class="mobile-phony-small-cover" ref="phonySmallCoverRef" />
    <div class="mobile-small-controls">
      <div class="mobile-song-info">
        <div class="name-wrapper">
          <div class="name" :class="{ 'is-scrolling': isNameOverflow }">
            <span class="name-inner">{{ songName || $t("other.noSong") }}</span>
            <span class="name-inner" v-if="isNameOverflow">{{
              songName || $t("other.noSong")
            }}</span>
          </div>
        </div>
        <div class="artists text-hidden" v-if="artistList.length">
          <span v-for="(item, index) in artistList" :key="'s' + index">
            {{ item.name }}<span v-if="index != artistList.length - 1"> / </span>
          </span>
        </div>
      </div>
      <div class="mobile-header-actions">
        <n-icon
          size="24"
          :component="
            music.getPlaySongData && music.getSongIsLike(music.getPlaySongData.id)
              ? StarRound
              : StarBorderRound
          "
          @click.stop="
            music.getPlaySongData &&
            (music.getSongIsLike(music.getPlaySongData.id)
              ? music.changeLikeList(music.getPlaySongData.id, false)
              : music.changeLikeList(music.getPlaySongData.id, true))
          "
        />
        <n-icon size="24" :component="MoreVertRound" @click.stop="" />
      </div>
    </div>
    <div class="mobile-lyric" v-if="hasLyrics">
      <RollingLyrics
        @mouseenter="$emit('lrcMouseEnter')"
        @mouseleave="$emit('lrcAllLeave')"
        @lrcTextClick="$emit('lrcTextClick', $event)"
        class="mobile-lyrics"
      />
    </div>
    <div v-else class="no-lyrics"><span>¯\_(ツ)_/¯</span></div>
  </div>

  <!-- AMLL .noLyricLayout — Layer 1: 大封面 + 歌曲信息 + controls -->
  <div class="mobile-cover-layout">
    <div class="mobile-phony-big-cover" ref="phonyBigCoverRef" />
    <div class="mobile-big-controls">
      <!-- 歌曲信息（展开） -->
      <div class="mobile-song-info-row">
        <div class="mobile-song-info">
          <div class="name-wrapper" ref="nameWrapperRef">
            <div class="name" ref="nameTextRef" :class="{ 'is-scrolling': isNameOverflow }">
              <span class="name-inner">{{ songName || $t("other.noSong") }}</span>
              <span class="name-inner" v-if="isNameOverflow">{{
                songName || $t("other.noSong")
              }}</span>
            </div>
          </div>
          <div class="artists text-hidden" v-if="artistList.length">
            <span v-for="(item, index) in artistList" :key="'b' + index">
              {{ item.name }}<span v-if="index != artistList.length - 1"> / </span>
            </span>
          </div>
        </div>
        <div class="mobile-header-actions">
          <n-icon
            size="24"
            :component="
              music.getPlaySongData && music.getSongIsLike(music.getPlaySongData.id)
                ? StarRound
                : StarBorderRound
            "
            @click.stop="
              music.getPlaySongData &&
              (music.getSongIsLike(music.getPlaySongData.id)
                ? music.changeLikeList(music.getPlaySongData.id, false)
                : music.changeLikeList(music.getPlaySongData.id, true))
            "
          />
          <n-icon size="24" :component="MoreVertRound" @click.stop="" />
        </div>
      </div>
      <!-- 进度条 -->
      <div class="mobile-progress">
        <BouncingSlider
          :value="music.getPlaySongTime.currentTime || 0"
          :min="0"
          :max="music.getPlaySongTime.duration || 1"
          :is-playing="music.getPlayState"
          @update:value="handleProgressSeek"
          @seek-start="music.setPlayState(false)"
          @seek-end="music.setPlayState(true)"
        />
        <div class="time-display">
          <span>{{ music.getPlaySongTime.songTimePlayed }}</span>
          <span>-{{ remainingTime }}</span>
        </div>
      </div>
      <!-- 控制按钮 + 音量 -->
      <MobileControls @toComment="$emit('toComment')" />
    </div>
  </div>

  <!-- AMLL .coverFrame — 唯一 spring 动画元素 -->
  <MobileCoverFrame
    :visible="!!currentCoverStyle"
    :frameStyle="coverFrameStyle"
    :coverUrl="coverImageUrl500"
    @click="$emit('switchLayer')"
  />
</template>

<script setup lang="ts">
import { StarBorderRound, StarRound, MoreVertRound } from "@vicons/material";
import { ref } from "vue";
import { musicStore } from "@/store";
import RollingLyrics from "../RollingLyrics.vue";
import BouncingSlider from "../BouncingSlider.vue";
import MobileControls from "./MobileControls.vue";
import MobileCoverFrame from "./MobileCoverFrame.vue";

defineProps<{
  songName: string;
  artistList: { name: string }[];
  isNameOverflow: boolean;
  hasLyrics: boolean;
  remainingTime: string;
  coverImageUrl500: string;
  currentCoverStyle: object | null;
  coverFrameStyle: Record<string, string>;
  handleProgressSeek: (val: number) => void;
}>();

defineEmits<{
  close: [];
  switchLayer: [];
  lrcMouseEnter: [];
  lrcAllLeave: [];
  lrcTextClick: [time: number];
  toComment: [];
}>();

const music = musicStore();

// Expose phony refs and name refs for parent (cover frame composable + name overflow)
const phonyBigCoverRef = ref<HTMLElement | null>(null);
const phonySmallCoverRef = ref<HTMLElement | null>(null);
const nameWrapperRef = ref<HTMLElement | null>(null);
const nameTextRef = ref<HTMLElement | null>(null);

defineExpose({ phonyBigCoverRef, phonySmallCoverRef, nameWrapperRef, nameTextRef });
</script>

<style lang="scss" scoped>
// ── AMLL .thumb ──
.mobile-thumb {
  grid-row: thumb;
  justify-self: center;
  align-self: end;
  z-index: 1;
  mix-blend-mode: plus-lighter;
  cursor: pointer;
  width: 60px;
  height: 20px;
  display: flex;
  align-items: center;
  justify-content: center;

  .handle-bar {
    width: 36px;
    height: 5px;
    background: rgba(255, 255, 255, 0.3);
    border-radius: 3px;
    transition: background 0.2s ease;
  }

  &:active .handle-bar {
    background: rgba(255, 255, 255, 0.5);
  }
}

// ── AMLL .lyricLayout — Layer 2: 紧凑封面/信息 + 歌词 ──
.mobile-lyric-layout {
  grid-row: main-view;
  grid-column: 1 / 2;
  display: grid;
  grid-template-rows: 8px [controls] 0fr [lyric-view] 1fr;
  grid-template-columns: 16px [cover-side] 0fr [info-side] 1fr 16px;
  mix-blend-mode: plus-lighter;
}

// ── AMLL .noLyricLayout — Layer 1: 大封面 + controls ──
.mobile-cover-layout {
  grid-row: main-view;
  grid-column: 1 / 2;
  overflow-y: hidden;
  display: grid;
  grid-template-rows: 1em [cover-view] 1fr [controls-view] 0fr;
  grid-template-columns: 24px [main-view] 1fr 24px;
  pointer-events: none;
}

// ── AMLL .phonySmallCover ──
.mobile-phony-small-cover {
  grid-row: controls;
  grid-column: cover-side;
  justify-self: center;
  align-self: center;
  aspect-ratio: 1 / 1;
  opacity: 0;
  pointer-events: none;
  width: 56px;
}

// ── AMLL .smallControls — 紧凑歌曲信息 ──
.mobile-small-controls {
  grid-row: controls;
  grid-column: info-side;
  align-self: center;
  transition: opacity 0.25s 0.25s;
  padding-left: 12px;
  min-width: 0;
  overflow: visible;
  height: fit-content;
  z-index: 1;
  mix-blend-mode: plus-lighter;
  display: flex;
  align-items: center;
  justify-content: space-between;

  .mobile-song-info {
    flex: 1;
    min-width: 0;
    overflow: hidden;

    .name-wrapper {
      overflow: hidden;
      width: 100%;

      .name {
        display: flex;
        font-weight: 600;
        font-size: 0.95rem;
        color: var(--main-cover-color);
        margin-bottom: 2px;
        white-space: nowrap;

        .name-inner {
          flex-shrink: 0;
          padding-right: 3em;
        }

        &.is-scrolling {
          animation: marquee-scroll 12s linear infinite;
        }
      }
    }

    .artists {
      font-size: 0.75rem;
      opacity: 0.7;
      color: var(--main-cover-color);
    }
  }

  .mobile-header-actions {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-left: 12px;
    flex-shrink: 0;

    .n-icon {
      color: var(--main-cover-color);
      opacity: 0.8;
      cursor: pointer;

      &:active {
        opacity: 0.5;
      }
    }
  }
}

// ── AMLL .lyric — 歌词区域 ──
.mobile-lyric {
  grid-row: lyric-view;
  grid-column: 1 / -1;
  transition: opacity 0.5s 0.5s;
  opacity: 1;
  mix-blend-mode: plus-lighter;
  min-height: 0;
  mask-image: linear-gradient(transparent 0%, black 8%, black 100%);

  .mobile-lyrics {
    height: 100%;
    overflow-y: auto;
    padding: 0;
    -ms-overflow-style: none;
    scrollbar-width: none;

    &::-webkit-scrollbar {
      display: none;
    }
  }
}

.no-lyrics {
  grid-row: lyric-view;
  grid-column: 1 / -1;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: opacity 0.5s 0.5s;
  opacity: 1;

  span {
    font-size: 1rem;
    color: var(--main-cover-color);
    opacity: 0.5;
  }
}

// ── AMLL .phonyBigCover ──
.mobile-phony-big-cover {
  grid-row: cover-view;
  grid-column: 2 / 3;
  opacity: 0;
  pointer-events: none;
}

// ── AMLL .bigControls — 完整 controls ──
.mobile-big-controls {
  grid-row: controls-view;
  grid-column: 2 / 3;
  transition: opacity 0.5s;
  opacity: 0;
  mix-blend-mode: plus-lighter;
  min-width: 0;
  z-index: 2;
  text-shadow: 0 0 0.3em color-mix(in srgb, currentColor 15%, transparent);
  padding-bottom: calc(env(safe-area-inset-bottom) + 16px);

  // 歌曲信息（展开）
  .mobile-song-info-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;

    .mobile-song-info {
      flex: 1;
      min-width: 0;
      overflow: hidden;

      .name-wrapper {
        overflow: hidden;
        width: 100%;

        .name {
          display: flex;
          font-weight: 600;
          font-size: 1.2rem;
          color: var(--main-cover-color);
          margin-bottom: 4px;
          white-space: nowrap;

          .name-inner {
            flex-shrink: 0;
            padding-right: 3em;
          }

          &.is-scrolling {
            animation: marquee-scroll 12s linear infinite;
          }
        }
      }

      .artists {
        font-size: 0.9rem;
        opacity: 0.7;
        color: var(--main-cover-color);
      }
    }

    .mobile-header-actions {
      display: flex;
      align-items: center;
      gap: 16px;
      margin-left: 12px;
      flex-shrink: 0;

      .n-icon {
        color: var(--main-cover-color);
        opacity: 0.8;
        cursor: pointer;

        &:active {
          opacity: 0.5;
        }
      }
    }
  }

  .mobile-progress {
    width: 100%;
    margin-bottom: 16px;

    .time-display {
      display: flex;
      justify-content: space-between;
      margin-top: 8px;
      font-size: max(1.2vh, 0.7rem);
      opacity: 0.5;
      color: var(--main-cover-color);
    }
  }
}

@keyframes marquee-scroll {
  0% {
    transform: translateX(0);
  }

  100% {
    transform: translateX(-50%);
  }
}

// ═══ 状态切换 ═══
// These are controlled by the parent's .layer2-active class on .bplayer
// The parent sets opacity/pointer-events via its own scoped CSS
// But since these elements are NOW in this child component,
// we need to handle the default state here.
// The parent will use :deep() or we handle via props if needed.

// Default state (Layer 1 visible):
.mobile-small-controls {
  opacity: 0;
  transition: opacity 0.5s;
  pointer-events: none;
}

.mobile-cover-layout {
  pointer-events: auto;
}

.mobile-lyric,
.no-lyrics {
  opacity: 0;
  transition: opacity 0.5s;
  pointer-events: none;
}

.mobile-big-controls {
  opacity: 1;
}
</style>
