import { onBeforeUnmount, onMounted } from "vue";
import { animateMini } from "motion-v";
import { useLayerNavigation } from "@/utils/navigation";
import { layerMotion, motionDuration } from "@/utils/navigation/motion";

const EDGE = 24; // px from the left edge where a back swipe may start
const SLOP = 10; // px before the gesture commits to an axis
const COMMIT_RATIO = 0.35; // of the viewport width
const COMMIT_VELOCITY = 0.5; // px/ms, a flick commits even when short

export interface EdgeSwipeTarget {
  /** The page to drag, or undefined while a transition owns the host. */
  page: () => HTMLElement | undefined;
  /** Hand the drag to the pop animation so it continues from the finger. */
  commit: (page: HTMLElement, offset: number, style: string | null) => void;
}

/**
 * iOS/Android-style edge swipe → pop one page layer. The page follows the finger and the pop
 * continues from where it was released; a short, slow drag springs back. Horizontal panning is
 * left to us by `touch-action: pan-y` on the page host, so these listeners stay passive.
 */
export function useEdgeSwipeBack(target: EdgeSwipeTarget) {
  const navigation = useLayerNavigation();
  const mobile = window.matchMedia("(max-width: 768px)");
  let drag:
    | {
        id: number;
        x: number;
        y: number;
        page?: HTMLElement;
        style: string | null;
        axis?: "x";
        offset: number;
        time: number;
        velocity: number;
      }
    | undefined;
  let settle: { page: HTMLElement; animation: ReturnType<typeof animateMini> } | undefined;

  const stopSettle = () => {
    settle?.animation.cancel();
    settle?.page.style.removeProperty("transform");
    settle = undefined;
  };

  const down = (e: PointerEvent) => {
    drag = undefined;
    if (!mobile.matches || e.pointerType !== "touch" || e.clientX > EDGE) return;
    // Only plain pages: the player, queue and search layers own their gestures.
    if (navigation.current.value?.presentation !== "page") return;
    stopSettle();
    // A popped root page has nothing under it to reveal; it still goes back on release.
    const page = navigation.state.value.layers.length > 1 ? target.page() : undefined;
    drag = {
      id: e.pointerId,
      x: e.clientX,
      y: e.clientY,
      page,
      style: page?.getAttribute("style") ?? null,
      offset: 0,
      time: e.timeStamp,
      velocity: 0,
    };
  };

  const move = (e: PointerEvent) => {
    if (!drag || drag.id !== e.pointerId) return;
    const dx = e.clientX - drag.x;
    if (!drag.axis) {
      const dy = Math.abs(e.clientY - drag.y);
      if (Math.max(dx, dy) < SLOP) return;
      if (dx < dy * 1.5) return void (drag = undefined);
      drag.axis = "x";
      drag.page?.style.setProperty("will-change", "transform");
    }
    const offset = Math.max(0, dx);
    drag.velocity = (offset - drag.offset) / Math.max(1, e.timeStamp - drag.time);
    drag.offset = offset;
    drag.time = e.timeStamp;
    drag.page?.style.setProperty("transform", `translate3d(${offset}px, 0px, 0px)`);
  };

  const release = (e: PointerEvent, cancelled: boolean) => {
    if (!drag || drag.id !== e.pointerId) return;
    const { page, style, axis, offset, velocity } = drag;
    drag = undefined;
    if (!axis) return;
    page?.style.removeProperty("will-change");
    const commit =
      !cancelled &&
      offset > 0 &&
      (offset > window.innerWidth * COMMIT_RATIO || velocity > COMMIT_VELOCITY);
    if (commit) {
      if (page) target.commit(page, offset, style);
      if (navigation.closeTop()) return;
    }
    if (!page) return;
    const animation = animateMini(
      page,
      { transform: [`translate3d(${offset}px, 0px, 0px)`, "translate3d(0px, 0px, 0px)"] },
      { duration: motionDuration(layerMotion.page), ease: layerMotion.ease },
    );
    settle = { page, animation };
    void Promise.resolve(animation).then(() => {
      if (settle?.animation === animation) stopSettle();
    });
  };
  const up = (e: PointerEvent) => release(e, false);
  const cancel = (e: PointerEvent) => release(e, true);

  onMounted(() => {
    window.addEventListener("pointerdown", down, { passive: true });
    window.addEventListener("pointermove", move, { passive: true });
    window.addEventListener("pointerup", up, { passive: true });
    window.addEventListener("pointercancel", cancel, { passive: true });
  });
  onBeforeUnmount(() => {
    stopSettle();
    window.removeEventListener("pointerdown", down);
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", cancel);
  });
}
