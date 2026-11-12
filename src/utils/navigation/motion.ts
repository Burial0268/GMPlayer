import { prefersReducedMotion } from "@/utils/reducedMotion";

export const layerMotion = {
  shared: 0.42,
  artwork: 0.18,
  content: 0.22,
  decoration: 0.32,
  cardDetails: { hide: 0.12, show: 0.22, delay: 0.075 },
  page: 0.28,
  root: 0.18,
  // Fade plus a field growing from the Nav button. Entry decelerates into place and its rows
  // cascade in `stagger` apart; exit is quicker and accelerates away, as dismissals do.
  search: {
    enter: 0.4,
    exit: 0.22,
    stagger: 0.02,
    ease: [0.22, 1, 0.36, 1] as [number, number, number, number],
    exitEase: [0.4, 0, 1, 1] as [number, number, number, number],
  },
  desktop: 0.3,
  ease: [0.22, 1, 0.36, 1] as [number, number, number, number],
  sharedEase: [0.2, 0.8, 0.2, 1] as [number, number, number, number],
  // Near-critical damping keeps full-height surfaces from bouncing past their bounds.
  settle: {
    type: "spring",
    stiffness: 420,
    damping: 38,
    mass: 0.9,
    restDelta: 0.001,
    restSpeed: 0.02,
  } as const,
};

export const motionDuration = (duration: number) => (prefersReducedMotion() ? 0 : duration);

export function cancelLayerMotion() {
  window.dispatchEvent(new Event("gmplayer-cancel-layer-motion"));
}
