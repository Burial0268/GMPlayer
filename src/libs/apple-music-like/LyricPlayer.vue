<template>
  <div
    class="lyric-player-wrapper"
    :style="lyricStyles"
    ref="playerEl"
    @wheel.self="passToPlayer"
    @touchstart.self="passToPlayer"
    @touchmove.self="passToPlayer"
    @touchend.self="passToPlayer"
  />
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch, toRaw, shallowRef } from "vue";
import { musicStore, settingStore, siteStore } from "../../store";
import {
  LyricPlayer as CoreLyricPlayer,
  type DomLyricPlayer,
  type LyricLineMouseEvent,
} from "@applemusic-like-lyrics/core";
import { preprocessLyrics, getProcessedLyrics, type AMLLLine } from "@/utils/LyricsProcessor";
import "@applemusic-like-lyrics/core/style.css";

const site = siteStore();
const music = musicStore();
const setting = settingStore();

const playerEl = ref<HTMLElement | null>(null);
const lyricPlayerRef = shallowRef<DomLyricPlayer>();
const amllLyricLines = shallowRef<AMLLLine[]>([]);

const playState = computed(() => music.playState);

const currentTime = computed(() =>
  Math.round(music.getPlaySongPlaybackCurrentTime() * 1000 + (setting.lyricTimeOffset ?? 0)),
);

const alignAnchor = computed(() => (setting.lyricsBlock === "center" ? "center" : "top"));

const alignPosition = computed(() => (setting.lyricsBlock === "center" ? 0.5 : 0.2));

const mainColor = computed(() => {
  if (!setting.immersivePlayer) return "rgb(239, 239, 239)";
  return `var(--main-cover-mix-color, rgb(${site.songPicColor}))`;
});

const lyricStyles = computed(() => ({
  "--amll-lp-color": mainColor.value,
  "--amll-lyric-view-color": mainColor.value,
  "font-weight": setting.lyricFontWeight,
  "font-family": setting.lyricFont,
  "letter-spacing": setting.lyricLetterSpacing,
  cursor: "pointer",
  "-webkit-tap-highlight-color": "transparent",
}));

type SyncableSound = {
  seek?: () => unknown;
  duration?: () => unknown;
};

const emit = defineEmits<{
  "line-click": [evt: LyricLineMouseEvent];
  lrcTextClick: [time: number];
}>();

function getDomPlayer(): DomLyricPlayer | undefined {
  return lyricPlayerRef.value;
}

function lineClickStartTime(evt: LyricLineMouseEvent): number | undefined {
  const fromGetLine = evt.line?.getLine?.()?.startTime;
  if (typeof fromGetLine === "number") return fromGetLine;
  const fromLyricLine = (evt.line as { lyricLine?: { startTime: number } })?.lyricLine?.startTime;
  if (typeof fromLyricLine === "number") return fromLyricLine;
  return undefined;
}

function getWindowPlayer(): SyncableSound | undefined {
  return (window as Window & { $player?: SyncableSound }).$player;
}

function readPlaybackPosition(): number {
  const player = getWindowPlayer();
  const seekValue = player?.seek?.();
  return typeof seekValue === "number" && Number.isFinite(seekValue)
    ? seekValue
    : music.getPlaySongPlaybackCurrentTime();
}

function readCurrentPlaybackTime(): { currentTime: number; duration: number } {
  const player = getWindowPlayer();
  const durationValue = player?.duration?.();
  const currentTime = readPlaybackPosition();
  const duration =
    typeof durationValue === "number" && Number.isFinite(durationValue)
      ? durationValue
      : music.getPlaySongTime.duration;

  return { currentTime, duration };
}

function syncCurrentTimeFromPlayback() {
  const { currentTime, duration } = readCurrentPlaybackTime();

  if (Number.isFinite(currentTime)) {
    music.setPlaySongTime({ currentTime, duration });
  }

  const player = getDomPlayer();
  if (!player) return;

  const lyricTime = Math.round(currentTime * 1000 + (setting.lyricTimeOffset ?? 0));
  player.setCurrentTime(lyricTime, true);
  lastSubmittedLyricTime = lyricTime;
  player.resetScroll();
}

let syncFrameId = 0;
let lyricFrameId = 0;
let lastLyricFrameTime = -1;
let lastSubmittedLyricTime = -1;

function applyPlayerSettings(player: DomLyricPlayer) {
  player.setEnableBlur(setting.lyricsBlur);
  player.setHidePassedLines(setting.hidePassedLines);
  player.setEnableSpring(setting.showYrcAnimation);
  player.setEnableScale(setting.showYrcAnimation);
  player.setWordFadeWidth(0.5);
  player.setAlignAnchor(alignAnchor.value);
  player.setAlignPosition(alignPosition.value);
  player.setLinePosXSpringParams(setting.springParams.posX);
  player.setLinePosYSpringParams(setting.springParams.posY);
  player.setLineScaleSpringParams(setting.springParams.scale);
}

function applyLyricLines(lines: AMLLLine[]) {
  const player = getDomPlayer();
  if (!player) return;

  const time = currentTime.value;
  player.setLyricLines(lines, time);
  player.setCurrentTime(time, true);
  lastSubmittedLyricTime = time;
}

function updateLyricPlayer(frameTime: number) {
  const player = getDomPlayer();
  if (!player) return;

  const delta = lastLyricFrameTime < 0 ? 0 : frameTime - lastLyricFrameTime;
  lastLyricFrameTime = frameTime;

  if (playState.value) {
    const lyricTime = Math.round(readPlaybackPosition() * 1000 + (setting.lyricTimeOffset ?? 0));
    if (lyricTime !== lastSubmittedLyricTime) {
      player.setCurrentTime(lyricTime);
      lastSubmittedLyricTime = lyricTime;
    }
  }

  player.update(delta);
  lyricFrameId = requestAnimationFrame(updateLyricPlayer);
}

function applyPlayback(playing: boolean) {
  const player = getDomPlayer();
  if (!player) return;
  if (playing) player.resume();
  else player.pause();
}

function scheduleCurrentTimeSync() {
  if (syncFrameId) return;

  syncFrameId = requestAnimationFrame(() => {
    syncFrameId = 0;
    syncCurrentTimeFromPlayback();
  });
}

function handleVisibilityChange() {
  if (document.visibilityState === "visible") {
    scheduleCurrentTimeSync();
  }
}

const jumpSeek = (evt: LyricLineMouseEvent) => {
  const time = lineClickStartTime(evt);
  if (typeof time !== "number") return;
  const player = getDomPlayer();
  player?.setCurrentTime(time, true);
  lastSubmittedLyricTime = time;
  player?.resetScroll();
  emit("lrcTextClick", time / 1000);
  emit("line-click", evt);
};

function passToPlayer(event: Event) {
  const playerEl = getDomPlayer()?.getElement();
  if (!playerEl) return;
  playerEl.dispatchEvent(new (event.constructor as typeof Event)(event.type, event));
}

onMounted(() => {
  const host = playerEl.value;
  if (host) {
    const lyricPlayer = new CoreLyricPlayer();
    lyricPlayer.addEventListener("line-click", jumpSeek);
    const playerElement = lyricPlayer.getElement();
    playerElement.style.width = "100%";
    playerElement.style.height = "100%";
    host.appendChild(playerElement);
    lyricPlayerRef.value = lyricPlayer;
    applyPlayerSettings(lyricPlayer);
    applyPlayback(playState.value);
    applyLyricLines(amllLyricLines.value);
    lyricFrameId = requestAnimationFrame(updateLyricPlayer);
  }

  window.addEventListener("focus", scheduleCurrentTimeSync);
  window.addEventListener("pageshow", scheduleCurrentTimeSync);
  document.addEventListener("visibilitychange", handleVisibilityChange);

  nextTick(scheduleCurrentTimeSync);
});

onBeforeUnmount(() => {
  window.removeEventListener("focus", scheduleCurrentTimeSync);
  window.removeEventListener("pageshow", scheduleCurrentTimeSync);
  document.removeEventListener("visibilitychange", handleVisibilityChange);

  if (syncFrameId) {
    cancelAnimationFrame(syncFrameId);
    syncFrameId = 0;
  }

  if (lyricFrameId) {
    cancelAnimationFrame(lyricFrameId);
    lyricFrameId = 0;
  }

  lyricPlayerRef.value?.removeEventListener("line-click", jumpSeek);
  lyricPlayerRef.value?.dispose();
  lastLyricFrameTime = -1;
  lastSubmittedLyricTime = -1;
});

watch(
  () => playState.value,
  (playing) => applyPlayback(playing),
);

watch(
  () => [
    setting.lyricsBlur,
    setting.hidePassedLines,
    setting.showYrcAnimation,
    setting.lyricsBlock,
    setting.springParams.posX,
    setting.springParams.posY,
    setting.springParams.scale,
  ],
  () => {
    const player = getDomPlayer();
    if (player) applyPlayerSettings(player);
  },
  { deep: true },
);

watch(
  () => [music.songLyric, setting.showYrc, setting.showRoma, setting.showTransl],
  () => {
    const rawSongLyric = toRaw(music.songLyric);

    if (!rawSongLyric) {
      amllLyricLines.value = [];
      applyLyricLines([]);
      return;
    }

    try {
      preprocessLyrics(rawSongLyric, {
        showYrc: setting.showYrc,
        showRoma: setting.showRoma,
        showTransl: setting.showTransl,
      });
    } catch (error) {
      console.error("[LyricPlayer] 预处理歌词失败", error);
    }

    const lines = structuredClone(
      getProcessedLyrics(rawSongLyric, {
        showYrc: setting.showYrc,
        showRoma: setting.showRoma,
        showTransl: setting.showTransl,
      }),
    );
    amllLyricLines.value = lines;
    applyLyricLines(lines);
  },
  { immediate: true },
);
</script>

<style lang="scss" scoped>
.lyric-player-wrapper {
  width: 100%;
  height: 100%;
  touch-action: pan-y;
  container-type: size;
  overflow: hidden;
  position: relative;

  // The official Core playground inherits Tailwind's border-box preflight.
  // Keep the AMLL root's own box model, but match it for measured descendants
  // such as interlude dots whose percentage padding affects clientHeight.
  :deep(.amll-lyric-player *) {
    box-sizing: border-box;
  }
}
</style>
