//! Emotional transitions: hand over after the outgoing song's last chorus, and
//! shape the blend by the energy of the two passages that overlap.
//!
//! Mirrored for the Web path by `src/utils/AudioContext/AutoMix/EmotionalArc.ts`;
//! keep the thresholds and profiles in step.

use super::{SectionAnalysis, SongSection, SongSectionKind, TrackAnalysis};
use crate::types::CrossfadeCurve;

/// Furthest ahead of the plan it replaces that a section cue may start the
/// fade. Beyond this it would drop too much of a long outro.
const CUE_LOOKBACK_SECONDS: f32 = 20.0;
const MIN_ANALYSIS_CONFIDENCE: f32 = 0.35;
const MIN_CUE_CONFIDENCE: f32 = 0.4;
/// Section energy is relative to each song's own loud passages (p95 = 1),
/// so these describe where a passage sits in its own song's arc.
const HIGH_ENERGY: f32 = 0.62;
const LOW_ENERGY: f32 = 0.42;
/// Two sung passages on top of each other is the classic train wreck.
const VOCAL_CLASH_RISK: f32 = 0.6;
const VOCAL_CLASH_DURATION_SCALE: f32 = 0.8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmotionalArc {
    /// Loud into soft: let the peak recede into the new song.
    Afterglow,
    /// Soft into loud: hold the new song back, then let it land.
    Lift,
    /// Loud into loud: swap quickly so the overlap does not turn to mud.
    Sustain,
    /// Soft into soft: a long, wide blend.
    Drift,
    Neutral,
}

/// Adjustments an arc makes to a crossfade plan. Deliberately small: every
/// other AutoMix heuristic compounds with these.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ArcProfile {
    /// Applied to the planned duration, which stays within the user's setting.
    pub duration_scale: f32,
    pub curve: Option<CrossfadeCurve>,
    /// Exponent on the incoming curve: above 1 holds the new song back.
    pub in_shape: f32,
    /// Exponent on the outgoing curve: above 1 clears the old song sooner.
    pub out_shape: f32,
    /// Only ever deepens the headroom already planned.
    pub overlap_headroom_db: Option<f32>,
}

/// Where the outgoing song should start handing over: the end of its last
/// chorus, or failing that the start of its outro.
pub(crate) fn section_mix_out_cue(
    sections: &SectionAnalysis,
    baseline_start: f32,
    effective_end: f32,
    min_duration: f32,
) -> Option<f32> {
    if sections.confidence < MIN_ANALYSIS_CONFIDENCE {
        return None;
    }
    let last_confident = |kind: SongSectionKind| {
        sections
            .sections
            .iter()
            .rev()
            .find(|s| s.section_type == kind && s.confidence >= MIN_CUE_CONFIDENCE)
    };
    let cue = last_confident(SongSectionKind::Chorus)
        .map(|s| s.end)
        .or_else(|| last_confident(SongSectionKind::Outro).map(|s| s.start))?;
    (cue >= baseline_start - CUE_LOOKBACK_SECONDS && cue <= effective_end - min_duration)
        .then_some(cue)
}

/// The arc of a fade of `duration` seconds starting at `start` in the outgoing
/// song, and the profile that shapes it.
pub(crate) fn plan_arc(
    outgoing: &TrackAnalysis,
    incoming: &TrackAnalysis,
    start: f32,
    duration: f32,
) -> Option<(EmotionalArc, ArcProfile)> {
    let end = start + duration;
    let arc = classify_arc(
        passage_energy(outgoing, start, end)?,
        passage_energy(incoming, 0.0, duration)?,
    );
    let mut profile = arc_profile(arc);
    let clash = matches!(
        (
            passage_vocal_risk(outgoing, start, end),
            passage_vocal_risk(incoming, 0.0, duration),
        ),
        (Some(out), Some(inc)) if out >= VOCAL_CLASH_RISK && inc >= VOCAL_CLASH_RISK
    );
    if clash {
        profile.duration_scale = profile.duration_scale.min(VOCAL_CLASH_DURATION_SCALE);
    }
    Some((arc, profile))
}

fn classify_arc(outgoing: f32, incoming: f32) -> EmotionalArc {
    let high = |energy: f32| energy >= HIGH_ENERGY;
    let low = |energy: f32| energy <= LOW_ENERGY;
    if high(outgoing) && low(incoming) {
        EmotionalArc::Afterglow
    } else if low(outgoing) && high(incoming) {
        EmotionalArc::Lift
    } else if high(outgoing) && high(incoming) {
        EmotionalArc::Sustain
    } else if low(outgoing) && low(incoming) {
        EmotionalArc::Drift
    } else {
        EmotionalArc::Neutral
    }
}

fn arc_profile(arc: EmotionalArc) -> ArcProfile {
    match arc {
        EmotionalArc::Afterglow => ArcProfile {
            duration_scale: 1.25,
            curve: Some(CrossfadeCurve::SCurve),
            in_shape: 1.0,
            // Unmask the quiet intro instead of burying it under the old peak.
            out_shape: 1.1,
            overlap_headroom_db: Some(-1.2),
        },
        EmotionalArc::Lift => ArcProfile {
            duration_scale: 0.8,
            curve: Some(CrossfadeCurve::EqualPower),
            in_shape: 1.2,
            out_shape: 1.0,
            overlap_headroom_db: Some(-1.6),
        },
        EmotionalArc::Sustain => ArcProfile {
            duration_scale: 0.8,
            curve: Some(CrossfadeCurve::EqualPower),
            in_shape: 1.15,
            out_shape: 1.15,
            overlap_headroom_db: Some(-2.0),
        },
        EmotionalArc::Drift => ArcProfile {
            duration_scale: 1.2,
            curve: Some(CrossfadeCurve::SCurve),
            in_shape: 0.9,
            out_shape: 0.9,
            overlap_headroom_db: None,
        },
        EmotionalArc::Neutral => ArcProfile {
            duration_scale: 1.0,
            curve: None,
            in_shape: 1.0,
            out_shape: 1.0,
            overlap_headroom_db: None,
        },
    }
}

/// Energy of a passage, from the sections it overlaps when they are known —
/// a section's level is steadier than a few seconds of raw RMS — and from
/// the per-second energy otherwise.
fn passage_energy(analysis: &TrackAnalysis, start: f32, end: f32) -> Option<f32> {
    if end <= start {
        return None;
    }
    sections_average(analysis, start, end, |s| s.energy).or_else(|| {
        let values = &analysis.energy.energy_per_second;
        let from = (start.max(0.0).floor() as usize).min(values.len());
        let to = (end.ceil() as usize).clamp(from, values.len());
        (to > from).then(|| values[from..to].iter().sum::<f32>() / (to - from) as f32)
    })
}

fn passage_vocal_risk(analysis: &TrackAnalysis, start: f32, end: f32) -> Option<f32> {
    if end <= start {
        return None;
    }
    sections_average(analysis, start, end, |s| s.vocal_risk)
}

fn sections_average(
    analysis: &TrackAnalysis,
    start: f32,
    end: f32,
    value: impl Fn(&SongSection) -> f32,
) -> Option<f32> {
    let sections = analysis
        .sections
        .as_ref()
        .filter(|s| s.confidence >= MIN_ANALYSIS_CONFIDENCE)?;
    let (sum, weight) = sections
        .sections
        .iter()
        .filter(|s| s.section_type != SongSectionKind::Silence)
        .fold((0.0f32, 0.0f32), |(sum, weight), section| {
            let overlap = end.min(section.end) - start.max(section.start);
            if overlap > 0.0 {
                (sum + value(section) * overlap, weight + overlap)
            } else {
                (sum, weight)
            }
        });
    (weight >= 0.5 * (end - start)).then(|| sum / weight)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automix::{EnergyAnalysis, SpectralFingerprint, VolumeAnalysis};

    fn section(kind: SongSectionKind, start: f32, end: f32, energy: f32) -> SongSection {
        SongSection {
            section_type: kind,
            start,
            end,
            index: 0,
            confidence: 0.7,
            energy,
            vocal_risk: 0.3,
            mix_suitability: 0.5,
        }
    }

    fn sections(list: Vec<SongSection>) -> SectionAnalysis {
        SectionAnalysis {
            sections: list,
            confidence: 0.7,
            method: "noveltyRepetition".into(),
        }
    }

    fn track(sections: Option<SectionAnalysis>, energy_per_second: Vec<f32>) -> TrackAnalysis {
        let duration = energy_per_second.len() as f32;
        TrackAnalysis {
            volume: VolumeAnalysis {
                peak: 1.0,
                rms: 0.2,
                estimated_lufs: -14.0,
                gain_adjustment: 1.0,
            },
            energy: EnergyAnalysis {
                energy_per_second,
                outro_start_offset: 8.0,
                intro_end_offset: 2.0,
                average_energy: 0.5,
                trailing_silence: 0.0,
                is_fade_out: false,
            },
            bpm: None,
            fingerprint: SpectralFingerprint { bands: Vec::new() },
            outro: None,
            intro: None,
            phrases: None,
            sections,
            vocal_activity: None,
            mix_candidates: None,
            duration,
        }
    }

    fn song() -> SectionAnalysis {
        sections(vec![
            section(SongSectionKind::Intro, 0.0, 10.0, 0.3),
            section(SongSectionKind::Verse, 10.0, 60.0, 0.55),
            section(SongSectionKind::Chorus, 60.0, 90.0, 0.9),
            section(SongSectionKind::Outro, 90.0, 110.0, 0.35),
        ])
    }

    #[test]
    fn cue_is_the_end_of_the_last_chorus() {
        assert_eq!(section_mix_out_cue(&song(), 100.0, 110.0, 2.0), Some(90.0));
    }

    #[test]
    fn cue_does_not_drop_more_than_the_lookback() {
        assert_eq!(section_mix_out_cue(&song(), 111.0, 130.0, 2.0), None);
    }

    #[test]
    fn cue_needs_room_to_fade_after_it() {
        assert_eq!(section_mix_out_cue(&song(), 85.0, 91.0, 2.0), None);
    }

    #[test]
    fn cue_falls_back_to_the_outro_without_a_chorus() {
        let mut analysis = song();
        analysis.sections[2].section_type = SongSectionKind::Verse;
        assert_eq!(
            section_mix_out_cue(&analysis, 100.0, 110.0, 2.0),
            Some(90.0)
        );
    }

    #[test]
    fn cue_ignores_unsure_sections() {
        let mut analysis = song();
        analysis.confidence = 0.2;
        assert_eq!(section_mix_out_cue(&analysis, 100.0, 110.0, 2.0), None);
    }

    #[test]
    fn arcs_follow_the_energy_of_both_passages() {
        assert_eq!(classify_arc(0.9, 0.3), EmotionalArc::Afterglow);
        assert_eq!(classify_arc(0.3, 0.9), EmotionalArc::Lift);
        assert_eq!(classify_arc(0.9, 0.8), EmotionalArc::Sustain);
        assert_eq!(classify_arc(0.3, 0.2), EmotionalArc::Drift);
        assert_eq!(classify_arc(0.5, 0.9), EmotionalArc::Neutral);
    }

    #[test]
    fn a_fade_out_of_the_chorus_into_a_quiet_intro_is_an_afterglow() {
        let outgoing = track(Some(song()), vec![0.5; 110]);
        let incoming = track(Some(song()), vec![0.5; 110]);
        let (arc, profile) = plan_arc(&outgoing, &incoming, 82.0, 8.0).expect("arc");
        assert_eq!(arc, EmotionalArc::Afterglow);
        assert_eq!(profile.curve, Some(CrossfadeCurve::SCurve));
    }

    #[test]
    fn per_second_energy_stands_in_for_missing_sections() {
        let outgoing = track(None, vec![0.3; 110]);
        let incoming = track(None, vec![0.9; 110]);
        let (arc, _) = plan_arc(&outgoing, &incoming, 100.0, 8.0).expect("arc");
        assert_eq!(arc, EmotionalArc::Lift);
    }

    #[test]
    fn overlapping_vocals_shorten_the_blend() {
        let mut sung = song();
        sung.sections.iter_mut().for_each(|s| s.vocal_risk = 0.8);
        let outgoing = track(Some(sung.clone()), vec![0.5; 110]);
        let incoming = track(Some(sung), vec![0.5; 110]);
        let (arc, profile) = plan_arc(&outgoing, &incoming, 82.0, 8.0).expect("arc");
        assert_eq!(arc, EmotionalArc::Afterglow);
        assert_eq!(profile.duration_scale, VOCAL_CLASH_DURATION_SCALE);
    }
}
