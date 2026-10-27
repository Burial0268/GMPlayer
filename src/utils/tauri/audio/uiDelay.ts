import { isTauri } from "../core/runtime";
import { isMobile, isMobileDevice } from "../platform/mobile";

export const MOBILE_TAURI_AUDIO_UI_DELAY_SECONDS = 0.45;

let cachedMobileTauri: boolean | null = null;
let mobileTauriPromise: Promise<boolean> | null = null;

async function resolveMobileTauri(): Promise<boolean> {
  const enabled = isTauri() && (await isMobile());
  cachedMobileTauri = enabled;
  return enabled;
}

export function primeMobileTauriAudioUiDelay(): void {
  if (cachedMobileTauri !== null || mobileTauriPromise) return;
  mobileTauriPromise = resolveMobileTauri().finally(() => {
    mobileTauriPromise = null;
  });
}

export function isMobileTauriAudioUiDelayEnabled(): boolean {
  if (!isTauri()) return false;

  if (isMobileDevice()) {
    cachedMobileTauri = true;
    return true;
  }

  if (cachedMobileTauri !== null) return cachedMobileTauri;
  primeMobileTauriAudioUiDelay();
  return false;
}

/**
 * How far the audible output trails the backend's position clock, as the
 * output stream measures it. It follows the device, so it moves when headphones
 * are connected or removed; `null` until a native stream has reported one.
 */
let measuredOutputLatencySeconds: number | null = null;

/** Above this the figure is a broken timestamp, not a device. */
const MAX_PLAUSIBLE_OUTPUT_LATENCY_SECONDS = 2;

export function setMeasuredOutputLatencyMs(latencyMs: number): void {
  const seconds = latencyMs / 1000;
  if (!Number.isFinite(seconds) || seconds <= 0 || seconds > MAX_PLAUSIBLE_OUTPUT_LATENCY_SECONDS) {
    return;
  }
  measuredOutputLatencySeconds = seconds;
}

/** The measurement when there is one, else the fixed mobile compensation. */
export function getAudioUiDelaySeconds(): number {
  if (measuredOutputLatencySeconds !== null) return measuredOutputLatencySeconds;
  return isMobileTauriAudioUiDelayEnabled() ? MOBILE_TAURI_AUDIO_UI_DELAY_SECONDS : 0;
}

export function applyAudioUiDelay(currentTime: number, duration = 0): number {
  const normalizedTime = Number.isFinite(currentTime) ? Math.max(0, currentTime) : 0;
  const delay = getAudioUiDelaySeconds();
  const displayTime = delay > 0 ? Math.max(0, normalizedTime - delay) : normalizedTime;

  return duration > 0 ? Math.min(displayTime, duration) : displayTime;
}

/**
 * The position a seek just moved the clock to, kept until the lyric clock has
 * caught up with it.
 *
 * The lyric view jumps to a tapped line at once, but the conversion below runs
 * the output delay behind the playback clock — so on the
 * very next frame it reads ~450 ms *before* the target and springs the view
 * back to the previous line, then forward again once the clock has advanced
 * past the delay. The hold pins the presentation at the target through that
 * window. It only ever raises the value and never touches the seek itself.
 */
let lyricSeekPresentationTarget: number | null = null;

export function armLyricSeekPresentationHold(targetSeconds: number): void {
  if (!Number.isFinite(targetSeconds) || targetSeconds < 0) return;
  lyricSeekPresentationTarget = targetSeconds;
}

/** Convert a raw playback position for lyrics only; never use it as a seek target. */
export function getLyricPresentationTimeSeconds(playbackTime: number, lyricOffsetMs = 0): number {
  const natural = applyAudioUiDelay(playbackTime) + lyricOffsetMs / 1000;
  const target = lyricSeekPresentationTarget;
  if (target === null || !Number.isFinite(playbackTime)) return natural;
  // Pinned while the clock sits at or just past the target and the delayed
  // presentation has not reached it yet — a clock paused there stays on the
  // tapped line.
  if (playbackTime >= target && natural < target) return target;
  // Anything else ends the hold: the delayed clock has arrived, or playback is
  // somewhere the seek did not put it (a track restarting at 0, a failed seek,
  // a later seek). Dropping it here keeps a spent target from pinning the same
  // position again in a later track.
  lyricSeekPresentationTarget = null;
  return natural;
}
