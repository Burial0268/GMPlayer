//! Pure transition planning from compact track analysis.

use super::beat_plan;
use crate::automix as automix_analysis;
use crate::automix::emotion::{self, EmotionalArc};
use crate::player::mixer::BassSwap;
use crate::types::{AutoMixConfig, CrossfadeCurve};

const MIN_CROSSFADE_DURATION: f64 = 2.0;
const DEFAULT_OVERLAP_HEADROOM_DB: f64 = -1.2;
/// Share of an overlap's vocal windows that makes a side "singing" / "clear".
const VOCAL_BUSY: f32 = 0.5;
const VOCAL_CLEAR: f32 = 0.25;
/// `VocalActivityAnalysis` risk at which a window counts as vocal.
const VOCAL_WINDOW_ACTIVE: f32 = 0.55;
/// Mean low-band level both sides need before two basslines would stack.
const BASS_PRESENT: f32 = 0.01;

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
    pub bass_swap: Option<BassSwap>,
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
        curve: CrossfadeCurve::EqualPower,
        in_shape: 1.0,
        out_shape: 1.0,
        arc: EmotionalArc::Neutral,
        section_cue: None,
        outgoing_rate: 1.0,
        beat_synced: false,
        bass_swap: None,
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
    plan.curve = choose_analysis_curve(current);
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
    // No outro-type skip here: a fade-out usually keeps its drums, and the pair
    // fit already refuses an overlap A's measured grid does not cover.
    let aligned = next_analysis
        .filter(|_| config.bpm_match && config.beat_align)
        .and_then(|next| beat_plan::align(&plan, current, next, effective_end, config.bpm_match));
    let mut plan = aligned.unwrap_or_else(|| clamp_plan_to_content(plan, effective_end));
    if let Some(next) = next_analysis {
        shape_for_pair(&mut plan, current, next);
    }
    plan
}

/// Auto-curve from what actually overlaps: two vocals hand over quickly, a
/// singer arriving clears the old song first, a quiet opening comes in early,
/// and two basslines swap instead of stacking. Exponents compose, so this
/// scales whatever the outro or arc chose.
fn shape_for_pair(
    plan: &mut CrossfadePlan,
    current: &automix_analysis::TrackAnalysis,
    next: &automix_analysis::TrackAnalysis,
) {
    let outgoing = (plan.start_time, plan.start_time + plan.duration * plan.outgoing_rate);
    let incoming = (0.0, plan.duration);
    let (in_scale, out_scale) = pair_shape_scale(
        vocal_share(current, outgoing),
        vocal_share(next, incoming),
        mean_energy(current, outgoing),
        mean_energy(next, incoming),
    );
    if in_scale != 1.0 {
        plan.in_shape = (plan.in_shape * in_scale).clamp(0.7, 1.3);
    }
    if out_scale != 1.0 {
        plan.out_shape = (plan.out_shape * out_scale).clamp(0.7, 1.3);
    }
    plan.bass_swap = choose_bass_swap(plan, current, next, outgoing, incoming);
}

/// (incoming, outgoing) exponent scales. Mirrored by `pairShapeScale` in
/// `AutoMix/curves.ts`; keep the thresholds in step.
fn pair_shape_scale(
    vocal_out: Option<f32>,
    vocal_in: Option<f32>,
    energy_out: Option<f32>,
    energy_in: Option<f32>,
) -> (f64, f64) {
    let (mut in_scale, mut out_scale) = (1.0, 1.0);
    if let (Some(a), Some(b)) = (vocal_out, vocal_in) {
        if a >= VOCAL_BUSY && b >= VOCAL_BUSY {
            in_scale *= 1.2;
            out_scale *= 1.2;
        } else if b >= VOCAL_BUSY && a < VOCAL_CLEAR {
            out_scale *= 1.2;
        } else if a >= VOCAL_BUSY && b < VOCAL_CLEAR {
            in_scale *= 0.85;
        }
    }
    if let (Some(a), Some(b)) = (energy_out, energy_in) {
        let ratio = b / a.max(1e-3);
        if ratio < 0.5 {
            in_scale *= 0.85;
        } else if ratio > 2.0 {
            in_scale *= 1.15;
        }
    }
    (in_scale, out_scale)
}

fn vocal_share(track: &automix_analysis::TrackAnalysis, (from, to): (f64, f64)) -> Option<f32> {
    let vocal = track.vocal_activity.as_ref()?;
    let width = f64::from(vocal.window_duration);
    if width <= 0.0 {
        return None;
    }
    let first = (from / width).max(0.0) as usize;
    let last = ((to / width).ceil() as usize).min(vocal.risk.len());
    let windows = vocal.risk.get(first..last).filter(|w| !w.is_empty())?;
    let active = windows.iter().filter(|risk| **risk >= VOCAL_WINDOW_ACTIVE).count();
    Some(active as f32 / windows.len() as f32)
}

fn mean_energy(track: &automix_analysis::TrackAnalysis, (from, to): (f64, f64)) -> Option<f32> {
    let seconds = &track.energy.energy_per_second;
    let first = from.max(0.0) as usize;
    let last = (to.ceil() as usize).min(seconds.len());
    let span = seconds.get(first..last).filter(|s| !s.is_empty())?;
    Some(span.iter().sum::<f32>() / span.len() as f32)
}

/// Swap the low band when both sides carry one. A beat-synced pair swaps over
/// the beat leading into B's pulse nearest the middle, so the change lands on
/// a kick; otherwise over the middle fifth, as the Web path does.
fn choose_bass_swap(
    plan: &CrossfadePlan,
    current: &automix_analysis::TrackAnalysis,
    next: &automix_analysis::TrackAnalysis,
    (out_from, out_to): (f64, f64),
    (in_from, in_to): (f64, f64),
) -> Option<BassSwap> {
    let outro = current.outro.as_ref()?;
    let low = &outro.multiband_energy.low;
    // Outro windows end at the content end, 0.25 s each.
    let origin = f64::from(current.duration - current.energy.trailing_silence) - low.len() as f64 * 0.25;
    let a_low = mean_window(low, (out_from - origin) / 0.25, (out_to - origin) / 0.25)?;
    let intro = next.intro.as_ref()?.multiband_energy.as_ref()?;
    let b_low = mean_window(&intro.low, in_from / 0.25, in_to / 0.25)?;
    if a_low < BASS_PRESENT || b_low < BASS_PRESENT {
        return None;
    }
    let beat = plan
        .beat_synced
        .then(|| next.bpm.as_ref()?.head.as_ref())
        .flatten()
        .and_then(|grid| {
            let middle = plan.duration * 0.5;
            let pulse = grid
                .beats
                .iter()
                .map(|t| f64::from(*t))
                .filter(|t| *t >= plan.duration * 0.3 && *t <= plan.duration * 0.8)
                .min_by(|a, b| (a - middle).abs().total_cmp(&(b - middle).abs()))?;
            let end = pulse / plan.duration;
            let start = (pulse - f64::from(grid.period)) / plan.duration;
            Some(BassSwap { start: start as f32, end: end as f32 })
        });
    Some(beat.unwrap_or(BassSwap { start: 0.4, end: 0.6 }))
}

fn mean_window(values: &[f32], from: f64, to: f64) -> Option<f32> {
    let first = from.max(0.0) as usize;
    let last = (to.ceil().max(0.0) as usize).min(values.len());
    let span = values.get(first..last).filter(|s| !s.is_empty())?;
    Some(span.iter().sum::<f32>() / span.len() as f32)
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
    plan.curve = CrossfadeCurve::EqualPower;
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

fn choose_analysis_curve(current: &automix_analysis::TrackAnalysis) -> CrossfadeCurve {
    let Some(outro) = current.outro.as_ref() else {
        return CrossfadeCurve::EqualPower;
    };
    if outro.outro_confidence < 0.6 {
        return CrossfadeCurve::EqualPower;
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

#[cfg(test)]
mod pair_shape_tests {
    use super::*;

    #[test]
    fn two_singers_hand_over_quickly_and_one_arriving_clears_the_way() {
        assert_eq!(pair_shape_scale(Some(0.8), Some(0.7), None, None), (1.2, 1.2));
        assert_eq!(pair_shape_scale(Some(0.1), Some(0.7), None, None), (1.0, 1.2));
        assert_eq!(pair_shape_scale(Some(0.7), Some(0.1), None, None), (0.85, 1.0));
        // In between neither side is clearly singing or clear: leave it alone.
        assert_eq!(pair_shape_scale(Some(0.4), Some(0.4), None, None), (1.0, 1.0));
        assert_eq!(pair_shape_scale(None, None, None, None), (1.0, 1.0));
    }

    #[test]
    fn a_quiet_opening_comes_in_early_and_a_loud_one_is_held_back() {
        assert_eq!(pair_shape_scale(None, None, Some(0.8), Some(0.3)), (0.85, 1.0));
        assert_eq!(pair_shape_scale(None, None, Some(0.3), Some(0.8)), (1.15, 1.0));
        assert_eq!(pair_shape_scale(None, None, Some(0.6), Some(0.7)), (1.0, 1.0));
    }
}
