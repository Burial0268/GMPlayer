import { computed, onScopeDispose, ref, watch, type Ref } from "vue";
import { musicStore } from "@/store";
import { getSongPlayingTime } from "@/utils/timeTools";

// How far ahead of the scheduled crossfade the transition state starts, in the outgoing song's
// own time: waiting for the audio overlap itself started it too late to read as a lead-in.
const LEAD_SECONDS = 5;

/**
 * Whether the Apple Music style AutoMix transition state is on: the progress bar fills and runs a
 * forward light, the quality tag gives way to "过渡" and the remaining time reads ∞.
 *
 * It starts once a transition is scheduled, its incoming audio is buffered and the outgoing song is
 * within `LEAD_SECONDS` of its crossfade, and ends when the store commits the incoming song
 * (`finishing`) — after the mix, on both paths — which is the same instant the title and cover
 * switch. Waiting for the buffer keeps a mix that is about to fail from lighting up first.
 */
export function useAutoMixTransition() {
  const music = musicStore();
  return computed(() => {
    const { phase, incomingSongId, incomingReady, crossfadeStartTime } = music.autoMixState;
    if (phase === "crossfading") return true;
    return (
      phase === "waiting" &&
      incomingReady &&
      incomingSongId !== null &&
      crossfadeStartTime > 0 &&
      (music.getPlaySongTime.currentTime || 0) >= crossfadeStartTime - LEAD_SECONDS
    );
  });
}

const ELAPSED_TICK_MS = 250;

/**
 * The elapsed-time label, which keeps running through the transition.
 *
 * A mix can outlast the outgoing song's own length, and every layer clamps its time to that
 * length (the outgoing web sound simply ends there), so the label stopped while audio played on.
 * During the transition it adds play time for as long as the store's time stands still; any real
 * movement resets that, so ordinary playback reads exactly as before.
 */
export function useAutoMixElapsed(transitioning: Ref<boolean>) {
  const music = musicStore();
  const extra = ref(0);
  let lastSeen = -1;
  let lastTick = 0;
  let timer: ReturnType<typeof setInterval> | null = null;

  const stop = () => {
    if (timer !== null) clearInterval(timer);
    timer = null;
    extra.value = 0;
  };

  watch(
    transitioning,
    (on) => {
      stop();
      if (!on) return;
      lastSeen = music.getPlaySongTime.currentTime;
      lastTick = performance.now();
      timer = setInterval(() => {
        const now = performance.now();
        const time = music.getPlaySongTime.currentTime;
        if (time !== lastSeen) {
          lastSeen = time;
          extra.value = 0;
        } else if (music.getPlayState) {
          extra.value += (now - lastTick) / 1000;
        }
        lastTick = now;
      }, ELAPSED_TICK_MS);
    },
    { immediate: true },
  );
  onScopeDispose(stop);

  return computed(() =>
    extra.value > 0
      ? getSongPlayingTime(music.getPlaySongTime.currentTime + extra.value)
      : music.getPlaySongTime.songTimePlayed,
  );
}
