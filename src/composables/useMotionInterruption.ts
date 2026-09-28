import { onBeforeUnmount, onMounted } from "vue";

/** Geometry and motion preferences can change while a gesture owns the surface. */
export function useMotionInterruption(
  interrupt: () => void,
  { viewport = true }: { viewport?: boolean | "width" } = {},
) {
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  let layoutWidth = window.innerWidth;
  let visualWidth = window.visualViewport?.width;
  const resized = () => {
    const nextLayoutWidth = window.innerWidth;
    const nextVisualWidth = window.visualViewport?.width;
    const widthChanged = nextLayoutWidth !== layoutWidth || nextVisualWidth !== visualWidth;
    layoutWidth = nextLayoutWidth;
    visualWidth = nextVisualWidth;
    if (viewport !== "width" || widthChanged) interrupt();
  };
  onMounted(() => {
    if (viewport) {
      window.addEventListener("resize", resized);
      window.visualViewport?.addEventListener("resize", resized);
    }
    window.addEventListener("gmplayer-cancel-layer-motion", interrupt);
    reducedMotion.addEventListener("change", interrupt);
  });
  onBeforeUnmount(() => {
    if (viewport) {
      window.removeEventListener("resize", resized);
      window.visualViewport?.removeEventListener("resize", resized);
    }
    window.removeEventListener("gmplayer-cancel-layer-motion", interrupt);
    reducedMotion.removeEventListener("change", interrupt);
  });
}
