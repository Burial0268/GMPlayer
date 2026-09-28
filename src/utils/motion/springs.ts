import type { Transition } from "motion-v";
import { prefersReducedMotion } from "@/utils/reducedMotion";

/**
 * Miuix (Xiaomi HyperOS / Folme) motion primitives, ported for motion-v.
 *
 * The visual base of this app is Apple HIG; these helpers add Miuix's
 * physically-based spring feel where a component wants it. CSS approximations
 * of the accelerate/decelerate curves live in global.scss as
 * `--ease-accelerate` / `--ease-decelerate`; use these for anything driven by
 * motion-v where a real spring reads better than a fixed-duration tween.
 */

export type EasingFn = (fraction: number) => number;

/**
 * miuix AccelerateEasing. factor = 1 → y = x²; factor > 1 exaggerates the ease-in.
 * Pass the result straight into motion-v's `transition.ease`.
 */
export function accelerateEasing(factor = 1): EasingFn {
  if (factor === 1) return (x) => x * x;
  const exp = 2 * factor;
  return (x) => x ** exp;
}

/**
 * miuix DecelerateEasing. factor = 1 → y = 1 - (1-x)²; factor > 1 exaggerates
 * the ease-out (miuix's Dialog dim uses factor = 1.5).
 */
export function decelerateEasing(factor = 1): EasingFn {
  if (factor === 1) return (x) => 1 - (1 - x) * (1 - x);
  const exp = 2 * factor;
  return (x) => 1 - (1 - x) ** exp;
}

/** miuix SinOutEasing: y = sin(x · π / 2). */
export const sinOutEasing: EasingFn = (x) => Math.sin((x * Math.PI) / 2);

/**
 * Convert Compose's `spring(dampingRatio, stiffness)` to a motion-v spring.
 * Compose's dampingRatio is normalized (≈0.6–1.0); motion-v wants raw damping.
 * For mass = 1: damping = 2 · dampingRatio · √stiffness.
 */
export function folmeSpring(dampingRatio: number, stiffness: number): Transition {
  return {
    type: "spring",
    stiffness,
    damping: 2 * dampingRatio * Math.sqrt(stiffness),
    mass: 1,
  };
}

/**
 * Convert miuix `folmeSpring(damping, response)` to a motion-v spring.
 * stiffness = (2π / response)².
 */
export function folmeSpringByResponse(damping: number, response: number): Transition {
  const stiffness = ((2 * Math.PI) / response) ** 2;
  return folmeSpring(damping, stiffness);
}

/**
 * Named presets mirroring Folme's feel: crisp, near-critical, no overshoot.
 * `response` is the perceived duration; smaller = snappier. Damping ≈1 avoids
 * the bouncy overshoot that would clash with the app's Apple-flavored surfaces.
 */
export const miuixSpring = {
  /** Control feedback — buttons, switches, small surfaces. */
  snappy: folmeSpringByResponse(0.9, 0.25),
  /** Standard surface transitions — cards, sheets, dialogs. */
  standard: folmeSpringByResponse(0.9, 0.35),
  /** Large / full-height surfaces — heavier, settles without bounce. */
  gentle: folmeSpringByResponse(1, 0.45),
} satisfies Record<string, Transition>;

/**
 * Collapse any spring to an instant settle when the user asks for reduced
 * motion. Mirrors `motionDuration()` in navigation/motion.ts for spring specs.
 */
export function withReducedMotion(transition: Transition): Transition {
  return prefersReducedMotion() ? { duration: 0 } : transition;
}
