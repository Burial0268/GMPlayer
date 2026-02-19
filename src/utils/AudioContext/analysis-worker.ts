/**
 * Analysis Worker — runs all CPU-intensive audio analysis off the main thread.
 *
 * Receives raw mono PCM data (Float32Array) from the main thread and performs:
 *  - Volume analysis (peak, RMS, LUFS estimation, gain adjustment)
 *  - Energy analysis (per-second RMS, intro/outro detection)
 *  - BPM detection (time-domain onset envelope + autocorrelation — O(N), NOT spectral DFT)
 *  - Spectral fingerprint (bark-band energy from a few windows)
 *
 * All computation is self-contained — no external imports.
 */

// ─── Protocol types ────────────────────────────────────────────────

interface AnalysisRequest {
  type: 'analyze';
  id: number;
  /** Mono PCM samples (Transferable) */
  monoData: Float32Array;
  sampleRate: number;
  duration: number;
  analyzeBPM: boolean;
}

interface VolumeAnalysis {
  peak: number;
  rms: number;
  estimatedLUFS: number;
  gainAdjustment: number;
}

interface EnergyAnalysis {
  energyPerSecond: number[];
  outroStartOffset: number;
  introEndOffset: number;
  averageEnergy: number;
  /** Seconds of silence (< -50dB RMS) at the end of the track */
  trailingSilence: number;
  /** True if the song ends with a gradual fade-out (vs abrupt ending) */
  isFadeOut: boolean;
}

interface BPMResult {
  bpm: number;
  confidence: number;
  beatGrid: number[];
  analysisOffset: number;
}

interface SpectralFingerprint {
  /** Serialized as plain number[] for transfer (no Float32Array across postMessage) */
  bands: number[];
}

type OutroType = 'hard' | 'fadeOut' | 'reverbTail' | 'silence' | 'noiseEnd' | 'slowDown' | 'sustained' | 'musicalOutro' | 'loopFade';

interface OutroAnalysis {
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

interface AnalysisResponse {
  type: 'result';
  id: number;
  volume: VolumeAnalysis;
  energy: EnergyAnalysis;
  bpm: BPMResult | null;
  fingerprint: SpectralFingerprint;
  outro: OutroAnalysis | null;
  intro: IntroAnalysis | null;
  duration: number;
}

interface ErrorResponse {
  type: 'error';
  id: number;
  error: string;
}

// ─── Constants ─────────────────────────────────────────────────────

const TARGET_LUFS = -14;
const REFERENCE_RMS = 0.707;

const BPM_ANALYSIS_DURATION = 30; // seconds
const BPM_ANALYSIS_RATE = 11025;  // downsample target
const MIN_BPM = 60;
const MAX_BPM = 200;

// ─── Volume Analysis ───────────────────────────────────────────────

function analyzeVolume(data: Float32Array): VolumeAnalysis {
  const length = data.length;
  let peak = 0;
  let sumSquares = 0;

  for (let i = 0; i < length; i++) {
    const abs = data[i] < 0 ? -data[i] : data[i];
    if (abs > peak) peak = abs;
    sumSquares += data[i] * data[i];
  }

  const rms = Math.sqrt(sumSquares / length);
  const estimatedLUFS = rms > 0
    ? 20 * Math.log10(rms / REFERENCE_RMS) - 0.691
    : -70;

  const lufsOffset = TARGET_LUFS - estimatedLUFS;
  const rawGain = Math.pow(10, lufsOffset / 20);
  const gainAdjustment = Math.max(0.1, Math.min(3.0, rawGain));

  return { peak, rms, estimatedLUFS, gainAdjustment };
}

// ─── Energy Analysis ───────────────────────────────────────────────

/** Absolute silence threshold: ~-50dB RMS in linear amplitude */
const SILENCE_THRESHOLD = 0.003;

function analyzeEnergy(data: Float32Array, sampleRate: number, duration: number): EnergyAnalysis {
  const length = data.length;
  const secondCount = Math.ceil(duration);
  const energyPerSecond: number[] = new Array(secondCount);

  for (let sec = 0; sec < secondCount; sec++) {
    const start = (sec * sampleRate) | 0;
    const end = Math.min(((sec + 1) * sampleRate) | 0, length);
    const count = end - start;
    if (count <= 0) { energyPerSecond[sec] = 0; continue; }

    let sumSq = 0;
    for (let i = start; i < end; i++) {
      sumSq += data[i] * data[i];
    }
    energyPerSecond[sec] = Math.sqrt(sumSq / count);
  }

  // ── Trailing silence detection (absolute threshold) ──
  // Scan from the end backward using 100ms windows for sub-second precision.
  // Uses absolute RMS threshold (~-50dB) to find where actual audio content ends.
  const windowSamples = Math.min(Math.floor(sampleRate * 0.1), length); // 100ms
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
    // If we scanned the entire track and everything is silent
    if (trailingSilence === 0 && energyPerSecond[0] < SILENCE_THRESHOLD) {
      trailingSilence = duration;
    }
    // Round to 0.1s
    trailingSilence = Math.round(trailingSilence * 10) / 10;
  }

  // ── Normalize energy to 0-1 (for outro/intro detection) ──
  // Exclude trailing silence seconds from normalization to avoid skewing
  const contentSeconds = Math.max(1, secondCount - Math.floor(trailingSilence));

  let maxE = 0.001;
  for (let i = 0; i < contentSeconds; i++) {
    if (energyPerSecond[i] > maxE) maxE = energyPerSecond[i];
  }
  for (let i = 0; i < secondCount; i++) {
    energyPerSecond[i] /= maxE;
  }

  // Average energy (content portion only)
  let sum = 0;
  for (let i = 0; i < contentSeconds; i++) sum += energyPerSecond[i];
  const averageEnergy = sum / contentSeconds;

  // ── Outro detection ──
  // Scan from the last content second backward to find where energy drops.
  // outroStartOffset is measured from the content end (not file end).
  const outroThreshold = averageEnergy * 0.3;
  const lastContentSecond = Math.max(0, contentSeconds - 1);
  let outroStartOffset = 8; // default fallback

  for (let i = lastContentSecond; i >= Math.max(0, lastContentSecond - 45); i--) {
    if (energyPerSecond[i] > outroThreshold) {
      outroStartOffset = lastContentSecond - i + 1;
      break;
    }
  }
  // Add trailing silence so the offset is from the file end
  outroStartOffset = outroStartOffset + trailingSilence;
  // Minimum 3s from file end, no upper cap (let AutoMixEngine handle max)
  outroStartOffset = Math.max(3, outroStartOffset);

  // ── Intro detection ──
  const introThreshold = averageEnergy * 0.4;
  let introEndOffset = 2;
  for (let i = 0; i < Math.min(secondCount, 30); i++) {
    if (energyPerSecond[i] > introThreshold) {
      introEndOffset = i;
      break;
    }
  }
  introEndOffset = Math.max(0, Math.min(introEndOffset, 10));

  // ── Fade-out detection ──
  // A fade-out song has gradually decreasing energy in the outro region.
  // Compare energy at the start vs end of the outro content (excluding silence).
  // If the energy ratio drops below 0.3 over >5s, it's a fade-out.
  let isFadeOut = false;
  const outroContentDuration = outroStartOffset - trailingSilence;

  if (outroContentDuration > 5) {
    const fadeStartSec = Math.max(0, lastContentSecond - Math.floor(outroContentDuration));
    const fadeEndSec = Math.max(0, lastContentSecond - 1);
    const startEnergy = energyPerSecond[fadeStartSec] || 0;
    const endEnergy = energyPerSecond[fadeEndSec] || 0;

    if (startEnergy > 0.05 && endEnergy / startEnergy < 0.3) {
      // Verify it's a gradual decrease, not a cliff: check the midpoint
      const midSec = Math.floor((fadeStartSec + fadeEndSec) / 2);
      const midEnergy = energyPerSecond[midSec] || 0;
      // Midpoint energy should be between start and end (gradual slope)
      if (midEnergy < startEnergy * 0.85 && midEnergy > endEnergy * 0.8) {
        isFadeOut = true;
      }
    }
  }

  return { energyPerSecond, outroStartOffset, introEndOffset, averageEnergy, trailingSilence, isFadeOut };
}

// ─── BPM Detection (efficient time-domain approach) ────────────────

/**
 * Downsample mono data to target rate via nearest-neighbor.
 */
function downsample(data: Float32Array, srcRate: number, dstRate: number): Float32Array {
  if (srcRate === dstRate) return data;
  const ratio = srcRate / dstRate;
  const outLen = Math.floor(data.length / ratio);
  const out = new Float32Array(outLen);
  for (let i = 0; i < outLen; i++) {
    out[i] = data[(i * ratio) | 0];
  }
  return out;
}

/**
 * Extract a time window from the middle of the track.
 */
function extractWindow(data: Float32Array, sampleRate: number, windowSec: number): { samples: Float32Array; offsetSec: number } {
  const totalSamples = data.length;
  const windowSamples = Math.min((windowSec * sampleRate) | 0, totalSamples);
  const start = Math.max(0, ((totalSamples - windowSamples) / 2) | 0);
  return {
    samples: data.subarray(start, start + windowSamples),
    offsetSec: start / sampleRate,
  };
}

/**
 * Compute onset envelope using frame-level energy difference.
 * O(N) — simply computes RMS per hop and takes the half-wave rectified difference.
 * This replaces the O(N²) spectral flux DFT from the original implementation.
 */
function computeOnsetEnvelope(samples: Float32Array, hopSize: number, frameSize: number): Float32Array {
  const frameCount = Math.floor((samples.length - frameSize) / hopSize);
  if (frameCount <= 1) return new Float32Array(0);

  // Step 1: compute per-frame RMS energy — O(N)
  const energy = new Float32Array(frameCount);
  for (let f = 0; f < frameCount; f++) {
    const offset = f * hopSize;
    let sumSq = 0;
    for (let i = 0; i < frameSize; i++) {
      const s = samples[offset + i];
      sumSq += s * s;
    }
    energy[f] = Math.sqrt(sumSq / frameSize);
  }

  // Step 2: half-wave rectified first difference — O(frameCount)
  const envelope = new Float32Array(frameCount);
  envelope[0] = 0;
  for (let f = 1; f < frameCount; f++) {
    const diff = energy[f] - energy[f - 1];
    envelope[f] = diff > 0 ? diff : 0;
  }

  return envelope;
}

/**
 * Autocorrelation-based BPM detection on onset envelope.
 */
function detectBPMFromEnvelope(
  envelope: Float32Array,
  hopsPerSecond: number
): { bpm: number; confidence: number } {
  if (envelope.length < 2) return { bpm: 120, confidence: 0 };

  // Remove DC offset
  let mean = 0;
  for (let i = 0; i < envelope.length; i++) mean += envelope[i];
  mean /= envelope.length;

  const len = envelope.length;
  const centered = new Float32Array(len);
  for (let i = 0; i < len; i++) {
    centered[i] = envelope[i] - mean;
  }

  // Lag range corresponding to BPM range
  const minLag = Math.max(1, Math.floor(hopsPerSecond * 60 / MAX_BPM));
  const maxLag = Math.min(len - 1, Math.floor(hopsPerSecond * 60 / MIN_BPM));
  const correlations = new Float32Array(maxLag + 1);

  // Zero-lag autocorrelation (for confidence normalization)
  let zeroCorr = 0;
  for (let i = 0; i < len; i++) zeroCorr += centered[i] * centered[i];
  zeroCorr /= len;

  let maxCorr = 0;
  let bestLag = minLag;

  for (let lag = minLag; lag <= maxLag; lag++) {
    let sum = 0;
    const n = len - lag;
    for (let i = 0; i < n; i++) {
      sum += centered[i] * centered[i + lag];
    }
    correlations[lag] = sum / n;

    if (correlations[lag] > maxCorr) {
      maxCorr = correlations[lag];
      bestLag = lag;
    }
  }

  // Comb filter refinement
  const candidates = [
    bestLag,
    Math.round(bestLag / 2),
    bestLag * 2,
    Math.round(bestLag * 2 / 3),
    Math.round(bestLag * 3 / 2),
  ];

  let refinedLag = bestLag;
  let refinedScore = -Infinity;

  for (const c of candidates) {
    if (c < minLag || c > maxLag) continue;
    let combScore = 0;
    let combCount = 0;
    for (let mult = 1; mult <= 4; mult++) {
      const idx = c * mult;
      if (idx < correlations.length) {
        combScore += correlations[idx];
        combCount++;
      }
    }
    if (combCount > 0) combScore /= combCount;
    if (combScore > refinedScore) {
      refinedScore = combScore;
      refinedLag = c;
    }
  }

  const bpm = Math.round((hopsPerSecond * 60 / refinedLag) * 10) / 10;
  const confidence = zeroCorr > 0 ? Math.min(1, Math.max(0, maxCorr / zeroCorr)) : 0;

  return { bpm, confidence };
}

function generateBeatGrid(bpm: number, durationSec: number): number[] {
  const interval = 60 / bpm;
  const beats: number[] = [];
  for (let t = 0; t < durationSec; t += interval) {
    beats.push(t);
  }
  return beats;
}

function runBPMDetection(data: Float32Array, sampleRate: number, duration: number): BPMResult | null {
  if (duration < 5) return null;

  // Downsample to analysis rate
  const downsampled = downsample(data, sampleRate, BPM_ANALYSIS_RATE);

  // Extract 30s window from middle
  const { samples, offsetSec } = extractWindow(downsampled, BPM_ANALYSIS_RATE, BPM_ANALYSIS_DURATION);

  // Onset envelope (efficient time-domain approach)
  const hopSize = 256;
  const frameSize = 1024;
  const hopsPerSecond = BPM_ANALYSIS_RATE / hopSize;
  const envelope = computeOnsetEnvelope(samples, hopSize, frameSize);

  if (envelope.length === 0) return null;

  const { bpm, confidence } = detectBPMFromEnvelope(envelope, hopsPerSecond);
  const windowDuration = samples.length / BPM_ANALYSIS_RATE;
  const beatGrid = generateBeatGrid(bpm, windowDuration);

  return { bpm, confidence, beatGrid, analysisOffset: offsetSec };
}

// ─── Spectral Fingerprint ──────────────────────────────────────────

function computeFingerprint(data: Float32Array, sampleRate: number): SpectralFingerprint {
  const length = data.length;
  const windowSize = 2048;
  const windowCount = Math.min(8, Math.floor(length / windowSize));

  // 8 perceptual frequency bands
  const bandEdges: [number, number][] = [
    [20, 150],      // sub-bass
    [150, 400],     // bass
    [400, 800],     // low-mid
    [800, 1500],    // mid
    [1500, 3000],   // upper-mid
    [3000, 6000],   // presence
    [6000, 10000],  // brilliance
    [10000, Math.min(16000, sampleRate / 2 - 100)], // air
  ];

  const bandCount = bandEdges.length;
  const bands = new Array<number>(bandCount).fill(0);

  if (windowCount === 0) return { bands };

  const step = Math.floor(length / windowCount);

  for (let w = 0; w < windowCount; w++) {
    const offset = w * step;
    const end = Math.min(offset + windowSize, length);
    const wLen = end - offset;
    if (wLen < windowSize / 2) continue;

    // Create fresh IIR filter states for each window (independent measurements)
    for (let b = 0; b < bandCount; b++) {
      const [fLow, fHigh] = bandEdges[b];
      // Skip bands above Nyquist
      if (fLow >= sampleRate / 2) continue;
      const clampedHigh = Math.min(fHigh, sampleRate / 2 - 100);
      if (clampedHigh <= fLow) continue;

      const state = createIIRState(designBiquadBandpass(fLow, clampedHigh, sampleRate));

      // Warm up filter with first 64 samples (avoids transient)
      const warmupEnd = Math.min(offset + 64, end);
      for (let i = offset; i < warmupEnd; i++) {
        iirProcessSample(state, data[i]);
      }

      // Measure RMS energy in this band for this window
      let sumSq = 0;
      for (let i = offset; i < end; i++) {
        const filtered = iirProcessSample(state, data[i]);
        sumSq += filtered * filtered;
      }
      bands[b] += Math.sqrt(sumSq / wLen);
    }
  }

  // Average across windows and normalize to 0-1
  let maxB = 0.0001;
  for (let b = 0; b < bandCount; b++) {
    bands[b] /= windowCount;
    if (bands[b] > maxB) maxB = bands[b];
  }
  for (let b = 0; b < bandCount; b++) {
    bands[b] /= maxB;
  }

  return { bands };
}

// ─── IIR Filter Infrastructure ─────────────────────────────────────

interface BiquadCoeffs {
  b0: number; b1: number; b2: number;
  a1: number; a2: number;
}

/**
 * Design 2nd-order Butterworth bandpass biquad filter.
 * Uses bilinear transform: w0 = 2π × center / sampleRate, Q = center / bandwidth.
 */
function designBiquadBandpass(fLow: number, fHigh: number, sampleRate: number): BiquadCoeffs {
  const center = Math.sqrt(fLow * fHigh);
  const bandwidth = fHigh - fLow;
  const w0 = 2 * Math.PI * center / sampleRate;
  const Q = center / bandwidth;
  const alpha = Math.sin(w0) / (2 * Q);

  const a0 = 1 + alpha;
  return {
    b0: alpha / a0,
    b1: 0,
    b2: -alpha / a0,
    a1: (-2 * Math.cos(w0)) / a0,
    a2: (1 - alpha) / a0,
  };
}

/**
 * Design approximate K-weighting high-shelf biquad at ~2kHz.
 * Boosts high frequencies by ~4dB for perceptual loudness approximation.
 */
function designKWeightShelf(sampleRate: number): BiquadCoeffs {
  const f0 = 2000;
  const gainDB = 4;
  const w0 = 2 * Math.PI * f0 / sampleRate;
  const A = Math.pow(10, gainDB / 40);
  const alpha = Math.sin(w0) / (2 * 0.707); // Q = 0.707 (Butterworth)

  const a0 = (A + 1) - (A - 1) * Math.cos(w0) + 2 * Math.sqrt(A) * alpha;
  return {
    b0: (A * ((A + 1) + (A - 1) * Math.cos(w0) + 2 * Math.sqrt(A) * alpha)) / a0,
    b1: (-2 * A * ((A - 1) + (A + 1) * Math.cos(w0))) / a0,
    b2: (A * ((A + 1) + (A - 1) * Math.cos(w0) - 2 * Math.sqrt(A) * alpha)) / a0,
    a1: (2 * ((A - 1) - (A + 1) * Math.cos(w0))) / a0,
    a2: ((A + 1) - (A - 1) * Math.cos(w0) - 2 * Math.sqrt(A) * alpha) / a0,
  };
}

/** IIR filter state for Direct Form II transposed */
interface IIRState {
  coeffs: BiquadCoeffs;
  z1: number;
  z2: number;
}

function createIIRState(coeffs: BiquadCoeffs): IIRState {
  return { coeffs, z1: 0, z2: 0 };
}

/** Apply one sample through IIR filter (Direct Form II transposed). Returns filtered sample. */
function iirProcessSample(state: IIRState, x: number): number {
  const { b0, b1, b2, a1, a2 } = state.coeffs;
  const y = b0 * x + state.z1;
  state.z1 = b1 * x - a1 * y + state.z2;
  state.z2 = b2 * x - a2 * y;
  return y;
}

// ─── Multiband Outro Analysis ──────────────────────────────────────

/** Analysis window size: 250ms worth of windows */
const OUTRO_WINDOW_MS = 250;
/** Maximum analysis region: last 60s of content */
const OUTRO_ANALYSIS_SECONDS = 60;

/**
 * Single-pass multiband analysis of the outro region.
 * Computes 3-band energy, K-weighted loudness, and spectral flux in one pass.
 * O(N), ~5KB memory for window arrays.
 */
function analyzeOutroMultiband(
  data: Float32Array,
  sampleRate: number,
  duration: number,
  trailingSilence: number
): OutroAnalysis | null {
  const contentDuration = duration - trailingSilence;
  if (contentDuration < 10) return null; // too short for meaningful analysis

  const analysisDuration = Math.min(OUTRO_ANALYSIS_SECONDS, contentDuration);
  const windowSamples = Math.floor(sampleRate * OUTRO_WINDOW_MS / 1000);
  const totalWindows = Math.floor(analysisDuration * 1000 / OUTRO_WINDOW_MS);

  if (totalWindows < 4 || windowSamples < 1) return null;

  // Compute sample range: last `analysisDuration` seconds of content
  const contentEndSample = Math.floor((duration - trailingSilence) * sampleRate);
  const analysisStartSample = Math.max(0, contentEndSample - Math.floor(analysisDuration * sampleRate));

  // Design filters
  const lowState = createIIRState(designBiquadBandpass(20, 300, sampleRate));
  const midState = createIIRState(designBiquadBandpass(300, 4000, sampleRate));
  const highState = createIIRState(designBiquadBandpass(4000, Math.min(16000, sampleRate / 2 - 100), sampleRate));
  const kState = createIIRState(designKWeightShelf(sampleRate));

  // Warm up filters: run 200 samples before analysis region
  const warmupStart = Math.max(0, analysisStartSample - 200);
  for (let i = warmupStart; i < analysisStartSample; i++) {
    const s = data[i];
    iirProcessSample(lowState, s);
    iirProcessSample(midState, s);
    iirProcessSample(highState, s);
    iirProcessSample(kState, s);
  }

  // Allocate window arrays
  const low: number[] = new Array(totalWindows);
  const mid: number[] = new Array(totalWindows);
  const high: number[] = new Array(totalWindows);
  const loudness: number[] = new Array(totalWindows);
  const flux: number[] = new Array(totalWindows);

  let accumLow = 0, accumMid = 0, accumHigh = 0, accumK = 0;
  let sampleInWindow = 0;
  let windowIdx = 0;
  let prevLow = 0, prevMid = 0, prevHigh = 0;

  const endSample = Math.min(analysisStartSample + totalWindows * windowSamples, data.length);

  for (let i = analysisStartSample; i < endSample && windowIdx < totalWindows; i++) {
    const s = data[i];
    const fLow = iirProcessSample(lowState, s);
    const fMid = iirProcessSample(midState, s);
    const fHigh = iirProcessSample(highState, s);
    const fK = iirProcessSample(kState, s);

    accumLow += fLow * fLow;
    accumMid += fMid * fMid;
    accumHigh += fHigh * fHigh;
    accumK += fK * fK;
    sampleInWindow++;

    if (sampleInWindow >= windowSamples) {
      const rmsLow = Math.sqrt(accumLow / sampleInWindow);
      const rmsMid = Math.sqrt(accumMid / sampleInWindow);
      const rmsHigh = Math.sqrt(accumHigh / sampleInWindow);
      const meanSqK = accumK / sampleInWindow;

      low[windowIdx] = rmsLow;
      mid[windowIdx] = rmsMid;
      high[windowIdx] = rmsHigh;
      loudness[windowIdx] = meanSqK > 0 ? -0.691 + 10 * Math.log10(meanSqK) : -70;

      // Spectral flux: sum of half-wave-rectified band differences
      if (windowIdx === 0) {
        flux[windowIdx] = 0;
      } else {
        const dLow = rmsLow - prevLow;
        const dMid = rmsMid - prevMid;
        const dHigh = rmsHigh - prevHigh;
        flux[windowIdx] = (dLow > 0 ? dLow : 0) + (dMid > 0 ? dMid : 0) + (dHigh > 0 ? dHigh : 0);
      }

      prevLow = rmsLow;
      prevMid = rmsMid;
      prevHigh = rmsHigh;

      accumLow = 0; accumMid = 0; accumHigh = 0; accumK = 0;
      sampleInWindow = 0;
      windowIdx++;
    }
  }

  // If we didn't fill all windows, truncate
  const actualWindows = windowIdx;
  if (actualWindows < 4) return null;

  const lowArr = low.slice(0, actualWindows);
  const midArr = mid.slice(0, actualWindows);
  const highArr = high.slice(0, actualWindows);
  const loudnessArr = loudness.slice(0, actualWindows);
  const fluxArr = flux.slice(0, actualWindows);

  const result = classifyOutro(
    { low: lowArr, mid: midArr, high: highArr },
    fluxArr,
    loudnessArr,
    duration,
    trailingSilence,
    analysisDuration,
    actualWindows
  );

  return {
    ...result,
    multibandEnergy: { low: lowArr, mid: midArr, high: highArr },
    spectralFlux: fluxArr,
    shortTermLoudness: loudnessArr,
  };
}

// ─── New OutroType Detection Helpers ──────────────────────────────

/**
 * Detect repeating period in an array via autocorrelation.
 * Returns { period, confidence } where period is in array indices.
 */
function detectRepetitionPeriod(
  arr: number[],
  start: number,
  end: number
): { period: number; confidence: number } {
  const len = end - start;
  if (len < 8) return { period: 0, confidence: 0 };

  // Remove DC offset
  let mean = 0;
  for (let i = start; i < end; i++) mean += arr[i];
  mean /= len;

  // Autocorrelation for lags 2..len/2
  const minLag = 2;
  const maxLag = Math.floor(len / 2);
  let zeroCorr = 0;
  for (let i = start; i < end; i++) {
    const v = arr[i] - mean;
    zeroCorr += v * v;
  }
  if (zeroCorr < 1e-10) return { period: 0, confidence: 0 };

  let bestLag = minLag;
  let bestCorr = -Infinity;

  for (let lag = minLag; lag <= maxLag; lag++) {
    let sum = 0;
    for (let i = start; i < end - lag; i++) {
      sum += (arr[i] - mean) * (arr[i + lag] - mean);
    }
    const norm = sum / zeroCorr;
    if (norm > bestCorr) {
      bestCorr = norm;
      bestLag = lag;
    }
  }

  return { period: bestLag, confidence: Math.max(0, bestCorr) };
}

/**
 * Detect tempo deceleration (slowDown) via increasing inter-onset intervals.
 * Returns { detected, slope, r2 } where slope > 0 means slowing down.
 */
function detectSlowDown(
  flux: number[],
  start: number,
  end: number,
  medianFlux: number
): { detected: boolean; slope: number; r2: number } {
  // Find onset peaks: flux values > 1.5× median
  const threshold = medianFlux * 1.5;
  const peakIndices: number[] = [];
  for (let i = start + 1; i < end - 1; i++) {
    if (flux[i] > threshold && flux[i] >= flux[i - 1] && flux[i] >= flux[i + 1]) {
      peakIndices.push(i);
    }
  }

  if (peakIndices.length < 6) return { detected: false, slope: 0, r2: 0 };

  // Compute inter-onset intervals
  const intervals: number[] = [];
  for (let i = 1; i < peakIndices.length; i++) {
    intervals.push(peakIndices[i] - peakIndices[i - 1]);
  }

  // Linear regression: intervals[i] = a + b*i
  const n = intervals.length;
  let sumX = 0, sumY = 0, sumXY = 0, sumX2 = 0;
  for (let i = 0; i < n; i++) {
    sumX += i;
    sumY += intervals[i];
    sumXY += i * intervals[i];
    sumX2 += i * i;
  }
  const denom = n * sumX2 - sumX * sumX;
  if (denom === 0) return { detected: false, slope: 0, r2: 0 };

  const slope = (n * sumXY - sumX * sumY) / denom;
  const intercept = (sumY - slope * sumX) / n;
  const meanY = sumY / n;

  // R² calculation
  let ssTot = 0, ssRes = 0;
  for (let i = 0; i < n; i++) {
    const predicted = intercept + slope * i;
    ssRes += (intervals[i] - predicted) ** 2;
    ssTot += (intervals[i] - meanY) ** 2;
  }
  const r2 = ssTot > 0 ? 1 - ssRes / ssTot : 0;

  return {
    detected: slope > 0 && r2 > 0.3,
    slope,
    r2,
  };
}

/**
 * Compute average band ratio vector for a region.
 * Returns [lowRatio, midRatio, highRatio] normalized to sum=1.
 */
function computeLocalBandRatio(
  bands: { low: number[]; mid: number[]; high: number[] },
  start: number,
  end: number
): [number, number, number] {
  let sumLow = 0, sumMid = 0, sumHigh = 0;
  for (let i = start; i < end; i++) {
    sumLow += bands.low[i];
    sumMid += bands.mid[i];
    sumHigh += bands.high[i];
  }
  const total = sumLow + sumMid + sumHigh;
  if (total < 1e-10) return [0.33, 0.33, 0.33];
  return [sumLow / total, sumMid / total, sumHigh / total];
}

/**
 * Cosine similarity between two 3-element vectors.
 */
function cosineSimilarity3(a: [number, number, number], b: [number, number, number]): number {
  let dot = 0, n1 = 0, n2 = 0;
  for (let i = 0; i < 3; i++) {
    dot += a[i] * b[i];
    n1 += a[i] * a[i];
    n2 += b[i] * b[i];
  }
  const d = Math.sqrt(n1) * Math.sqrt(n2);
  return d === 0 ? 0 : dot / d;
}

// ─── Outro Classification ──────────────────────────────────────────

interface ClassificationResult {
  outroType: OutroType;
  outroConfidence: number;
  musicalEndOffset: number;
  suggestedCrossfadeStart: number;
  decelerationStart?: number;
  sustainOnset?: number;
  outroSectionStart?: number;
  loopPeriod?: number;
}

function classifyOutro(
  bands: { low: number[]; mid: number[]; high: number[] },
  flux: number[],
  loudness: number[],
  duration: number,
  trailingSilence: number,
  analysisDuration: number,
  windowCount: number
): ClassificationResult {
  const windowDuration = OUTRO_WINDOW_MS / 1000; // 0.25s

  // Compute aggregate stats
  const totalEnergy: number[] = new Array(windowCount);
  let avgEnergy = 0;
  for (let i = 0; i < windowCount; i++) {
    totalEnergy[i] = bands.low[i] + bands.mid[i] + bands.high[i];
    avgEnergy += totalEnergy[i];
  }
  avgEnergy /= windowCount;

  // Median flux (sort copy)
  const fluxSorted = flux.slice().sort((a, b) => a - b);
  const medianFlux = fluxSorted[Math.floor(fluxSorted.length / 2)];

  // ── Priority 1: Silence ──
  if (trailingSilence >= duration * 0.5) {
    const musicalEndOffset = trailingSilence;
    return {
      outroType: 'silence',
      outroConfidence: 0.95,
      musicalEndOffset,
      suggestedCrossfadeStart: computeSuggestedCrossfadeStart('silence', musicalEndOffset, duration, trailingSilence),
    };
  }
  // All windows below absolute threshold
  const allSilent = totalEnergy.every(e => e < 0.001);
  if (allSilent) {
    return {
      outroType: 'silence',
      outroConfidence: 0.9,
      musicalEndOffset: analysisDuration,
      suggestedCrossfadeStart: computeSuggestedCrossfadeStart('silence', analysisDuration, duration, trailingSilence),
    };
  }

  // ── Priority 2: Noise ending (applause/crowd) ──
  // Last 20% of windows: high flux (> median) while total energy is low (< 30% avg),
  // and high band is disproportionally strong
  const tailStart = Math.floor(windowCount * 0.8);
  let noiseCount = 0;
  let highDominantCount = 0;
  for (let i = tailStart; i < windowCount; i++) {
    if (flux[i] > medianFlux && totalEnergy[i] < avgEnergy * 0.3) {
      noiseCount++;
    }
    if (bands.high[i] > bands.mid[i] * 0.8 && bands.high[i] > bands.low[i]) {
      highDominantCount++;
    }
  }
  const tailLen = windowCount - tailStart;
  if (noiseCount > tailLen * 0.5 && highDominantCount > tailLen * 0.4) {
    const musicalEndOffset = computeMusicalEndOffset('noiseEnd', bands, flux, duration, trailingSilence, analysisDuration, windowCount);
    return {
      outroType: 'noiseEnd',
      outroConfidence: Math.min(0.9, noiseCount / tailLen),
      musicalEndOffset,
      suggestedCrossfadeStart: computeSuggestedCrossfadeStart('noiseEnd', musicalEndOffset, duration, trailingSilence),
    };
  }

  // ── Priority 3: Loop fade (repeating pattern with energy decrease) ──
  // Autocorrelation of totalEnergy in last 30s (120 windows at 250ms)
  const loopScanWindows = Math.min(120, windowCount);
  const loopScanStart = windowCount - loopScanWindows;
  const { period: loopPeriodIdx, confidence: loopConf } = detectRepetitionPeriod(
    totalEnergy, loopScanStart, windowCount
  );
  if (loopConf > 0.65 && loopPeriodIdx >= 2 && loopPeriodIdx <= 32) {
    // Check near-monotonic energy decrease across the looped region
    let decreasingCount = 0;
    const checkWindows = Math.floor(loopScanWindows / loopPeriodIdx);
    for (let p = 1; p < checkWindows; p++) {
      const prevStart = loopScanStart + (p - 1) * loopPeriodIdx;
      const currStart = loopScanStart + p * loopPeriodIdx;
      let prevAvg = 0, currAvg = 0;
      for (let j = 0; j < loopPeriodIdx && prevStart + j < windowCount && currStart + j < windowCount; j++) {
        prevAvg += totalEnergy[prevStart + j];
        currAvg += totalEnergy[currStart + j];
      }
      if (currAvg <= prevAvg * 1.02) decreasingCount++;
    }
    if (checkWindows > 1 && decreasingCount / (checkWindows - 1) > 0.7) {
      // Spectral stability gate: reject if band ratios change > 2x between halves
      // (indicates a structural transition, not a genuine loop fade)
      const loopHalf = Math.floor(loopScanWindows / 2);
      const firstHalfRatio = computeLocalBandRatio(bands, loopScanStart, loopScanStart + loopHalf);
      const secondHalfRatio = computeLocalBandRatio(bands, loopScanStart + loopHalf, windowCount);
      let spectralStable = true;
      for (let b = 0; b < 3; b++) {
        if (firstHalfRatio[b] > 0.01 && secondHalfRatio[b] / firstHalfRatio[b] > 2) spectralStable = false;
        if (secondHalfRatio[b] > 0.01 && firstHalfRatio[b] / secondHalfRatio[b] > 2) spectralStable = false;
      }

      if (spectralStable) {
        const loopPeriodSec = loopPeriodIdx * windowDuration;
        const musicalEndOffset = computeMusicalEndOffset('loopFade', bands, flux, duration, trailingSilence, analysisDuration, windowCount);
        return {
          outroType: 'loopFade',
          outroConfidence: Math.min(0.9, loopConf),
          musicalEndOffset,
          suggestedCrossfadeStart: computeSuggestedCrossfadeStart('loopFade', musicalEndOffset, duration, trailingSilence),
          loopPeriod: loopPeriodSec,
        };
      }
    }
  }

  // ── Priority 4: Fade-out ──
  // All 3 bands decrease near-monotonically over >= 12 windows (3s),
  // spectral shape preserved, flux low
  const minFadeWindows = 12; // 3s
  let fadeDetected = false;
  let fadeStartIdx = -1;
  let fadeConfidence = 0;

  // Scan from end, find longest near-monotonic decrease
  for (let start = Math.max(0, windowCount - 60); start < windowCount - minFadeWindows; start++) {
    let decreasing = true;
    let increaseCount = 0;
    const maxIncreases = Math.floor((windowCount - start) * 0.15); // allow 15% non-monotonic

    for (let i = start + 1; i < windowCount; i++) {
      if (totalEnergy[i] > totalEnergy[i - 1] * 1.05) { // 5% tolerance
        increaseCount++;
        if (increaseCount > maxIncreases) {
          decreasing = false;
          break;
        }
      }
    }

    if (decreasing && (windowCount - start) >= minFadeWindows) {
      // Check spectral shape preservation: band ratios should stay within 2x
      const startLow = bands.low[start] || 0.0001;
      const startMid = bands.mid[start] || 0.0001;
      const midpoint = Math.floor((start + windowCount) / 2);
      const midLow = bands.low[midpoint] || 0.0001;
      const midMid = bands.mid[midpoint] || 0.0001;

      const ratioStart = startLow / startMid;
      const ratioMid = midLow / midMid;
      const ratioChange = ratioStart > 0 ? Math.abs(ratioMid / ratioStart - 1) : 1;

      // Check flux is low during fade
      let avgFlux = 0;
      for (let i = start; i < windowCount; i++) avgFlux += flux[i];
      avgFlux /= (windowCount - start);
      const fluxIsLow = avgFlux < medianFlux * 1.5;

      if (ratioChange < 1.0 && fluxIsLow) {
        fadeDetected = true;
        fadeStartIdx = start;
        const fadeLength = windowCount - start;
        fadeConfidence = Math.min(0.95, 0.5 + (fadeLength / 40) * 0.3 + (1 - ratioChange) * 0.2);
        break; // take earliest qualifying fade start
      }
    }
  }

  if (fadeDetected && fadeStartIdx >= 0) {
    const musicalEndOffset = computeMusicalEndOffset('fadeOut', bands, flux, duration, trailingSilence, analysisDuration, windowCount);
    return {
      outroType: 'fadeOut',
      outroConfidence: fadeConfidence,
      musicalEndOffset,
      suggestedCrossfadeStart: computeSuggestedCrossfadeStart('fadeOut', musicalEndOffset, duration, trailingSilence),
    };
  }

  // ── Priority 5: SlowDown (tempo deceleration) ──
  // Detect increasing inter-onset intervals in last 15s (60 windows)
  const slowDownWindows = Math.min(60, windowCount);
  const slowDownStart = windowCount - slowDownWindows;
  const slowDownResult = detectSlowDown(flux, slowDownStart, windowCount, medianFlux);
  if (slowDownResult.detected) {
    const musicalEndOffset = computeMusicalEndOffset('slowDown', bands, flux, duration, trailingSilence, analysisDuration, windowCount);
    const decelerationStartSec = duration - analysisDuration + slowDownStart * windowDuration;
    return {
      outroType: 'slowDown',
      outroConfidence: Math.min(0.85, 0.5 + slowDownResult.r2 * 0.35),
      musicalEndOffset,
      suggestedCrossfadeStart: computeSuggestedCrossfadeStart('slowDown', musicalEndOffset, duration, trailingSilence),
      decelerationStart: Math.max(0, decelerationStartSec),
    };
  }

  // ── Priority 6: Sustained (held note/chord) ──
  // Last 20s (80 windows): very low spectral flux + slow energy decay + stable band ratios
  const sustainWindows = Math.min(80, windowCount);
  const sustainStart = windowCount - sustainWindows;
  const sustainFluxThreshold = medianFlux * 0.3;
  let consecutiveLowFlux = 0;
  let maxConsecutiveLowFlux = 0;
  let sustainBlockStart = sustainStart;

  for (let i = sustainStart; i < windowCount; i++) {
    if (flux[i] < sustainFluxThreshold) {
      consecutiveLowFlux++;
      if (consecutiveLowFlux > maxConsecutiveLowFlux) {
        maxConsecutiveLowFlux = consecutiveLowFlux;
        sustainBlockStart = i - consecutiveLowFlux + 1;
      }
    } else {
      consecutiveLowFlux = 0;
    }
  }

  if (maxConsecutiveLowFlux >= 32) { // >= 8s of low flux
    // Check slow energy decay (slope between -0.05 and 0)
    const energySlope = linearDecayRate(totalEnergy, sustainBlockStart, windowCount);
    const lowDecay = linearDecayRate(bands.low, sustainBlockStart, windowCount);
    const midDecay = linearDecayRate(bands.mid, sustainBlockStart, windowCount);
    const highDecay = linearDecayRate(bands.high, sustainBlockStart, windowCount);

    // Must NOT have mid+high decaying faster than low (that would be reverbTail)
    const midHighAvg = (midDecay + highDecay) / 2;
    const isNotReverbLike = lowDecay === 0 || midHighAvg >= lowDecay * 1.5;

    // Band ratio stability: check ratio doesn't change more than 1.5x
    const ratioStart2 = computeLocalBandRatio(bands, sustainBlockStart, sustainBlockStart + 8);
    const ratioEnd2 = computeLocalBandRatio(bands, windowCount - 8, windowCount);
    let ratioStable = true;
    for (let b = 0; b < 3; b++) {
      if (ratioStart2[b] > 0.01 && ratioEnd2[b] / ratioStart2[b] > 1.5) ratioStable = false;
      if (ratioEnd2[b] > 0.01 && ratioStart2[b] / ratioEnd2[b] > 1.5) ratioStable = false;
    }

    if (energySlope > -0.05 && energySlope <= 0 && isNotReverbLike && ratioStable) {
      const musicalEndOffset = computeMusicalEndOffset('sustained', bands, flux, duration, trailingSilence, analysisDuration, windowCount);
      const sustainOnsetSec = duration - analysisDuration + sustainBlockStart * windowDuration;
      return {
        outroType: 'sustained',
        outroConfidence: Math.min(0.85, 0.5 + (maxConsecutiveLowFlux / 80) * 0.35),
        musicalEndOffset,
        suggestedCrossfadeStart: computeSuggestedCrossfadeStart('sustained', musicalEndOffset, duration, trailingSilence),
        sustainOnset: Math.max(0, sustainOnsetSec),
      };
    }
  }

  // ── Priority 7: Musical outro (distinct spectral change) ──
  // Compare spectral shape of last 15s vs middle 30s
  const outroWindows = Math.min(60, Math.floor(windowCount * 0.25)); // last 15s
  const outroRegionStart = windowCount - outroWindows;
  const midStart = Math.floor(windowCount * 0.25);
  const midEnd = Math.min(midStart + 120, outroRegionStart); // middle 30s

  if (midEnd > midStart + 8 && outroWindows >= 8) {
    const midRatio = computeLocalBandRatio(bands, midStart, midEnd);
    const outroRatio = computeLocalBandRatio(bands, outroRegionStart, windowCount);
    const similarity = cosineSimilarity3(midRatio, outroRatio);

    // Check energy is still significant (> 0.3 * avg)
    let outroAvgEnergy = 0;
    for (let i = outroRegionStart; i < windowCount; i++) outroAvgEnergy += totalEnergy[i];
    outroAvgEnergy /= outroWindows;
    const hasEnergy = outroAvgEnergy > avgEnergy * 0.3;

    if (similarity < 0.7 && hasEnergy) {
      const musicalEndOffset = computeMusicalEndOffset('musicalOutro', bands, flux, duration, trailingSilence, analysisDuration, windowCount);
      const outroSectionStartSec = duration - analysisDuration + outroRegionStart * windowDuration;
      return {
        outroType: 'musicalOutro',
        outroConfidence: Math.min(0.85, 0.5 + (0.7 - similarity) * 0.5),
        musicalEndOffset,
        suggestedCrossfadeStart: computeSuggestedCrossfadeStart('musicalOutro', musicalEndOffset, duration, trailingSilence),
        outroSectionStart: Math.max(0, outroSectionStartSec),
      };
    }
  }

  // ── Priority 8: Reverb tail ──
  const reverbMinWindows = 8;  // 2s
  const reverbMaxWindows = 32; // 8s

  // Scan last 32 windows for reverb pattern
  const reverbScanStart = Math.max(0, windowCount - reverbMaxWindows);
  let reverbDetected = false;

  // Find where decay starts (first consistent decline in mid+high)
  for (let start = reverbScanStart; start < windowCount - reverbMinWindows; start++) {
    const tailLen2 = windowCount - start;
    if (tailLen2 < reverbMinWindows || tailLen2 > reverbMaxWindows) continue;

    // Measure decay rates: linear regression slope
    const lowDecay = linearDecayRate(bands.low, start, windowCount);
    const midDecay = linearDecayRate(bands.mid, start, windowCount);
    const highDecay = linearDecayRate(bands.high, start, windowCount);

    // Mid+High should decay faster (more negative) than Low
    const midHighAvgDecay = (midDecay + highDecay) / 2;
    if (midHighAvgDecay < 0 && lowDecay < 0 && midHighAvgDecay < lowDecay * 2) {
      // Flux should be decreasing but non-zero
      let fluxDecreasing = true;
      let fluxNonZero = false;
      for (let i = start + 1; i < windowCount; i++) {
        if (flux[i] > 0.0001) fluxNonZero = true;
      }
      const fluxEndAvg = (flux[windowCount - 1] + flux[Math.max(0, windowCount - 2)]) / 2;
      const fluxStartAvg = (flux[start] + flux[Math.min(windowCount - 1, start + 1)]) / 2;
      if (fluxStartAvg > 0 && fluxEndAvg > fluxStartAvg) fluxDecreasing = false;

      if (fluxNonZero && fluxDecreasing) {
        reverbDetected = true;
        const musicalEndOffset = computeMusicalEndOffset('reverbTail', bands, flux, duration, trailingSilence, analysisDuration, windowCount);
        return {
          outroType: 'reverbTail',
          outroConfidence: Math.min(0.85, 0.5 + Math.abs(midHighAvgDecay / lowDecay - 2) * 0.15),
          musicalEndOffset,
          suggestedCrossfadeStart: computeSuggestedCrossfadeStart('reverbTail', musicalEndOffset, duration, trailingSilence),
        };
      }
    }
  }

  // ── Priority 9: Hard ending (default) ──
  // Energy drops > 80% within 4 windows (1s)
  let hardConfidence = 0.5; // default fallback confidence
  for (let i = Math.max(0, windowCount - 8); i < windowCount - 1; i++) {
    if (totalEnergy[i] > avgEnergy * 0.3 && totalEnergy[i + 1] < totalEnergy[i] * 0.2) {
      hardConfidence = 0.85;
      break;
    }
  }

  const musicalEndOffset = computeMusicalEndOffset('hard', bands, flux, duration, trailingSilence, analysisDuration, windowCount);
  return {
    outroType: 'hard',
    outroConfidence: hardConfidence,
    musicalEndOffset,
    suggestedCrossfadeStart: computeSuggestedCrossfadeStart('hard', musicalEndOffset, duration, trailingSilence),
  };
}

/**
 * Simple linear decay rate (slope) of a band's values over a window range.
 * Returns negative value for decaying signals.
 */
function linearDecayRate(arr: number[], start: number, end: number): number {
  const n = end - start;
  if (n < 2) return 0;
  let sumX = 0, sumY = 0, sumXY = 0, sumX2 = 0;
  for (let i = start; i < end; i++) {
    const x = i - start;
    const y = arr[i];
    sumX += x;
    sumY += y;
    sumXY += x * y;
    sumX2 += x * x;
  }
  const denom = n * sumX2 - sumX * sumX;
  return denom !== 0 ? (n * sumXY - sumX * sumY) / denom : 0;
}

/**
 * Compute where the musical content effectively ends (seconds from file end).
 */
function computeMusicalEndOffset(
  outroType: OutroType,
  bands: { low: number[]; mid: number[]; high: number[] },
  flux: number[],
  duration: number,
  trailingSilence: number,
  analysisDuration: number,
  windowCount: number
): number {
  const windowDuration = OUTRO_WINDOW_MS / 1000;

  switch (outroType) {
    case 'fadeOut': {
      // Where mid-band drops below 50% of pre-fade average
      const preFadeCount = Math.max(1, Math.floor(windowCount * 0.3));
      let preFadeAvg = 0;
      for (let i = 0; i < preFadeCount; i++) preFadeAvg += bands.mid[i];
      preFadeAvg /= preFadeCount;
      const threshold = preFadeAvg * 0.5;

      for (let i = preFadeCount; i < windowCount; i++) {
        if (bands.mid[i] < threshold) {
          const offsetFromAnalysisEnd = (windowCount - i) * windowDuration;
          return offsetFromAnalysisEnd + trailingSilence;
        }
      }
      return trailingSilence + 2; // fallback
    }

    case 'reverbTail': {
      // Last point where flux > 2× tail-region average
      const tailRegionStart = Math.floor(windowCount * 0.7);
      let tailAvg = 0;
      for (let i = tailRegionStart; i < windowCount; i++) tailAvg += flux[i];
      tailAvg /= (windowCount - tailRegionStart);

      for (let i = windowCount - 1; i >= tailRegionStart; i--) {
        if (flux[i] > tailAvg * 2) {
          const offsetFromAnalysisEnd = (windowCount - i) * windowDuration;
          return offsetFromAnalysisEnd + trailingSilence;
        }
      }
      return (windowCount - tailRegionStart) * windowDuration + trailingSilence;
    }

    case 'noiseEnd': {
      // Where flux pattern changes (music → noise): find max first-derivative of flux
      let maxDerivChange = 0;
      let changeIdx = Math.floor(windowCount * 0.5);

      for (let i = 2; i < windowCount - 1; i++) {
        const d1 = flux[i] - flux[i - 1];
        const d2 = flux[i + 1] - flux[i];
        const derivChange = Math.abs(d2 - d1);
        if (derivChange > maxDerivChange) {
          maxDerivChange = derivChange;
          changeIdx = i;
        }
      }
      const offsetFromAnalysisEnd = (windowCount - changeIdx) * windowDuration;
      return offsetFromAnalysisEnd + trailingSilence;
    }

    case 'hard': {
      // Where energy last exceeds 10% of average
      let avgE = 0;
      for (let i = 0; i < windowCount; i++) {
        avgE += bands.low[i] + bands.mid[i] + bands.high[i];
      }
      avgE /= windowCount;
      const threshold = avgE * 0.1;

      for (let i = windowCount - 1; i >= 0; i--) {
        const e = bands.low[i] + bands.mid[i] + bands.high[i];
        if (e > threshold) {
          const offsetFromAnalysisEnd = (windowCount - i - 1) * windowDuration;
          return offsetFromAnalysisEnd + trailingSilence;
        }
      }
      return trailingSilence;
    }

    case 'slowDown': {
      // Last point where flux has significant onsets
      for (let i = windowCount - 1; i >= Math.floor(windowCount * 0.5); i--) {
        const e = bands.low[i] + bands.mid[i] + bands.high[i];
        if (e > 0.05) {
          const offsetFromAnalysisEnd = (windowCount - i - 1) * windowDuration;
          return offsetFromAnalysisEnd + trailingSilence;
        }
      }
      return trailingSilence + 2;
    }

    case 'sustained': {
      // Where flux drops to sustained-level low values
      const tailRegionStart3 = Math.floor(windowCount * 0.6);
      let tailAvg3 = 0;
      for (let i = tailRegionStart3; i < windowCount; i++) tailAvg3 += flux[i];
      tailAvg3 /= (windowCount - tailRegionStart3);

      for (let i = windowCount - 1; i >= tailRegionStart3; i--) {
        if (flux[i] > tailAvg3 * 3) {
          const offsetFromAnalysisEnd = (windowCount - i) * windowDuration;
          return offsetFromAnalysisEnd + trailingSilence;
        }
      }
      return (windowCount - tailRegionStart3) * windowDuration + trailingSilence;
    }

    case 'musicalOutro': {
      // Where spectral shape diverges from the main body
      // Use energy threshold: last point where energy exceeds 20% of average
      let avgE2 = 0;
      for (let i = 0; i < windowCount; i++) {
        avgE2 += bands.low[i] + bands.mid[i] + bands.high[i];
      }
      avgE2 /= windowCount;

      for (let i = windowCount - 1; i >= 0; i--) {
        const e = bands.low[i] + bands.mid[i] + bands.high[i];
        if (e > avgE2 * 0.2) {
          const offsetFromAnalysisEnd = (windowCount - i - 1) * windowDuration;
          return offsetFromAnalysisEnd + trailingSilence;
        }
      }
      return trailingSilence;
    }

    case 'loopFade': {
      // Where energy drops below 30% of the looped region average
      const loopRegionStart = Math.floor(windowCount * 0.5);
      let loopAvg = 0;
      for (let i = loopRegionStart; i < windowCount; i++) {
        loopAvg += bands.low[i] + bands.mid[i] + bands.high[i];
      }
      loopAvg /= (windowCount - loopRegionStart);

      for (let i = windowCount - 1; i >= loopRegionStart; i--) {
        const e = bands.low[i] + bands.mid[i] + bands.high[i];
        if (e > loopAvg * 0.3) {
          const offsetFromAnalysisEnd = (windowCount - i - 1) * windowDuration;
          return offsetFromAnalysisEnd + trailingSilence;
        }
      }
      return trailingSilence + 2;
    }

    case 'silence':
      return trailingSilence;

    default:
      return trailingSilence;
  }
}

/**
 * Compute suggested crossfade start (seconds from track start).
 */
function computeSuggestedCrossfadeStart(
  outroType: OutroType,
  musicalEndOffset: number,
  duration: number,
  trailingSilence: number
): number {
  let start: number;

  switch (outroType) {
    case 'fadeOut':
      // Start when mid-band is at ~70% — before fade gets too deep
      // musicalEndOffset is where mid drops below 50%, so start earlier
      start = duration - musicalEndOffset - 4;
      break;

    case 'reverbTail':
      // Start 1s before tail begins (pre-buffer eliminates load delay)
      start = duration - musicalEndOffset - 1;
      break;

    case 'noiseEnd':
      // At musicalEndOffset (before applause/noise)
      start = duration - musicalEndOffset;
      break;

    case 'hard':
      // Buffer before cliff (tighter with pre-buffering)
      start = duration - musicalEndOffset - 3;
      break;

    case 'slowDown':
      // Start at ~70% of the remaining deceleration region (tighter with pre-buffering)
      start = duration - musicalEndOffset - 3;
      break;

    case 'sustained':
      // Start at musicalEndOffset + 1.5s buffer (tighter with pre-buffering)
      start = duration - musicalEndOffset - 1.5;
      break;

    case 'musicalOutro':
      // Start at ~60% into the outro section
      start = duration - musicalEndOffset * 0.6;
      break;

    case 'loopFade':
      // Start early, like fadeOut — tighter with pre-buffering
      start = duration - musicalEndOffset - 3;
      break;

    case 'silence':
      // Before silence starts
      start = duration - trailingSilence - 8;
      break;

    default:
      start = duration - 12;
  }

  // Clamp: never start before 30s into the track or after duration - 3s
  return Math.max(30, Math.min(start, duration - 3));
}

// ─── Intro Analysis ─────────────────────────────────────────────────

interface IntroAnalysis {
  /** Seconds from start until energy consistently exceeds 50% of track average */
  quietIntroDuration: number;
  /** Seconds from start until energy consistently exceeds 80% of track average */
  energyBuildDuration: number;
  /** Average energy of first 20s relative to track average (0-1+) */
  introEnergyRatio: number;
  /** Multiband energy at 250ms windows for first 20s (same format as OutroAnalysis) */
  multibandEnergy: { low: number[]; mid: number[]; high: number[] } | null;
}

/** Maximum intro scan window in seconds */
const INTRO_SCAN_SECONDS = 20;

/** Maximum intro multiband analysis duration in seconds */
const INTRO_ANALYSIS_SECONDS = 20;

/**
 * Multiband energy analysis of the intro region (first 20s).
 * Same IIR filter infrastructure as the outro analysis — produces
 * low/mid/high RMS at 250ms windows for spectral similarity matching.
 */
function analyzeIntroMultiband(
  data: Float32Array,
  sampleRate: number,
  duration: number
): { low: number[]; mid: number[]; high: number[] } | null {
  if (duration < 5) return null;

  const analysisDuration = Math.min(INTRO_ANALYSIS_SECONDS, duration);
  const windowSamples = Math.floor(sampleRate * OUTRO_WINDOW_MS / 1000);
  const totalWindows = Math.floor(analysisDuration * 1000 / OUTRO_WINDOW_MS);

  if (totalWindows < 4 || windowSamples < 1) return null;

  // Design same filters as outro analysis
  const lowState = createIIRState(designBiquadBandpass(20, 300, sampleRate));
  const midState = createIIRState(designBiquadBandpass(300, 4000, sampleRate));
  const highState = createIIRState(designBiquadBandpass(4000, Math.min(16000, sampleRate / 2 - 100), sampleRate));

  const low: number[] = new Array(totalWindows);
  const mid: number[] = new Array(totalWindows);
  const high: number[] = new Array(totalWindows);

  let accumLow = 0, accumMid = 0, accumHigh = 0;
  let sampleInWindow = 0;
  let windowIdx = 0;

  const endSample = Math.min(totalWindows * windowSamples, data.length);

  for (let i = 0; i < endSample && windowIdx < totalWindows; i++) {
    const s = data[i];
    const fLow = iirProcessSample(lowState, s);
    const fMid = iirProcessSample(midState, s);
    const fHigh = iirProcessSample(highState, s);

    accumLow += fLow * fLow;
    accumMid += fMid * fMid;
    accumHigh += fHigh * fHigh;
    sampleInWindow++;

    if (sampleInWindow >= windowSamples) {
      low[windowIdx] = Math.sqrt(accumLow / sampleInWindow);
      mid[windowIdx] = Math.sqrt(accumMid / sampleInWindow);
      high[windowIdx] = Math.sqrt(accumHigh / sampleInWindow);

      accumLow = 0; accumMid = 0; accumHigh = 0;
      sampleInWindow = 0;
      windowIdx++;
    }
  }

  if (windowIdx < 4) return null;
  return {
    low: low.slice(0, windowIdx),
    mid: mid.slice(0, windowIdx),
    high: high.slice(0, windowIdx),
  };
}

/**
 * Analyze the intro region of a track using per-second energy data.
 * Detects how long the intro stays quiet and when energy fully builds up.
 * Used by AutoMixEngine to adjust crossfade duration/curve for the incoming track.
 */
function analyzeIntro(
  energyPerSecond: number[],
  averageEnergy: number,
  data: Float32Array,
  sampleRate: number,
  duration: number
): IntroAnalysis | null {
  const scanLen = Math.min(INTRO_SCAN_SECONDS, energyPerSecond.length);
  if (scanLen < 4) return null;

  // Quiet intro: first second where energy consistently exceeds 50% of average
  // "Consistently" = 2 consecutive seconds above threshold (avoids transient false triggers)
  const quietThreshold = averageEnergy * 0.5;
  let quietIntroDuration = scanLen;

  for (let i = 0; i < scanLen - 1; i++) {
    if (energyPerSecond[i] >= quietThreshold && energyPerSecond[i + 1] >= quietThreshold) {
      quietIntroDuration = i;
      break;
    }
  }

  // Energy build: first second where energy consistently exceeds 80% of average
  // This marks where the track's main content is fully active
  const buildThreshold = averageEnergy * 0.8;
  let energyBuildDuration = scanLen;

  for (let i = 0; i < scanLen - 1; i++) {
    if (energyPerSecond[i] >= buildThreshold && energyPerSecond[i + 1] >= buildThreshold) {
      energyBuildDuration = i;
      break;
    }
  }

  // Average energy of first 20s relative to track average
  let sum = 0;
  for (let i = 0; i < scanLen; i++) sum += energyPerSecond[i];
  const introEnergyRatio = averageEnergy > 0.001 ? (sum / scanLen) / averageEnergy : 1;

  const multibandEnergy = analyzeIntroMultiband(data, sampleRate, duration);

  return {
    quietIntroDuration,
    energyBuildDuration,
    introEnergyRatio,
    multibandEnergy,
  };
}

// ─── Message Handler ───────────────────────────────────────────────

self.onmessage = (e: MessageEvent<AnalysisRequest>) => {
  const { type, id, monoData, sampleRate, duration, analyzeBPM } = e.data;

  if (type !== 'analyze') return;

  try {
    const volume = analyzeVolume(monoData);
    const energy = analyzeEnergy(monoData, sampleRate, duration);
    const bpm = analyzeBPM ? runBPMDetection(monoData, sampleRate, duration) : null;
    const fingerprint = computeFingerprint(monoData, sampleRate);
    const outro = analyzeOutroMultiband(monoData, sampleRate, duration, energy.trailingSilence);
    const intro = analyzeIntro(energy.energyPerSecond, energy.averageEnergy, monoData, sampleRate, duration);

    const response: AnalysisResponse = {
      type: 'result',
      id,
      volume,
      energy,
      bpm,
      fingerprint,
      outro,
      intro,
      duration,
    };

    (self as unknown as Worker).postMessage(response);
  } catch (err) {
    const response: ErrorResponse = {
      type: 'error',
      id,
      error: err instanceof Error ? err.message : String(err),
    };
    (self as unknown as Worker).postMessage(response);
  }
};
