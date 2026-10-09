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

export function getMobileTauriAudioUiDelaySeconds(): number {
  return isMobileTauriAudioUiDelayEnabled() ? MOBILE_TAURI_AUDIO_UI_DELAY_SECONDS : 0;
}

export function applyMobileTauriAudioUiDelay(currentTime: number, duration = 0): number {
  const normalizedTime = Number.isFinite(currentTime) ? Math.max(0, currentTime) : 0;
  const delay = getMobileTauriAudioUiDelaySeconds();
  const displayTime = delay > 0 ? Math.max(0, normalizedTime - delay) : normalizedTime;

  return duration > 0 ? Math.min(displayTime, duration) : displayTime;
}

/**
 * The position a seek just moved the clock to, kept until the lyric clock has
 * caught up with it.
 *
 * The lyric view jumps to a tapped line at once, but the conversion below runs
 * `MOBILE_TAURI_AUDIO_UI_DELAY_SECONDS` behind the playback clock — so on the
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
  const natural = applyMobileTauriAudioUiDelay(playbackTime) + lyricOffsetMs / 1000;
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
