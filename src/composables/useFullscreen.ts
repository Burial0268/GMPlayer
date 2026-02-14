import { shallowRef, type Ref } from "vue";
import {
  FullscreenRound,
  FullscreenExitRound,
} from "@vicons/material";
import screenfull from "screenfull";
import gsap from "gsap";

export function useFullscreen(
  bigPlayerRef: Ref<HTMLElement | null>,
  onAfterToggle?: () => void
) {
  const screenfullIcon = shallowRef(FullscreenRound);
  let timeOut: ReturnType<typeof setTimeout> | null = null;

  const screenfullChange = () => {
    if (!screenfull.isEnabled) return;

    screenfull.toggle();

    gsap.fromTo(
      bigPlayerRef.value,
      { scale: screenfull.isFullscreen ? 1.05 : 0.95 },
      { scale: 1, duration: 0.4, ease: "elastic.out(1, 0.5)" }
    );

    screenfullIcon.value = screenfull.isFullscreen
      ? FullscreenRound
      : FullscreenExitRound;

    timeOut = setTimeout(() => {
      onAfterToggle?.();
    }, 500);
  };

  const cleanup = () => {
    if (timeOut !== null) clearTimeout(timeOut);
  };

  return { screenfullIcon, screenfullChange, cleanupFullscreen: cleanup };
}
