import { onBeforeUnmount, onMounted } from "vue";
import { useLayerNavigation } from "@/utils/navigation";

const EDGE = 24; // px from the left edge where a back swipe may start
const DISTANCE = 72;

/** iOS/Android-style edge swipe → pop one page layer. Commits on release; no live drag. */
export function useEdgeSwipeBack() {
  const navigation = useLayerNavigation();
  const mobile = window.matchMedia("(max-width: 768px)");
  let start: { x: number; y: number; id: number } | undefined;

  const down = (e: PointerEvent) => {
    start = undefined;
    if (!mobile.matches || e.pointerType !== "touch" || e.clientX > EDGE) return;
    // Only plain pages: the player, queue and search layers own their gestures.
    if (navigation.current.value?.presentation !== "page") return;
    start = { x: e.clientX, y: e.clientY, id: e.pointerId };
  };
  const up = (e: PointerEvent) => {
    if (!start || start.id !== e.pointerId) return;
    const dx = e.clientX - start.x;
    const dy = Math.abs(e.clientY - start.y);
    start = undefined;
    if (dx > DISTANCE && dx > dy * 2) navigation.closeTop();
  };
  const cancel = () => (start = undefined);

  onMounted(() => {
    window.addEventListener("pointerdown", down, { passive: true });
    window.addEventListener("pointerup", up, { passive: true });
    window.addEventListener("pointercancel", cancel, { passive: true });
  });
  onBeforeUnmount(() => {
    window.removeEventListener("pointerdown", down);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", cancel);
  });
}
