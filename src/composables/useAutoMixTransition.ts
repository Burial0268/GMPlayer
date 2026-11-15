import { computed, onScopeDispose, ref, watch, type Ref } from "vue";
import { musicStore } from "@/store";
import { getSongPlayingTime } from "@/utils/timeTools";

/** Where the player bar is in an AutoMix transition. */
export type AutoMixBarPhase = "none" | "active" | "committed";

/**
 * The AutoMix transition state of the player bar: `active` highlights it (the bar keeps moving and
 * stays seekable) and swaps the quality tag for "过渡"; `committed` is the incoming song taking
 * over, which the bar answers with its hand-off animation. A transition that is cancelled (a seek,
 * a failure) goes straight back to `none`, so nothing announces a song change that did not happen.
 *
 * `active` starts once a transition is scheduled, its incoming audio is buffered and the outgoing
 * song has reached `leadInStart` (its analysed outro, bounded by the crossfade); `committed` lasts
 * the finishing hold, which on both paths begins at the instant the title and cover switch.
 */
export function useAutoMixBarPhase() {
  const music = musicStore();
  return computed<AutoMixBarPhase>(() => {
    const { phase, incomingSongId, incomingReady, crossfadeStartTime, leadInStart } =
      music.autoMixState;
    if (phase === "crossfading") return "active";
    if (phase === "finishing") return "committed";
    const leadIn =
      phase === "waiting" &&
      incomingReady &&
      incomingSongId !== null &&
      crossfadeStartTime > 0 &&
      (music.getPlaySongTime.currentTime || 0) >= leadInStart;
    return leadIn ? "active" : "none";
  });
}

/** Whether the transition state is on: the bar is highlighted and the tag reads "过渡". */
export function useAutoMixTransition(phase: Ref<AutoMixBarPhase> = useAutoMixBarPhase()) {
  return computed(() => phase.value === "active");
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
