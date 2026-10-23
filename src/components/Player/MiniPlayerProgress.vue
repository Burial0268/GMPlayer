<!--
  Mini-player progress bar.

  Deliberately a leaf: the playback clock ticks at 30Hz
  (PLAYBACK_PRESENTATION_INTERVAL_MS), and `Player/index.vue` is always
  mounted and ~1600 lines. Binding `barMoveDistance` up there re-rendered
  that whole template on every tick during playback. Reading the store here
  confines the per-tick invalidation to this component.

  `setSeek` takes `window.$player` directly rather than a prop, so the parent
  does not have to thread its `player` shallowRef down.
-->
<template>
  <div class="slider">
    <span>{{ music.getPlaySongTime.songTimePlayed }}</span>
    <vue-slider
      v-model="sliderPercent"
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
          {{ getSongPlayingTime((music.getPlaySongTime.duration / 100) * sliderPercent) }}
        </div>
      </template>
    </vue-slider>
    <span>{{ music.getPlaySongTime.songTimeDuration }}</span>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { musicStore, listenTogetherStore } from "@/store";
import { setSeek } from "@/utils/AudioContext/PlayerFunctions";
import { getSongPlayingTime } from "@/utils/timeTools";
import VueSlider from "vue-slider-component";
import "vue-slider-component/theme/default.css";

const music = musicStore();
const listenTogether = listenTogetherStore();

const isSliderDragging = ref(false);
const pendingSliderPercent = ref<number | null>(null);

const sliderPercent = computed({
  get: () => music.getPlaySongTime.barMoveDistance,
  set: (val) => {
    music.getPlaySongTime.barMoveDistance = val;
  },
});

const normalizeSliderPercent = (val: unknown): number | null => {
  const raw = Array.isArray(val) ? val[0] : val;
  const num = Number(raw);
  if (!Number.isFinite(num)) return null;
  return Math.max(0, Math.min(100, num));
};

const previewSliderTime = (percent: number) => {
  const duration = music.getPlaySongTime?.duration;
  if (!duration) return;
  const currentTime = (duration / 100) * percent;
  music.setPlaySongTime({
    currentTime,
    displayCurrentTime: currentTime,
    duration,
  });
};

const sliderDragStart = () => {
  isSliderDragging.value = true;
  pendingSliderPercent.value = normalizeSliderPercent(music.getPlaySongTime.barMoveDistance);
};

const sliderDragging = (val: unknown) => {
  const percent = normalizeSliderPercent(val);
  if (percent === null) return;
  pendingSliderPercent.value = percent;
  previewSliderTime(percent);
};

const sliderDragEnd = () => {
  isSliderDragging.value = false;
};

const songTimeSliderUpdate = (val: unknown) => {
  if (!music.getPlaySongTime?.duration) return;
  const percent = normalizeSliderPercent(
    isSliderDragging.value ? (pendingSliderPercent.value ?? val) : val,
  );
  if (percent === null) return;
  isSliderDragging.value = false;
  pendingSliderPercent.value = null;
  const currentTime = (music.getPlaySongTime.duration / 100) * percent;
  setSeek(window.$player, currentTime);
  // 一起听歌：发送进度跳转命令（房主和房客均可）
  if (listenTogether.isInRoom) {
    listenTogether.sendPlayCommand("seek", Math.floor(currentTime * 1000));
  }
};
</script>

<style lang="scss" scoped>
// Moved verbatim from Player/index.vue's `.player > .slider` block. Every value
// here comes from CSS custom properties defined on `.player`, which still
// inherit through the DOM into this child component.
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
      background-color: var(--player-time-chip-bg);
      outline: 1px solid var(--player-time-chip-border);
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
      background-color: var(--player-time-chip-bg);
      outline: 1px solid var(--player-time-chip-border);
      padding: 2px 8px;
      border-radius: var(--radius-pill);
    }

    :deep(.vue-slider-rail) {
      background-color: var(--player-rail-color);
      border-radius: var(--radius-pill);

      .vue-slider-process {
        background: linear-gradient(90deg, var(--player-accent-strong), var(--player-accent-color));
      }

      .vue-slider-dot {
        width: 12px !important;
        height: 12px !important;
      }

      .vue-slider-dot-handle-focus {
        box-shadow: 0px 0px 1px 2px var(--player-accent-color);
      }
    }
  }
}
</style>
