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
import { getLyricPresentationTimeSeconds } from "@/utils/tauri/audio/uiDelay";
import "@applemusic-like-lyrics/core/style.css";

const site = siteStore();
const music = musicStore();
const setting = settingStore();

const playerEl = ref<HTMLElement | null>(null);
const lyricPlayerRef = shallowRef<DomLyricPlayer>();
const amllLyricLines = shallowRef<AMLLLine[]>([]);

const playState = computed(() => music.playState);

const currentTime = computed(() => lyricPresentationTimeMs(music.getPlaySongPlaybackCurrentTime()));

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

function lyricPresentationTimeMs(playbackTime: number): number {
  return Math.round(
    getLyricPresentationTimeSeconds(playbackTime, setting.lyricTimeOffset ?? 0) * 1000,
  );
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

  const lyricTime = lyricPresentationTimeMs(currentTime);
  player.setCurrentTime(lyricTime, true);
  lastSubmittedLyricTime = lyricTime;
  player.resetScroll();
}

let syncFrameId = 0;
let lyricFrameId = 0;
let lastLyricFrameTime = -1;
let lastSubmittedLyricTime = -1;
// Park the AMLL spring/DOM work while the big player is closed: it slides
// off-screen via translateY(100%), so per-frame updates are invisible. The
// grace period covers the close transition; reopening resyncs via
// scheduleCurrentTimeSync (same path as returning from a hidden tab).
let lyricParkedSince = -1;
const LYRIC_PARK_DELAY_MS = 1000;

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

  if (!music.showBigPlayer) {
    if (lyricParkedSince < 0) lyricParkedSince = frameTime;
    if (frameTime - lyricParkedSince > LYRIC_PARK_DELAY_MS) {
      lyricFrameId = requestAnimationFrame(updateLyricPlayer);
      return;
    }
  } else {
    lyricParkedSince = -1;
  }

  if (playState.value) {
    const lyricTime = lyricPresentationTimeMs(readPlaybackPosition());
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

// Reopening the big player after a parked stretch: jump the lyric view to the
// live position instead of letting the spring animate across the gap.
watch(
  () => music.showBigPlayer,
  (visible) => {
    if (visible) scheduleCurrentTimeSync();
  },
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

    // No defensive copy: AMLL's setLyricLines() structuredClones the input into
    // its own currentLyricLines/processedLines before touching it, so the array
    // we hand over (and the processedLyrics cache it points into) is never
    // mutated from there.
    const lines = getProcessedLyrics(rawSongLyric, {
      showYrc: setting.showYrc,
      showRoma: setting.showRoma,
      showTransl: setting.showTransl,
    });
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
