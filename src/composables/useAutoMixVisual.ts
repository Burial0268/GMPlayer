import { computed, effectScope, ref, watch, type ComputedRef, type Ref } from "vue";
import { musicStore } from "@/store";
import type { SongData } from "@/store/musicTypes";

/**
 * The incoming song of an AutoMix crossfade, and how far the mix has come, for UI that blends
 * from one song to the next in step with the audio instead of switching when it is over.
 *
 * Progress is the outgoing track's own position since the crossfade began, over the crossfade's
 * length: it stops with a pause, and it needs no prediction of where the backend actually started
 * the fade. It is written to `--automix-mix` on the root element every frame rather than kept in
 * a ref, so blending costs no re-render; `pastMidpoint` is the one reactive edge, for what can only
 * switch (a background that takes an image URL), at the equal-power point.
 *
 * The incoming song stays set through `finishing`, when the store already names it: the base
 * layer switches to it underneath an overlay already showing it, so the hand-off is invisible.
 */
type AutoMixVisual = {
  incoming: ComputedRef<SongData | null>;
  pastMidpoint: Ref<boolean>;
};

let shared: AutoMixVisual | null = null;

const setMix = (value: number) =>
  document.documentElement.style.setProperty("--automix-mix", value.toFixed(3));

export function useAutoMixVisual(): AutoMixVisual {
  if (shared) return shared;
  // Detached: one loop for the app, outliving whichever component asked first.
  shared = effectScope(true).run(() => {
    const music = musicStore();
    const pastMidpoint = ref(false);
    const incoming = computed<SongData | null>(() => {
      const { phase, incomingSongId } = music.autoMixState;
      if ((phase !== "crossfading" && phase !== "finishing") || incomingSongId === null)
        return null;
      return music.getPlaylists.find((song) => song.id === incomingSongId) ?? null;
    });

    let frame = 0;
    let from = 0;
    const tick = () => {
      const { phase, crossfadeDuration } = music.autoMixState;
      const progress =
        phase === "finishing"
          ? 1
          : Math.min(
              1,
              Math.max(
                0,
                (music.getPlaySongPlaybackCurrentTime() - from) / Math.max(0.1, crossfadeDuration),
              ),
            );
      setMix(progress);
      if (pastMidpoint.value !== progress >= 0.5) pastMidpoint.value = progress >= 0.5;
      frame = requestAnimationFrame(tick);
    };

    watch(incoming, (song, previous) => {
      if (song?.id === previous?.id) return;
      cancelAnimationFrame(frame);
      pastMidpoint.value = false;
      setMix(0);
      if (!song) return;
      from = music.getPlaySongPlaybackCurrentTime();
      frame = requestAnimationFrame(tick);
    });

    return { incoming, pastMidpoint };
  })!;
  return shared;
}
