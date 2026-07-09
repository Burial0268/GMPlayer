import type { TrackAnalysis } from "./TrackAnalyzer";

const DISABLED_WORKER_ERROR =
  "AutoMix web analysis worker is disabled in Tauri builds; native audio-backend analysis is required.";

export function hasAnalysisWorker(): boolean {
  return false;
}

export function analyzePcmViaWorker(): Promise<TrackAnalysis> {
  return Promise.reject(new Error(DISABLED_WORKER_ERROR));
}

export function analyzeBytesViaWorker(): Promise<TrackAnalysis> {
  return Promise.reject(new Error(DISABLED_WORKER_ERROR));
}

export function terminateAnalysisWorker(): void {}
