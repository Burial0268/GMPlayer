use super::CrossfadePlan;
use crate::automix::TrackAnalysis;

/// Match actual local pulses. B keeps its original entry and its original speed.
pub(super) fn align(
    baseline: &CrossfadePlan,
    outgoing: &TrackAnalysis,
    incoming: &TrackAnalysis,
    effective_end: f64,
    allow_tempo: bool,
) -> Option<CrossfadePlan> {
    let a = outgoing.bpm.as_ref()?.tail.as_ref()?;
    let b = incoming.bpm.as_ref()?.head.as_ref()?;
    if a.stability < 0.9 || b.stability < 0.9 || a.period <= 0.0 || b.period <= 0.0 {
        return None;
    }
    let rate = [0.5, 1.0, 2.0]
        .into_iter()
        .map(|fold| a.period as f64 / b.period as f64 * fold)
        .min_by(|left, right| left.ln().abs().total_cmp(&right.ln().abs()))?;
    if !rate.is_finite() || (rate - 1.0).abs() > 0.04 + 1e-6 {
        return None;
    }
    if !allow_tempo && (rate - 1.0).abs() > 0.001 {
        return None;
    }
    let rate = if allow_tempo {
        rate.clamp(0.96, 1.04)
    } else {
        1.0
    };
    let incoming_beat = *b.beats.first()? as f64;
    // Preserve all of B's opening, including vocals before its first pulse.
    if incoming_beat > 2.0 || incoming_beat < 0.0 {
        return None;
    }
    let beat = (a.period as f64 / rate).max(b.period as f64);
    let group = beat * 4.0;
    let max_groups = (baseline.duration / group).floor() as usize;
    let a_end = (*a.beats.last()? as f64).min(effective_end);
    let b_end = *b.beats.last()? as f64;
    let mut best: Option<(f64, CrossfadePlan)> = None;
    for &anchor in &a.beats {
        let start = anchor as f64 - incoming_beat * rate;
        if start < *a.beats.first()? as f64 || (start - baseline.start_time).abs() > beat * 2.0 {
            continue;
        }
        for groups in (1..=max_groups).rev() {
            let duration = groups as f64 * group;
            if duration < 2.0 || start + duration * rate > a_end || duration > b_end {
                continue;
            }
            let cost = (duration - baseline.duration).abs() + (start - baseline.start_time).abs();
            if best.as_ref().is_some_and(|(old, _)| *old <= cost) {
                continue;
            }
            let mut plan = baseline.clone();
            plan.start_time = start;
            plan.duration = duration;
            plan.outgoing_rate = rate;
            plan.beat_synced = true;
            best = Some((cost, plan));
        }
    }
    best.map(|(_, plan)| plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automix::{BPMResult, BeatRegion};

    fn evidence(period: f32, first: f32, count: usize) -> BPMResult {
        let grid = BeatRegion {
            period,
            stability: 1.0,
            beats: (0..count).map(|i| first + i as f32 * period).collect(),
        };
        BPMResult {
            bpm: 60.0 / period,
            confidence: 1.0,
            beat_grid: Vec::new(),
            analysis_offset: 0.0,
            head: Some(grid.clone()),
            tail: Some(grid),
        }
    }

    #[test]
    fn a_pair_can_move_a_late_legacy_cue_earlier_before_its_duration_is_clamped() {
        let mut a = crate::automix::analyze_mono_samples(&vec![0.1; 48_000], 48_000, 1.0, false);
        let mut b = a.clone();
        a.duration = 16.0;
        a.outro = None;
        a.mix_candidates = None;
        a.energy.outro_start_offset = 4.0;
        a.energy.trailing_silence = 0.3;
        let mut a_beats = evidence(0.5, 0.154, 32);
        a_beats.beat_grid = vec![11.98];
        a.bpm = Some(a_beats);
        b.bpm = Some(evidence(0.4854, 0.154, 33));
        let mut config = crate::types::AutoMixConfig {
            enabled: true,
            crossfade_duration: 4.0,
            emotional_transition: false,
            smart_curve: false,
            volume_norm: false,
            ..Default::default()
        };
        let plan =
            super::super::plan::build_analysis_backed_plan(&config, 16.0, Some(&a), Some(&b));
        assert!(plan.beat_synced);
        assert!(plan.duration > 3.8);
        assert!(plan.start_time < 11.98);
        assert!(plan.start_time + plan.duration * plan.outgoing_rate < 15.7);
        config.bpm_match = false;
        let disabled =
            super::super::plan::build_analysis_backed_plan(&config, 16.0, Some(&a), Some(&b));
        assert!(!disabled.beat_synced);
        assert_eq!(disabled.outgoing_rate, 1.0);
    }

    #[test]
    fn both_pulse_phase_and_tempo_match_without_skipping_bs_opening() {
        let config = crate::types::AutoMixConfig::default();
        let base = super::super::plan::build_time_based_plan(&config, 100.0);
        let mut a = crate::automix::analyze_mono_samples(&vec![0.1; 48_000], 48_000, 1.0, false);
        let mut b = a.clone();
        a.bpm = Some(evidence(0.5, 70.13, 60));
        b.bpm = Some(evidence(0.49, 0.17, 70));
        let plan = align(&base, &a, &b, 100.0, true).unwrap();
        assert!((plan.outgoing_rate - 0.5 / 0.49).abs() < 1e-6);
        let a_pulse = plan.start_time + 0.17 * plan.outgoing_rate;
        let phase = (a_pulse - 70.13).rem_euclid(0.5);
        assert!(phase.min(0.5 - phase) < 1e-4);
        assert!(align(&base, &a, &b, 100.0, false).is_none());
        b.bpm = Some(evidence(0.4, 0.17, 70));
        assert!(align(&base, &a, &b, 100.0, true).is_none());
    }
}
