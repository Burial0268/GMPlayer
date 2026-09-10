import { onBeforeUnmount, onMounted } from "vue";

/** Geometry and motion preferences can change while a gesture owns the surface. */
export function useMotionInterruption(
  interrupt: () => void,
  { viewport = true }: { viewport?: boolean } = {},
) {
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  onMounted(() => {
    if (viewport) {
      window.addEventListener("resize", interrupt);
      window.visualViewport?.addEventListener("resize", interrupt);
    }
    window.addEventListener("gmplayer-cancel-layer-motion", interrupt);
    reducedMotion.addEventListener("change", interrupt);
  });
  onBeforeUnmount(() => {
    if (viewport) {
      window.removeEventListener("resize", interrupt);
      window.visualViewport?.removeEventListener("resize", interrupt);
    }
    window.removeEventListener("gmplayer-cancel-layer-motion", interrupt);
    reducedMotion.removeEventListener("change", interrupt);
  });
}
