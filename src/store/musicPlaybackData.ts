import { acceptHMRUpdate, defineStore } from "pinia";
import { reactive } from "vue";
import { getSongPlayingTime } from "@/utils/timeTools";
import { applyMobileTauriAudioUiDelay } from "@/utils/tauri/audioUiDelay";
import useMusicPlaybackResumeStore from "./musicPlaybackResume";
import { createDefaultPlaySongTime, type PlaySongTime } from "./musicTypes";

export const useMusicPlaybackDataStore = defineStore("musicPlaybackData", () => {
  const resumeStore = useMusicPlaybackResumeStore();
  const checkpoint = resumeStore.session.playSongTime;
  const playSongTime = reactive<PlaySongTime>({
    ...createDefaultPlaySongTime(),
    ...checkpoint,
  });
  let lastFormattedSecond = -1;
  let lastFormattedDuration = -1;

  function setPlaySongTime(value: {
    currentTime: number;
    duration: number;
    displayCurrentTime?: number;
  }) {
    const fallbackCurrentTime = Number.isFinite(playSongTime.playbackCurrentTime)
      ? (playSongTime.playbackCurrentTime ?? 0)
      : Number.isFinite(playSongTime.currentTime)
        ? playSongTime.currentTime
        : 0;
    const previousDuration =
      Number.isFinite(playSongTime.duration) && playSongTime.duration > 0
        ? playSongTime.duration
        : 0;
    const incomingDuration =
      Number.isFinite(value.duration) && value.duration > 0 ? value.duration : 0;
    const duration = incomingDuration > 0 ? incomingDuration : previousDuration;
    const incomingCurrentTime = Number.isFinite(value.currentTime)
      ? Math.max(0, value.currentTime)
      : fallbackCurrentTime;
    const currentTime =
      duration > 0 ? Math.min(incomingCurrentTime, duration) : incomingCurrentTime;
    const incomingDisplayTime = Number.isFinite(value.displayCurrentTime)
      ? Math.max(0, value.displayCurrentTime ?? 0)
      : applyMobileTauriAudioUiDelay(currentTime, duration);
    const displayCurrentTime =
      duration > 0 ? Math.min(incomingDisplayTime, duration) : incomingDisplayTime;

    playSongTime.playbackCurrentTime = currentTime;
    playSongTime.currentTime = displayCurrentTime;
    playSongTime.duration = duration;
    playSongTime.barMoveDistance = duration === 0 ? 0 : (displayCurrentTime / duration) * 100;

    if (!Number.isNaN(playSongTime.barMoveDistance)) {
      const displayedSecond = Math.floor(displayCurrentTime);
      if (displayedSecond !== lastFormattedSecond) {
        lastFormattedSecond = displayedSecond;
        playSongTime.songTimePlayed = getSongPlayingTime(displayCurrentTime);
      }
      if (duration !== lastFormattedDuration) {
        lastFormattedDuration = duration;
        playSongTime.songTimeDuration = getSongPlayingTime(duration);
      }
    }
  }

  function resetPlaySongTime({ checkpoint = true }: { checkpoint?: boolean } = {}) {
    Object.assign(playSongTime, createDefaultPlaySongTime());
    lastFormattedSecond = -1;
    lastFormattedDuration = -1;
    if (checkpoint) resumeStore.clearSessionTime();
  }

  function checkpointPlaySongTime(force = false) {
    if (!force) return;
  }

  function getPlaySongPlaybackCurrentTime(): number {
    if (Number.isFinite(playSongTime.playbackCurrentTime)) {
      return Math.max(0, playSongTime.playbackCurrentTime ?? 0);
    }
    return Number.isFinite(playSongTime.currentTime) ? Math.max(0, playSongTime.currentTime) : 0;
  }

  return {
    playSongTime,
    setPlaySongTime,
    resetPlaySongTime,
    checkpointPlaySongTime,
    getPlaySongPlaybackCurrentTime,
  };
});

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useMusicPlaybackDataStore, import.meta.hot));
}

export default useMusicPlaybackDataStore;
