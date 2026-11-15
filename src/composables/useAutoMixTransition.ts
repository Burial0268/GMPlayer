import { computed } from "vue";
import { musicStore } from "@/store";

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
