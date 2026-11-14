//! Pure transition planning from compact track analysis.

use super::beat_plan;
use crate::automix as automix_analysis;
use crate::automix::emotion::{self, EmotionalArc};
use crate::types::{AutoMixConfig, CrossfadeCurve};

const MIN_CROSSFADE_DURATION: f64 = 2.0;
const DEFAULT_OVERLAP_HEADROOM_DB: f64 = -1.2;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CrossfadePlan {
    pub start_time: f64,
    pub duration: f64,
    pub incoming_gain_adjustment: f64,
    pub overlap_headroom_db: f64,
    pub curve: CrossfadeCurve,
    pub in_shape: f64,
    pub out_shape: f64,
    pub arc: EmotionalArc,
    /// Section boundary the fade was moved to, when emotional transitions chose one.
    pub section_cue: Option<f64>,
    pub outgoing_rate: f64,
    pub beat_synced: bool,
}

pub(super) fn build_time_based_plan(
    config: &AutoMixConfig,
    current_duration: f64,
) -> CrossfadePlan {
    let max_duration = (current_duration / 4.0).max(MIN_CROSSFADE_DURATION);
    let duration = config
        .crossfade_duration
        .clamp(MIN_CROSSFADE_DURATION, max_duration);
    let start_time = (current_duration - duration).max(0.0);

    CrossfadePlan {
        start_time,
        duration,
        incoming_gain_adjustment: 1.0,
        overlap_headroom_db: DEFAULT_OVERLAP_HEADROOM_DB,
        curve: config.transition_style,
        in_shape: 1.0,
        out_shape: 1.0,
        arc: EmotionalArc::Neutral,
        section_cue: None,
        outgoing_rate: 1.0,
        beat_synced: false,
    }
}

pub(super) fn build_analysis_backed_plan(
    config: &AutoMixConfig,
    current_duration: f64,
    current_analysis: Option<&automix_analysis::TrackAnalysis>,
    next_analysis: Option<&automix_analysis::TrackAnalysis>,
) -> CrossfadePlan {
    let mut plan = build_time_based_plan(config, current_duration);

    if config.volume_norm {
        if let Some(next) = next_analysis {
            plan.incoming_gain_adjustment = next.volume.gain_adjustment as f64;
        }
    }

    let Some(current) = current_analysis else {
        return plan;
    };

    let trailing_silence = current.energy.trailing_silence as f64;
    let effective_end = (current_duration - trailing_silence)
        .max(0.0)
        .min(current_duration);
    if effective_end <= MIN_CROSSFADE_DURATION {
        return plan;
    }

    plan.duration = choose_analysis_duration(config, current, effective_end);
    plan.curve = choose_analysis_curve(config, current);
    plan.overlap_headroom_db = choose_overlap_headroom(current_analysis, next_analysis);

    let baseline_start =
        analysis_baseline_start(current, current_duration, effective_end, plan.duration)
            .unwrap_or(plan.start_time);
    let candidate_start = selected_mix_candidate_start(config, current);
    let start = if let Some(candidate_start) = candidate_start {
        let max_shift = plan.duration.max(8.0).min(20.0);
        if (candidate_start - baseline_start).abs() <= max_shift {
            candidate_start
        } else {
            baseline_start
        }
    } else {
        baseline_start
    };

    plan.start_time = start;
    if config.emotional_transition {
        apply_section_cue(&mut plan, config, current, baseline_start, effective_end);
    }
    if config.bpm_match && config.beat_align && !skip_native_beat_align(current) {
        if let Some(bpm) = current.bpm.as_ref() {
            plan.start_time = align_to_nearest_beat(plan.start_time, bpm);
        }
    }
    if config.emotional_transition {
        if let Some(next) = next_analysis {
            apply_emotional_arc(&mut plan, config, current, next, effective_end);
        }
    }

    // Fit the pair before the legacy clamp shortens a late candidate: a real
    // pulse may move it earlier and recover the configured overlap budget.
    if config.bpm_match && config.beat_align && !skip_native_beat_align(current) {
        if let Some(next) = next_analysis {
            if let Some(aligned) =
                beat_plan::align(&plan, current, next, effective_end, config.bpm_match)
            {
                return aligned;
            }
        }
    }
    clamp_plan_to_content(plan, effective_end)
}

/// Longest fade the user's setting allows for content ending at `effective_end`.
fn configured_duration(config: &AutoMixConfig, effective_end: f64) -> f64 {
    let max_duration = (effective_end / 4.0).max(MIN_CROSSFADE_DURATION);
    config
        .crossfade_duration
        .clamp(MIN_CROSSFADE_DURATION, max_duration)
}

/// Move the fade to where the song hands over emotionally: the end of its
/// last chorus. The outro classifier describes the song's final seconds,
/// which a fade starting this early no longer blends, so its duration and
/// curve choices are dropped with it.
fn apply_section_cue(
    plan: &mut CrossfadePlan,
    config: &AutoMixConfig,
    current: &automix_analysis::TrackAnalysis,
    baseline_start: f64,
    effective_end: f64,
) {
    let Some(cue) = current.sections.as_ref().and_then(|sections| {
        emotion::section_mix_out_cue(
            sections,
            baseline_start as f32,
            effective_end as f32,
            MIN_CROSSFADE_DURATION as f32,
        )
    }) else {
        return;
    };
    let cue = f64::from(cue);
    plan.start_time = cue;
    plan.section_cue = Some(cue);
    plan.duration = configured_duration(config, effective_end);
    plan.curve = config.transition_style;
}

fn apply_emotional_arc(
    plan: &mut CrossfadePlan,
    config: &AutoMixConfig,
    current: &automix_analysis::TrackAnalysis,
    next: &automix_analysis::TrackAnalysis,
    effective_end: f64,
) {
    let Some((arc, profile)) =
        emotion::plan_arc(current, next, plan.start_time as f32, plan.duration as f32)
    else {
        return;
    };
    plan.arc = arc;
    plan.duration = (plan.duration * f64::from(profile.duration_scale)).clamp(
        MIN_CROSSFADE_DURATION,
        configured_duration(config, effective_end),
    );
    if let Some(curve) = profile.curve {
        plan.curve = curve;
    }
    plan.in_shape = f64::from(profile.in_shape);
    plan.out_shape = f64::from(profile.out_shape);
    if let Some(headroom) = profile.overlap_headroom_db {
        plan.overlap_headroom_db = plan.overlap_headroom_db.min(f64::from(headroom));
    }
}

fn choose_analysis_duration(
    config: &AutoMixConfig,
    current: &automix_analysis::TrackAnalysis,
    effective_end: f64,
) -> f64 {
    let configured = configured_duration(config, effective_end);

    if !config.smart_curve {
        return configured;
    }

    let Some(outro) = current.outro.as_ref() else {
        return configured;
    };

    match outro.outro_type.as_str() {
        "hard" if outro.outro_confidence >= 0.6 => configured.min(3.0).max(MIN_CROSSFADE_DURATION),
        "silence" if outro.outro_confidence >= 0.6 => {
            configured.min(4.0).max(MIN_CROSSFADE_DURATION)
        }
        "reverbTail" if outro.outro_confidence >= 0.6 => configured.min(5.0),
        "sustained" if outro.outro_confidence >= 0.6 => configured.min(6.0),
        "musicalOutro" if outro.outro_confidence >= 0.6 => configured.min(7.0),
        _ => configured,
    }
}

fn selected_mix_candidate_start(
    config: &AutoMixConfig,
    current: &automix_analysis::TrackAnalysis,
) -> Option<f64> {
    let mix = current.mix_candidates.as_ref()?;
    let selected = mix.candidates.iter().max_by(|a, b| {
        adjusted_mix_score(config, a)
            .partial_cmp(&adjusted_mix_score(config, b))
            .unwrap_or(std::cmp::Ordering::Equal)
    })?;

    if adjusted_mix_score(config, selected) >= 0.35 {
        Some(selected.time as f64)
    } else {
        None
    }
}

fn adjusted_mix_score(
    config: &AutoMixConfig,
    candidate: &automix_analysis::MixPointCandidate,
) -> f32 {
    if config.vocal_guard {
        candidate.score
    } else {
        (candidate.score + candidate.vocal_risk * 0.38).clamp(0.0, 1.0)
    }
}

fn choose_analysis_curve(
    config: &AutoMixConfig,
    current: &automix_analysis::TrackAnalysis,
) -> CrossfadeCurve {
    if !config.smart_curve {
        return config.transition_style;
    }

    let Some(outro) = current.outro.as_ref() else {
        return config.transition_style;
    };
    if outro.outro_confidence < 0.6 {
        return config.transition_style;
    }

    match outro.outro_type.as_str() {
        "slowDown" | "sustained" | "reverbTail" => CrossfadeCurve::SCurve,
        _ => CrossfadeCurve::EqualPower,
    }
}

fn choose_overlap_headroom(
    current_analysis: Option<&automix_analysis::TrackAnalysis>,
    next_analysis: Option<&automix_analysis::TrackAnalysis>,
) -> f64 {
    let Some(current) = current_analysis else {
        return DEFAULT_OVERLAP_HEADROOM_DB;
    };
    let Some(next) = next_analysis else {
        return DEFAULT_OVERLAP_HEADROOM_DB;
    };

    let similarity = spectral_similarity(&current.fingerprint.bands, &next.fingerprint.bands);
    if similarity < 0.45 {
        -2.0
    } else if similarity < 0.65 {
        -1.6
    } else {
        DEFAULT_OVERLAP_HEADROOM_DB
    }
}

fn spectral_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom <= 0.000001 {
        0.0
    } else {
        (dot / denom).clamp(0.0, 1.0)
    }
}

fn analysis_baseline_start(
    current: &automix_analysis::TrackAnalysis,
    current_duration: f64,
    effective_end: f64,
    duration: f64,
) -> Option<f64> {
    if let Some(outro) = current.outro.as_ref() {
        if outro.outro_confidence >= 0.55 {
            return Some(outro.suggested_crossfade_start as f64);
        }
    }

    let energy = &current.energy;
    if energy.is_fade_out {
        let outro_content_duration = energy.outro_start_offset - energy.trailing_silence;
        let fade_in_point = (outro_content_duration.max(0.0) * 0.5) as f64;
        return Some((current_duration - energy.trailing_silence as f64 - fade_in_point).max(0.0));
    }

    if energy.outro_start_offset > 0.0 {
        return Some((current_duration - energy.outro_start_offset as f64).max(0.0));
    }

    Some((effective_end - duration).max(0.0))
}

fn skip_native_beat_align(current: &automix_analysis::TrackAnalysis) -> bool {
    let Some(outro) = current.outro.as_ref() else {
        return false;
    };

    matches!(
        outro.outro_type.as_str(),
        "fadeOut" | "reverbTail" | "sustained" | "loopFade"
    )
}

fn align_to_nearest_beat(start_time: f64, bpm: &automix_analysis::BPMResult) -> f64 {
    if bpm.beat_grid.is_empty() || bpm.bpm <= 0.0 {
        return start_time;
    }

    let beat_interval = (60.0 / bpm.bpm).max(0.1) as f64;
    let max_shift = (beat_interval * 0.5).min(0.6);
    let mut best = start_time;
    let mut best_diff = f64::INFINITY;

    for &beat in &bpm.beat_grid {
        let time = (beat + bpm.analysis_offset) as f64;
        let diff = (time - start_time).abs();
        if diff < best_diff {
            best_diff = diff;
            best = time;
        }
    }

    if best_diff <= max_shift {
        best
    } else {
        start_time
    }
}

fn clamp_plan_to_content(mut plan: CrossfadePlan, effective_end: f64) -> CrossfadePlan {
    if effective_end <= 0.0 {
        plan.start_time = 0.0;
        plan.duration = MIN_CROSSFADE_DURATION;
        return plan;
    }

    let latest_start = (effective_end - MIN_CROSSFADE_DURATION).max(0.0);
    plan.start_time = plan.start_time.clamp(0.0, latest_start);
    let remaining = (effective_end - plan.start_time).max(0.0);
    if remaining < plan.duration {
        plan.duration = remaining.max(0.5);
    }
    plan.duration = plan.duration.max(0.5);
    plan
}
