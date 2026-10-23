import { onActivated, onBeforeUnmount, onDeactivated, type ObjectDirective } from "vue";
import { animateMini } from "motion-v";
import { useLayerNavigation } from "@/utils/navigation";
import { layerMotion } from "@/utils/navigation/motion";
import { prefersReducedMotion } from "@/utils/reducedMotion";
import { useMotionInterruption } from "./useMotionInterruption";

/** Reveal newly available content once; cached content keeps its visible state. */
export function useContentIntro() {
  const navigation = useLayerNavigation();
  const running = new Map<HTMLElement, () => void>();
  let active = true;

  const finish = (element: HTMLElement) => running.get(element)?.();
  const finishAll = () => running.forEach((complete) => complete());
  const reveal = (element: HTMLElement) => {
    if (!active || !element.isConnected || prefersReducedMotion()) return;

    const opacity = element.style.getPropertyValue("opacity");
    const priority = element.style.getPropertyPriority("opacity");
    const animation = animateMini(
      element,
      { opacity: [0, Number(getComputedStyle(element).opacity)] },
      { duration: layerMotion.content, ease: layerMotion.ease },
    );
    const complete = () => {
      if (!running.delete(element)) return;
      animation.cancel();
      if (opacity) element.style.setProperty("opacity", opacity, priority);
      else element.style.removeProperty("opacity");
    };
    running.set(element, complete);
    void Promise.resolve(animation).then(complete);
  };

  const vContentIntro: ObjectDirective<HTMLElement> = {
    mounted: reveal,
    beforeUnmount: finish,
  };

  // A source must release its opacity before the shared transition takes ownership.
  const removeBefore = navigation.onBeforeNavigation(finishAll);
  // Scrollbar changes resize the viewport without invalidating an opacity-only intro.
  useMotionInterruption(finishAll, { viewport: false });
  onActivated(() => {
    active = true;
  });
  onDeactivated(() => {
    active = false;
    finishAll();
  });
  onBeforeUnmount(() => {
    removeBefore();
    finishAll();
  });

  return { vContentIntro };
}
