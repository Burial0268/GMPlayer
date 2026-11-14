/**
 * Low-frequency volume transport.
 *
 * This value is produced by the per-frame analysis loop in `PlayerFunctions`
 * and consumed by the background renderer. It is deliberately NOT Pinia/Vue
 * reactive state, for reasons that are specific to this signal:
 *
 *  - **It is written at display refresh rate.** `startSpectrumUpdate`'s loop is
 *    driven by `requestAnimationFrame`, so on a 120/144Hz panel the producer
 *    runs 120-144x/s — not the 30Hz the old `LOW_FREQ_STORE_INTERVAL_MS`
 *    constant implied. That constant only skipped a write when the value had
 *    *also* barely moved, and music moves it constantly, so in practice the
 *    throttle almost never engaged during playback.
 *  - **Its only consumer cannot use updates faster than its own frame rate.**
 *    `BackgroundRender` forwards it to amll-core's `setLowFreqVolume`, which is
 *    a plain float assignment; the renderer then samples that field from its
 *    own rAF loop. Nothing downstream reacts to the write itself.
 *
 * Routing it through reactive state therefore paid a dependency trigger and an
 * effect run every frame to hand a number to something that just reads a field.
 * Worse, when it lived on a component's render path it invalidated the
 * component hosting the WebGL canvas, so each analysis frame queued vnode work
 * that competed with the renderer's own rAF tick.
 *
 * Quantizing here (rather than throttling on time) keeps the update maximally
 * prompt — a real jump propagates on the very next frame, with no scheduler
 * hop and no time-based delay — while collapsing the long runs of frames whose
 * value rounds into the same bucket the renderer would have drawn identically.
 */

/** Renderer-visible resolution. Matches the 0.01 rounding the consumer used. */
const QUANTUM = 100;

let currentValue = 1;
const listeners = new Set<(value: number) => void>();

/** Latest published value. */
export const getLowFreqVolume = (): number => currentValue;

/**
 * Publish a new value. No-ops when the quantized value is unchanged, so a
 * steady passage costs one comparison per frame and zero listener calls.
 */
export const publishLowFreqVolume = (value: number): void => {
  const safe = Number.isFinite(value) ? value : 0;
  const next = Math.round(safe * QUANTUM) / QUANTUM;
  if (next === currentValue) return;
  currentValue = next;
  for (const listener of listeners) listener(next);
};

/** Reset to the neutral value (e.g. when analysis stops). */
export const resetLowFreqVolume = (): void => {
  publishLowFreqVolume(1);
};

/**
 * Subscribe to changes. The listener is primed with the current value
 * immediately so a late subscriber does not wait for the next transient.
 * Returns an unsubscribe function.
 */
export const subscribeLowFreqVolume = (listener: (value: number) => void): (() => void) => {
  listeners.add(listener);
  listener(currentValue);
  return () => {
    listeners.delete(listener);
  };
};
