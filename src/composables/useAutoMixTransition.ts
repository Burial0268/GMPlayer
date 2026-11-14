import { computed } from "vue";
import { musicStore } from "@/store";

/**
 * Whether an AutoMix crossfade is audible right now, for the Apple Music style transition state:
 * the progress bar glows and runs indeterminate, the quality tag reads "过渡" and the remaining
 * time reads ∞. It ends when the store commits the incoming song (`finishing`), which is the
 * same instant the title and cover switch, so the two changes land together.
 */
export function useAutoMixTransition() {
  const music = musicStore();
  return computed(() => music.autoMixState.phase === "crossfading");
}
