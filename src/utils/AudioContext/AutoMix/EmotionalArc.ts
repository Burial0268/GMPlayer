/**
 * EmotionalArc — hand over after the outgoing song's last chorus, and shape
 * the blend by the energy of the two passages that overlap.
 *
 * Mirrors `src-tauri/crates/audio-backend/src/automix/emotion.rs`, which plans
 * the native transitions; keep the thresholds and profiles in step.
 */

import type { CrossfadeCurve } from "./types";
import type { SectionAnalysis, SongSection, SongSectionKind, TrackAnalysis } from "./TrackAnalyzer";

/**
 * Furthest ahead of the plan it replaces that a section cue may start the
 * fade. Beyond this it would drop too much of a long outro.
 */
const CUE_LOOKBACK_SECONDS = 20;
const MIN_ANALYSIS_CONFIDENCE = 0.35;
const MIN_CUE_CONFIDENCE = 0.4;
/**
 * Section energy is relative to each song's own loud passages (p95 = 1), so
 * these describe where a passage sits in its own song's arc.
 */
const HIGH_ENERGY = 0.62;
const LOW_ENERGY = 0.42;
/** Two sung passages on top of each other is the classic train wreck. */
const VOCAL_CLASH_RISK = 0.6;
const VOCAL_CLASH_DURATION_SCALE = 0.8;

/**
 * - afterglow: loud into soft — let the peak recede into the new song.
 * - lift: soft into loud — hold the new song back, then let it land.
 * - sustain: loud into loud — swap quickly so the overlap does not turn to mud.
 * - drift: soft into soft — a long, wide blend.
 */
export type EmotionalArc = "afterglow" | "lift" | "sustain" | "drift" | "neutral";

/** Adjustments an arc makes to a crossfade. Deliberately small: every other
 *  AutoMix heuristic compounds with these. */
export interface ArcProfile {
  /** Applied to the planned duration, which stays within the user's setting. */
  durationScale: number;
  curve: CrossfadeCurve | null;
  /** Exponent on the incoming curve: above 1 holds the new song back. */
  inShape: number;
  /** Exponent on the outgoing curve: above 1 clears the old song sooner. */
  outShape: number;
  /** Only ever deepens the headroom already planned. */
  overlapHeadroomDb: number | null;
  /** Web-only: calm arcs drop the rhythmic DJ effects (riser, gate, echo, swell). */
  calm: boolean;
}

/**
 * Where the outgoing song should start handing over: the end of its last
 * chorus, or failing that the start of its outro.
 */
export function sectionMixOutCue(
  sections: SectionAnalysis | null | undefined,
  baselineStart: number,
  effectiveEnd: number,
  minDuration: number,
): number | null {
  if (!sections || sections.confidence < MIN_ANALYSIS_CONFIDENCE) return null;
  const lastConfident = (kind: SongSectionKind): SongSection | undefined => {
    for (let i = sections.sections.length - 1; i >= 0; i--) {
      const section = sections.sections[i];
      if (section.sectionType === kind && section.confidence >= MIN_CUE_CONFIDENCE) {
        return section;
      }
    }
    return undefined;
  };
  const cue = lastConfident("chorus")?.end ?? lastConfident("outro")?.start;
  if (cue === undefined) return null;
  return cue >= baselineStart - CUE_LOOKBACK_SECONDS && cue <= effectiveEnd - minDuration
    ? cue
    : null;
}

/**
 * The arc of a fade of `duration` seconds starting at `start` in the outgoing
 * song, and the profile that shapes it.
 */
export function planEmotionalArc(
  outgoing: TrackAnalysis,
  incoming: TrackAnalysis,
  start: number,
  duration: number,
): { arc: EmotionalArc; profile: ArcProfile } | null {
  const end = start + duration;
  const outEnergy = passageEnergy(outgoing, start, end);
  const inEnergy = passageEnergy(incoming, 0, duration);
  if (outEnergy === null || inEnergy === null) return null;

  const arc = classifyArc(outEnergy, inEnergy);
  const profile = arcProfile(arc);
  const outVocal = sectionsAverage(outgoing, start, end, (s) => s.vocalRisk);
  const inVocal = sectionsAverage(incoming, 0, duration, (s) => s.vocalRisk);
  if (
    outVocal !== null &&
    inVocal !== null &&
    outVocal >= VOCAL_CLASH_RISK &&
    inVocal >= VOCAL_CLASH_RISK
  ) {
    profile.durationScale = Math.min(profile.durationScale, VOCAL_CLASH_DURATION_SCALE);
  }
  return { arc, profile };
}

function classifyArc(outgoing: number, incoming: number): EmotionalArc {
  const high = (energy: number) => energy >= HIGH_ENERGY;
  const low = (energy: number) => energy <= LOW_ENERGY;
  if (high(outgoing) && low(incoming)) return "afterglow";
  if (low(outgoing) && high(incoming)) return "lift";
  if (high(outgoing) && high(incoming)) return "sustain";
  if (low(outgoing) && low(incoming)) return "drift";
  return "neutral";
}

function arcProfile(arc: EmotionalArc): ArcProfile {
  switch (arc) {
    case "afterglow":
      return {
        durationScale: 1.25,
        curve: "sCurve",
        inShape: 1,
        // Unmask the quiet intro instead of burying it under the old peak.
        outShape: 1.1,
        overlapHeadroomDb: -1.2,
        calm: true,
      };
    case "lift":
      return {
        durationScale: 0.8,
        curve: "equalPower",
        inShape: 1.2,
        outShape: 1,
        overlapHeadroomDb: -1.6,
        calm: false,
      };
    case "sustain":
      return {
        durationScale: 0.8,
        curve: "equalPower",
        inShape: 1.15,
        outShape: 1.15,
        overlapHeadroomDb: -2,
        calm: false,
      };
    case "drift":
      return {
        durationScale: 1.2,
        curve: "sCurve",
        inShape: 0.9,
        outShape: 0.9,
        overlapHeadroomDb: null,
        calm: true,
      };
    default:
      return {
        durationScale: 1,
        curve: null,
        inShape: 1,
        outShape: 1,
        overlapHeadroomDb: null,
        calm: false,
      };
  }
}

/**
 * Energy of a passage, from the sections it overlaps when they are known — a
 * section's level is steadier than a few seconds of raw RMS — and from the
 * per-second energy otherwise.
 */
function passageEnergy(analysis: TrackAnalysis, start: number, end: number): number | null {
  if (end <= start) return null;
  const fromSections = sectionsAverage(analysis, start, end, (s) => s.energy);
  if (fromSections !== null) return fromSections;

  const values = analysis.energy.energyPerSecond;
  const from = Math.min(Math.floor(Math.max(0, start)), values.length);
  const to = Math.max(from, Math.min(Math.ceil(end), values.length));
  if (to <= from) return null;
  let sum = 0;
  for (let i = from; i < to; i++) sum += values[i];
  return sum / (to - from);
}

function sectionsAverage(
  analysis: TrackAnalysis,
  start: number,
  end: number,
  value: (section: SongSection) => number,
): number | null {
  const sections = analysis.sections;
  if (!sections || sections.confidence < MIN_ANALYSIS_CONFIDENCE || end <= start) return null;
  let sum = 0;
  let weight = 0;
  for (const section of sections.sections) {
    if (section.sectionType === "silence") continue;
    const overlap = Math.min(end, section.end) - Math.max(start, section.start);
    if (overlap > 0) {
      sum += value(section) * overlap;
      weight += overlap;
    }
  }
  return weight >= 0.5 * (end - start) ? sum / weight : null;
}
