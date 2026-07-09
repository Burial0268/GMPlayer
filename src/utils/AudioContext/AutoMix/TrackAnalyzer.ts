/**
 * TrackAnalyzer - Offline audio analysis for AutoMix
 *
 * Thin main-thread wrapper that:
 *   1. Decodes audio using the GLOBAL AudioContext (no new contexts)
 *   2. Mixes to mono
 *   3. Transfers PCM data to a Web Worker (zero-copy)
 *   4. Worker performs all heavy computation off the main thread
 *   5. Falls back to yielding main-thread execution if Worker is unavailable
 *
 * Public API unchanged — `analyzeTrack()` and `spectralSimilarity()`.
 */

import { AudioContextManager } from "../AudioContextManager";
import type { BPMResult } from "./BPMDetector";

const IS_DEV = import.meta.env?.DEV ?? false;

declare const __GMPLAYER_TAURI_BUILD__: boolean;

// ─── Public types ──────────────────────────────────────────────────

export type OutroType =
  | "hard"
  | "fadeOut"
  | "reverbTail"
  | "silence"
  | "noiseEnd"
  | "slowDown"
  | "sustained"
  | "musicalOutro"
  | "loopFade";

export interface OutroAnalysis {
  outroType: OutroType;
  outroConfidence: number;
  /** Seconds from file end where music ends */
  musicalEndOffset: number;
  /** Seconds from start of track where crossfade should begin */
  suggestedCrossfadeStart: number;
  multibandEnergy: { low: number[]; mid: number[]; high: number[] };
  spectralFlux: number[];
  shortTermLoudness: number[];
  /** Seconds from start where tempo deceleration begins (slowDown only) */
  decelerationStart?: number;
  /** Seconds from start where sustained note/chord begins (sustained only) */
  sustainOnset?: number;
  /** Seconds from start where a distinct outro section begins (musicalOutro only) */
  outroSectionStart?: number;
  /** Detected loop period in seconds (loopFade only) */
  loopPeriod?: number;
}

export interface VolumeAnalysis {
  peak: number;
  rms: number;
  estimatedLUFS: number;
  gainAdjustment: number;
}

export interface EnergyAnalysis {
  energyPerSecond: number[];
  outroStartOffset: number;
  introEndOffset: number;
  averageEnergy: number;
  /** Seconds of silence (< -50dB RMS) at the end of the track */
  trailingSilence: number;
  /** True if the song ends with a gradual fade-out (vs abrupt ending) */
  isFadeOut: boolean;
}

export interface SpectralFingerprint {
  bands: number[] | Float32Array;
}

export interface IntroAnalysis {
  /** Seconds from start until energy consistently exceeds 50% of track average */
  quietIntroDuration: number;
  /** Seconds from start until energy consistently exceeds 80% of track average */
  energyBuildDuration: number;
  /** Average energy of first 20s relative to track average (0-1+) */
  introEnergyRatio: number;
  /** Multiband energy at 250ms windows (null if track too short or main-thread fallback) */
  multibandEnergy: { low: number[]; mid: number[]; high: number[] } | null;
}

export interface Phrase {
  start: number;
  end: number;
  index: number;
}

export interface PhraseAnalysis {
  phrases: Phrase[];
  /** Suggested mix-out phrase (outro → intro transition point) */
  mixOutPhrase: Phrase | null;
  /** Suggested mix-in phrase for incoming track */
  mixInPhrase: Phrase | null;
}

export type SongSectionKind =
  | "start"
  | "verse"
  | "chorus"
  | "bridge"
  | "breakdown"
  | "outro"
  | "silence";

export interface SongSection {
  sectionType: SongSectionKind;
  start: number;
  end: number;
  index: number;
  confidence: number;
  energy: number;
  vocalRisk: number;
  mixSuitability: number;
}

export interface SectionAnalysis {
  sections: SongSection[];
  confidence: number;
  /** Native Rust currently emits this; Web worker analysis may omit sections. */
  method: string;
}

export interface VocalActivityAnalysis {
  windowDuration: number;
  risk: number[];
  confidence: number;
  method: "multibandHeuristic" | string;
}

export interface MixPointCandidate {
  time: number;
  score: number;
  reason: string;
  sectionType?: SongSectionKind;
  vocalRisk: number;
  energy: number;
}

export interface MixPointAnalysis {
  candidates: MixPointCandidate[];
  selected?: MixPointCandidate | null;
}

export interface TrackAnalysis {
  volume: VolumeAnalysis;
  energy: EnergyAnalysis;
  bpm: BPMResult | null;
  fingerprint: SpectralFingerprint;
  outro: OutroAnalysis | null;
  intro: IntroAnalysis | null;
  phrases: PhraseAnalysis | null;
  sections?: SectionAnalysis | null;
  vocalActivity?: VocalActivityAnalysis | null;
  mixCandidates?: MixPointAnalysis | null;
  duration: number;
}

export interface AnalyzeOptions {
  analyzeBPM?: boolean;
}

interface AnalysisFetchResult {
  bytes: Uint8Array;
  responseUrl: string;
}

type TrackAnalyzerWorkerClient =
  typeof import("@/utils/AudioContext/AutoMix/TrackAnalyzerWorkerClient");

let workerClientPromise: Promise<TrackAnalyzerWorkerClient | null> | null = null;

function getWorkerClient(): Promise<TrackAnalyzerWorkerClient | null> {
  if (__GMPLAYER_TAURI_BUILD__) return Promise.resolve(null);
  if (!workerClientPromise) {
    workerClientPromise = import("@/utils/AudioContext/AutoMix/TrackAnalyzerWorkerClient");
  }
  return workerClientPromise;
}

// ─── Audio decoding (main thread, using global AudioContext) ───────

async function decodeBlob(blobUrl: string, ctx: AudioContext): Promise<AudioBuffer> {
  const response = await fetch(blobUrl, { credentials: "omit" });
  const arrayBuffer = await response.arrayBuffer();

  // Use callback form for maximum browser compatibility
  return new Promise<AudioBuffer>((resolve, reject) => {
    ctx.decodeAudioData(arrayBuffer, resolve, reject);
  });
}

/**
 * Mix AudioBuffer to mono Float32Array.
 */
function mixToMono(buffer: AudioBuffer): Float32Array {
  const length = buffer.length;
  const channels = buffer.numberOfChannels;

  if (channels === 1) {
    // Copy to avoid neutering the AudioBuffer's internal storage
    return new Float32Array(buffer.getChannelData(0));
  }

  const mono = new Float32Array(length);
  for (let ch = 0; ch < channels; ch++) {
    const data = buffer.getChannelData(ch);
    for (let i = 0; i < length; i++) {
      mono[i] += data[i];
    }
  }

  // Normalize
  const scale = 1 / channels;
  for (let i = 0; i < length; i++) {
    mono[i] *= scale;
  }

  return mono;
}

// ─── Main-thread fallback (yielding) ──────────────────────────────

function yieldToMain(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

/**
 * Fallback: run volume analysis on main thread.
 */
function analyzeVolumeFallback(data: Float32Array): VolumeAnalysis {
  let peak = 0;
  let sumSquares = 0;
  const length = data.length;

  for (let i = 0; i < length; i++) {
    const abs = data[i] < 0 ? -data[i] : data[i];
    if (abs > peak) peak = abs;
    sumSquares += data[i] * data[i];
  }

  const rms = Math.sqrt(sumSquares / length);
  const estimatedLUFS = rms > 0 ? 20 * Math.log10(rms / 0.707) - 0.691 : -70;
  const lufsOffset = -14 - estimatedLUFS;
  const rawGain = Math.pow(10, lufsOffset / 20);
  const gainAdjustment = Math.max(0.1, Math.min(3.0, rawGain));

  return { peak, rms, estimatedLUFS, gainAdjustment };
}

/**
 * Fallback: run energy analysis on main thread.
 */
function analyzeEnergyFallback(
  data: Float32Array,
  sampleRate: number,
  duration: number,
): EnergyAnalysis {
  const secondCount = Math.ceil(duration);
  const energyPerSecond: number[] = Array.from({ length: secondCount });
  const length = data.length;
  const SILENCE_THRESHOLD = 0.003; // ~-50dB

  for (let sec = 0; sec < secondCount; sec++) {
    const start = (sec * sampleRate) | 0;
    const end = Math.min(((sec + 1) * sampleRate) | 0, length);
    const count = end - start;
    if (count <= 0) {
      energyPerSecond[sec] = 0;
      continue;
    }

    let sumSq = 0;
    for (let i = start; i < end; i++) sumSq += data[i] * data[i];
    energyPerSecond[sec] = Math.sqrt(sumSq / count);
  }

  // Trailing silence detection (100ms windows, absolute threshold)
  const windowSamples = Math.min(Math.floor(sampleRate * 0.1), length);
  let trailingSilence = 0;

  if (windowSamples > 0) {
    for (let pos = length - windowSamples; pos >= 0; pos -= windowSamples) {
      let sumSq = 0;
      const winEnd = Math.min(pos + windowSamples, length);
      for (let i = pos; i < winEnd; i++) {
        sumSq += data[i] * data[i];
      }
      const rms = Math.sqrt(sumSq / (winEnd - pos));
      if (rms > SILENCE_THRESHOLD) {
        trailingSilence = (length - pos - windowSamples) / sampleRate;
        break;
      }
    }
    if (trailingSilence === 0 && energyPerSecond[0] < SILENCE_THRESHOLD) {
      trailingSilence = duration;
    }
    trailingSilence = Math.round(trailingSilence * 10) / 10;
  }

  // Normalize (excluding trailing silence)
  const contentSeconds = Math.max(1, secondCount - Math.floor(trailingSilence));

  let maxE = 0.001;
  for (let i = 0; i < contentSeconds; i++) {
    if (energyPerSecond[i] > maxE) maxE = energyPerSecond[i];
  }
  for (let i = 0; i < secondCount; i++) energyPerSecond[i] /= maxE;

  let sum = 0;
  for (let i = 0; i < contentSeconds; i++) sum += energyPerSecond[i];
  const averageEnergy = sum / contentSeconds;

  // Outro detection (from content end)
  const outroThreshold = averageEnergy * 0.3;
  const lastContentSecond = Math.max(0, contentSeconds - 1);
  let outroStartOffset = 8;
  for (let i = lastContentSecond; i >= Math.max(0, lastContentSecond - 45); i--) {
    if (energyPerSecond[i] > outroThreshold) {
      outroStartOffset = lastContentSecond - i + 1;
      break;
    }
  }
  outroStartOffset = Math.max(3, outroStartOffset + trailingSilence);

  // Intro detection
  const introThreshold = averageEnergy * 0.4;
  let introEndOffset = 2;
  for (let i = 0; i < Math.min(secondCount, 30); i++) {
    if (energyPerSecond[i] > introThreshold) {
      introEndOffset = i;
      break;
    }
  }
  introEndOffset = Math.max(0, Math.min(introEndOffset, 10));

  // Fade-out detection
  let isFadeOut = false;
  const outroContentDuration = outroStartOffset - trailingSilence;
  if (outroContentDuration > 5) {
    const fadeStartSec = Math.max(0, lastContentSecond - Math.floor(outroContentDuration));
    const fadeEndSec = Math.max(0, lastContentSecond - 1);
    const startEnergy = energyPerSecond[fadeStartSec] || 0;
    const endEnergy = energyPerSecond[fadeEndSec] || 0;
    if (startEnergy > 0.05 && endEnergy / startEnergy < 0.3) {
      const midSec = Math.floor((fadeStartSec + fadeEndSec) / 2);
      const midEnergy = energyPerSecond[midSec] || 0;
      if (midEnergy < startEnergy * 0.85 && midEnergy > endEnergy * 0.8) {
        isFadeOut = true;
      }
    }
  }

  return {
    energyPerSecond,
    outroStartOffset,
    introEndOffset,
    averageEnergy,
    trailingSilence,
    isFadeOut,
  };
}

/**
 * Main-thread fallback with yields between phases.
 */
async function analyzeOnMainThread(
  monoData: Float32Array,
  sampleRate: number,
  duration: number,
  _analyzeBPM: boolean,
): Promise<TrackAnalysis> {
  const volume = analyzeVolumeFallback(monoData);
  await yieldToMain();

  const energy = analyzeEnergyFallback(monoData, sampleRate, duration);
  await yieldToMain();

  // Compute intro from energy data
  const intro = analyzeIntroFallback(energy.energyPerSecond, energy.averageEnergy);

  // BPM skipped in fallback — too expensive for main thread
  // Fingerprint skipped in fallback — not critical
  // Phrases skipped in fallback — requires BPM
  return {
    volume,
    energy,
    bpm: null,
    fingerprint: { bands: Array.from({ length: 24 }, () => 0) },
    outro: null,
    intro,
    phrases: null,
    duration,
  };
}

/**
 * Fallback: run intro analysis on main thread (lightweight — just scans energyPerSecond).
 */
function analyzeIntroFallback(
  energyPerSecond: number[],
  averageEnergy: number,
): IntroAnalysis | null {
  const scanLen = Math.min(20, energyPerSecond.length);
  if (scanLen < 4) return null;

  const quietThreshold = averageEnergy * 0.5;
  let quietIntroDuration = scanLen;
  for (let i = 0; i < scanLen - 1; i++) {
    if (energyPerSecond[i] >= quietThreshold && energyPerSecond[i + 1] >= quietThreshold) {
      quietIntroDuration = i;
      break;
    }
  }

  const buildThreshold = averageEnergy * 0.8;
  let energyBuildDuration = scanLen;
  for (let i = 0; i < scanLen - 1; i++) {
    if (energyPerSecond[i] >= buildThreshold && energyPerSecond[i + 1] >= buildThreshold) {
      energyBuildDuration = i;
      break;
    }
  }

  let sum = 0;
  for (let i = 0; i < scanLen; i++) sum += energyPerSecond[i];
  const introEnergyRatio = averageEnergy > 0.001 ? sum / scanLen / averageEnergy : 1;

  return { quietIntroDuration, energyBuildDuration, introEnergyRatio, multibandEnergy: null };
}

// ─── Public API ────────────────────────────────────────────────────

/**
 * Perform full track analysis. Decodes on main thread, runs computation in Worker.
 * Falls back to main-thread execution with periodic yields if Worker is unavailable.
 */
export async function analyzeTrack(
  sourceUrl: string,
  options?: AnalyzeOptions,
): Promise<TrackAnalysis> {
  const analyzeBPM = options?.analyzeBPM ?? true;

  if (IS_DEV) {
    console.log("TrackAnalyzer: Starting analysis for", sourceUrl.substring(0, 50));
  }

  const workerClient = await getWorkerClient();
  const ctx = AudioContextManager.getContext();

  if (!ctx) {
    if (!workerClient?.hasAnalysisWorker()) {
      throw new Error("No AudioContext or analysis Worker available for decoding");
    }
    const fetchResult = await fetchAnalysisBytes(sourceUrl);
    const extension = extensionFromSrc(fetchResult.responseUrl) || extensionFromSrc(sourceUrl);
    return workerClient.analyzeBytesViaWorker(fetchResult.bytes, extension, analyzeBPM);
  }

  // Step 1: decode on main thread using global AudioContext when available.
  const buffer = await decodeBlob(sourceUrl, ctx);

  // Step 2: mix to mono
  const monoData = mixToMono(buffer);
  const sampleRate = buffer.sampleRate;
  const duration = buffer.duration;

  // Step 3: dispatch to Worker or fall back
  if (workerClient?.hasAnalysisWorker()) {
    return workerClient.analyzePcmViaWorker(monoData, sampleRate, duration, analyzeBPM);
  } else {
    if (IS_DEV) {
      console.log("TrackAnalyzer: Using main-thread fallback");
    }
    return analyzeOnMainThread(monoData, sampleRate, duration, analyzeBPM);
  }
}

async function fetchAnalysisBytes(sourceUrl: string): Promise<AnalysisFetchResult> {
  let lastError: unknown = null;
  for (const url of [sourceUrl, analysisProxyUrl(sourceUrl)]) {
    if (!url) continue;
    const attempts = buildAnalysisFetchAttempts(url);
    for (const attempt of attempts) {
      try {
        const response = await fetch(url, attempt.init);
        if (!response.ok) {
          lastError = new Error(
            `HTTP ${response.status} ${response.statusText || ""}`.trim() +
              ` (${attempt.label}, ${url === sourceUrl ? "direct" : "proxy"})`,
          );
          continue;
        }

        const buffer = await response.arrayBuffer();
        if (buffer.byteLength === 0) {
          lastError = new Error(
            `empty response (${attempt.label}, ${url === sourceUrl ? "direct" : "proxy"})`,
          );
          continue;
        }

        return {
          bytes: new Uint8Array(buffer),
          responseUrl: response.headers.get("x-audio-source-url") || response.url || sourceUrl,
        };
      } catch (err) {
        lastError = err;
      }
    }
  }

  const detail = lastError instanceof Error ? lastError.message : String(lastError);
  throw new Error(`unable to fetch audio for WASM analysis: ${detail}; url=${sourceUrl}`);
}

function buildAnalysisFetchAttempts(url: string): Array<{ label: string; init: RequestInit }> {
  if (isSameOriginUrl(url)) {
    return [
      {
        label: "range+credentials",
        init: { credentials: "include", headers: { Range: "bytes=0-" } },
      },
      {
        label: "range",
        init: { credentials: "same-origin", headers: { Range: "bytes=0-" } },
      },
      { label: "credentials", init: { credentials: "include" } },
      { label: "default", init: { credentials: "same-origin" } },
    ];
  }

  return [
    {
      label: "range",
      init: { credentials: "omit", headers: { Range: "bytes=0-" } },
    },
    { label: "default", init: { credentials: "omit" } },
  ];
}

function isSameOriginUrl(src: string): boolean {
  try {
    return new URL(src, self.location.href).origin === self.location.origin;
  } catch {
    return false;
  }
}

function analysisProxyUrl(src: string): string | null {
  try {
    const url = new URL(src, self.location.href);
    if (url.protocol !== "http:" && url.protocol !== "https:") return null;
    if (url.origin === self.location.origin) return null;
    return `/api/audio-proxy?url=${encodeURIComponent(url.href)}`;
  } catch {
    return null;
  }
}

function extensionFromSrc(src: string): string {
  const withoutHash = src.split("#", 1)[0] ?? src;
  const withoutQuery = withoutHash.split("?", 1)[0] ?? withoutHash;
  const name = withoutQuery.split(/[\\/]/).pop() ?? "";
  const dot = name.lastIndexOf(".");
  return dot >= 0 ? name.slice(dot + 1).toLowerCase() : "";
}

/**
 * Compute similarity between two spectral fingerprints (0-1, 1 = identical).
 * Runs on main thread — cheap O(24) operation.
 */
export function spectralSimilarity(fp1: SpectralFingerprint, fp2: SpectralFingerprint): number {
  const a = fp1.bands;
  const b = fp2.bands;
  if (a.length !== b.length) return 0;

  let dot = 0,
    n1 = 0,
    n2 = 0;
  for (let i = 0; i < a.length; i++) {
    dot += a[i] * b[i];
    n1 += a[i] * a[i];
    n2 += b[i] * b[i];
  }

  const denom = Math.sqrt(n1) * Math.sqrt(n2);
  return denom === 0 ? 0 : dot / denom;
}

/**
 * Terminate the analysis Worker. Call on app shutdown / hot reload.
 */
export function terminateAnalysisWorker(): void {
  void getWorkerClient().then((client) => client?.terminateAnalysisWorker());
}
