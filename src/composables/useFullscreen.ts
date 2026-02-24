import { shallowRef, type Ref } from "vue";
import { FullscreenRound, FullscreenExitRound } from "@vicons/material";
import screenfull from "screenfull";
import gsap from "gsap";

export function useFullscreen(bigPlayerRef: Ref<HTMLElement | null>, onAfterToggle?: () => void) {
  const screenfullIcon = shallowRef(FullscreenRound);
  let timeOut: ReturnType<typeof setTimeout> | null = null;

  // Sync icon with actual fullscreen state (handles Escape key, programmatic exit, etc.)
  const onFullscreenChange = () => {
    screenfullIcon.value = screenfull.isFullscreen ? FullscreenExitRound : FullscreenRound;
  };

  if (screenfull.isEnabled) {
    screenfull.on("change", onFullscreenChange);
  }

  const screenfullChange = () => {
    if (!screenfull.isEnabled) return;

    // Read state before toggle for GSAP animation direction
    const wasFullscreen = screenfull.isFullscreen;
    screenfull.toggle();

    gsap.fromTo(
      bigPlayerRef.value,
      { scale: wasFullscreen ? 1.05 : 0.95 },
      { scale: 1, duration: 0.4, ease: "elastic.out(1, 0.5)", clearProps: "transform" },
    );

    // Icon will be updated by the fullscreenchange event listener

    timeOut = setTimeout(() => {
      onAfterToggle?.();
    }, 500);
  };

  const cleanup = () => {
    if (timeOut !== null) clearTimeout(timeOut);
    if (screenfull.isEnabled) {
      screenfull.off("change", onFullscreenChange);
    }
  };

  return { screenfullIcon, screenfullChange, cleanupFullscreen: cleanup };
}
