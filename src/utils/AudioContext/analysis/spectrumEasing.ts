import type { SpectrumFrame } from "./SpectrumFrame";

/**
 * Eases a spectrum display between backend frames.
 *
 * The native backend paces analysis frames to ~30 Hz (`ANALYSIS_FORWARD_INTERVAL` in
 * audio-backend `player/api.rs`) because on WebView2 every IPC message leaves memory behind.
 * Each call returns what to draw: the bars move from what is on screen to the newest frame over
 * the measured gap between frames, so they still move at display rate. A new frame is a new
 * array; an array filled in place keeps its identity and is drawn as-is.
 */
export function createSpectrumEaser() {
  let target: SpectrumFrame | null = null;
  let from: Float32Array | null = null;
  let shown: Float32Array | null = null;
  let start = 0;
  let gap = 1000 / 30;

  const frame = (next: SpectrumFrame, now: number): SpectrumFrame => {
    if (!next.length) {
      target = null;
      return next;
    }
    if (!shown || !from || shown.length !== next.length) {
      shown = new Float32Array(next.length);
      from = new Float32Array(next.length);
      target = null;
    }
    if (next !== target) {
      const fresh = target === null;
      if (!fresh) gap = Math.min(100, Math.max(8, gap * 0.8 + (now - start) * 0.2));
      from.set(shown);
      target = next;
      start = now;
      if (fresh) {
        shown.set(next as ArrayLike<number>);
        return shown;
      }
    }
    const t = (now - start) / gap;
    if (t >= 1) shown.set(next as ArrayLike<number>);
    else for (let i = 0; i < next.length; i++) shown[i] = from[i] + (next[i] - from[i]) * t;
    return shown;
  };

  /** Whether the display is still moving toward the newest frame. */
  const easing = (now: number): boolean => target !== null && now - start < gap;

  return { frame, easing };
}
