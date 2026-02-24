/**
 * VocalActivityGuard — Mid-band vocal detection for crossfade deferral
 *
 * Uses existing multiband energy data (mid band = 300-4000Hz covers vocals)
 * to detect when vocals are active and defer crossfade start to avoid
 * cutting into vocal phrases.
 *
 * No new analysis needed — piggybacks on TrackAnalyzer's multiband energy.
 */

const IS_DEV = import.meta.env?.DEV ?? false;

/** Threshold for mid-band dominance indicating vocal activity */
const VOCAL_RATIO_THRESHOLD = 0.6;

/** Minimum consecutive quiet windows to consider a safe transition point */
const MIN_QUIET_WINDOWS = 2;

export class VocalActivityGuard {
  /**
   * Check if vocals are likely active at a given window index
   * based on mid-band energy dominance.
   *
   * @param multibandEnergy - { low, mid, high } arrays from TrackAnalyzer
   * @param windowIndex - Index into the multiband arrays (250ms windows)
   * @returns true if mid-band energy dominates (likely vocals)
   */
  isVocalActive(
    multibandEnergy: { low: number[]; mid: number[]; high: number[] },
    windowIndex: number,
  ): boolean {
    if (windowIndex < 0 || windowIndex >= multibandEnergy.mid.length) return false;

    const low = multibandEnergy.low[windowIndex] || 0;
    const mid = multibandEnergy.mid[windowIndex] || 0;
    const high = multibandEnergy.high[windowIndex] || 0;
    const total = low + mid + high;

    if (total < 0.001) return false; // silence

    return mid / total > VOCAL_RATIO_THRESHOLD;
  }

  /**
   * Find a safe transition point (non-vocal window) between startWindow and endWindow.
   * Scans forward from startWindow, looking for MIN_QUIET_WINDOWS consecutive
   * non-vocal windows.
   *
   * @returns Window index of safe point, or startWindow if none found
   */
  findSafeTransitionPoint(
    multibandEnergy: { low: number[]; mid: number[]; high: number[] },
    startWindow: number,
    endWindow: number,
    maxDeferWindows: number = 20, // 5s at 250ms windows
  ): number {
    const searchEnd = Math.min(endWindow, startWindow + maxDeferWindows);
    let consecutiveQuiet = 0;

    for (let i = startWindow; i < searchEnd; i++) {
      if (!this.isVocalActive(multibandEnergy, i)) {
        consecutiveQuiet++;
        if (consecutiveQuiet >= MIN_QUIET_WINDOWS) {
          return i - MIN_QUIET_WINDOWS + 1; // return start of quiet region
        }
      } else {
        consecutiveQuiet = 0;
      }
    }

    // No safe point found within budget — use original start
    return startWindow;
  }

  /**
   * Determine if crossfade should be deferred due to vocal activity.
   * Called as an additional check inside the energy gate deferral logic.
   *
   * @param currentTime - Current playback time in seconds
   * @param crossfadeStartTime - Planned crossfade start time in seconds
   * @param effectiveEnd - Effective content end time in seconds
   * @param outroMultibandEnergy - Outro multiband energy from TrackAnalyzer
   * @param crossfadeDuration - Planned crossfade duration in seconds
   * @returns true if crossfade should be deferred (vocals active at transition point)
   */
  shouldDeferForVocals(
    currentTime: number,
    crossfadeStartTime: number,
    effectiveEnd: number,
    outroMultibandEnergy: { low: number[]; mid: number[]; high: number[] } | undefined,
    crossfadeDuration: number,
  ): boolean {
    if (!outroMultibandEnergy) return false;

    // Convert crossfade start time to window index relative to outro data.
    // Outro multiband energy is analyzed from the last ~60s of content at 250ms windows.
    const windowDuration = 0.25; // 250ms per window
    const totalWindows = outroMultibandEnergy.mid.length;
    const outroStartTime = effectiveEnd - totalWindows * windowDuration;

    // If crossfade start is before outro data, can't check
    if (crossfadeStartTime < outroStartTime) return false;

    const startWindowIdx = Math.floor((crossfadeStartTime - outroStartTime) / windowDuration);
    if (startWindowIdx < 0 || startWindowIdx >= totalWindows) return false;

    // Check if vocals are active at the planned transition point
    const isVocal = this.isVocalActive(outroMultibandEnergy, startWindowIdx);
    if (!isVocal) return false;

    // Vocals are active — check if we have budget to defer
    const maxDeferTime = Math.min(crossfadeDuration * 0.5, 5);
    const maxDeferByRemaining = Math.max(0, effectiveEnd - crossfadeStartTime - 2);
    const actualMaxDefer = Math.min(maxDeferTime, maxDeferByRemaining);

    if (currentTime >= crossfadeStartTime + actualMaxDefer) return false;

    if (IS_DEV) {
      console.log(
        `VocalActivityGuard: Deferring crossfade — vocal activity detected at window ${startWindowIdx}, ` +
          `budget=${actualMaxDefer.toFixed(1)}s`,
      );
    }

    return true;
  }
}
