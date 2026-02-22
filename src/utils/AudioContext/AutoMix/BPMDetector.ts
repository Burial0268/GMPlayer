/**
 * BPMDetector - Types and lightweight utilities
 *
 * All heavy BPM computation has been moved to the analysis Web Worker
 * (analysis-worker.ts). This module only exports types and the cheap
 * `findNearestBeat()` utility used by AutoMixEngine.
 */

/** BPM detection result (produced by the analysis Worker) */
export interface BPMResult {
  /** Detected BPM */
  bpm: number;
  /** Confidence (0-1) */
  confidence: number;
  /** Beat timestamps in seconds (relative to analysis window start) */
  beatGrid: number[];
  /** Offset of analysis window start from track beginning (seconds) */
  analysisOffset: number;
}

/**
 * Find the nearest beat to a given time position.
 * Returns the beat time, or the input time if no beats are available.
 * Runs on main thread — O(beatGrid.length), typically < 500 entries.
 */
export function findNearestBeat(
  beatGrid: number[],
  targetTime: number,
  analysisOffset: number
): number {
  if (beatGrid.length === 0) return targetTime;

  const relativeTarget = targetTime - analysisOffset;
  let nearestBeat = beatGrid[0];
  let minDist = Math.abs(relativeTarget - beatGrid[0]);

  for (let i = 1; i < beatGrid.length; i++) {
    const dist = Math.abs(relativeTarget - beatGrid[i]);
    if (dist < minDist) {
      minDist = dist;
      nearestBeat = beatGrid[i];
    }
  }

  return nearestBeat + analysisOffset;
}
