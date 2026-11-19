//! Paired plans counted in bars. A leaves on a downbeat, preferably at a
//! four-bar phrase from its outro; B enters on its own first downbeat (or one
//! overlap before its first line); the handoff waits for A's last vocal line
//! and lands on a bar line. Matched tempos beat-mix with a bass swap at the
//! handoff; tempos too far apart to stretch filter-cut instead. The planning
//! follows HyPlayer-Nextgen's `AutoMixPlanner` (GPL-3.0), re-derived for our
//! pulse grids, which know the bar only when the low band accents one beat.

use super::plan::{vocal_runs, CrossfadePlan};
use crate::automix::{BeatRegion, TrackAnalysis};
use crate::player::mixer::{BassSwap, FadeStyle};

const MAX_OVERLAP: f64 = 40.0;
/// Shortest overlap a beat-mix aims for, however short the crossfade setting.
const MIN_TARGET: f64 = 8.0;
const BEAT_MIX_BARS: [usize; 5] = [16, 8, 4, 3, 2];
/// Without a tempo match the overlap stays short: nothing holds the two together.
const FILTER_CUT_BARS: [usize; 3] = [8, 4, 2];
/// Seconds of both sides singing at once that a plan may contain.
const MAX_VOCAL_CLASH: f64 = 1.0;
/// How far before the content end an exit may be.
pub(super) const EXIT_WINDOW: f64 = 45.0;
/// The swap reads as a step; this only keeps the shelf change from clicking.
const SWAP_RAMP_SECONDS: f64 = 0.03;

pub(super) fn align(
    baseline: &CrossfadePlan,
    outgoing: &TrackAnalysis,
    incoming: &TrackAnalysis,
    effective_end: f64,
    allow_tempo: bool,
    crossfade_duration: f64,
) -> Option<CrossfadePlan> {
    let a = outgoing.bpm.as_ref()?.tail.as_ref()?;
    let b = incoming.bpm.as_ref()?.head.as_ref()?;
    if a.stability < 0.9 || b.stability < 0.9 || a.period <= 0.0 || b.period <= 0.0 {
        return None;
    }
    let (a_period, b_period) = (f64::from(a.period), f64::from(b.period));
    let rate = [0.5, 1.0, 2.0]
        .into_iter()
        .map(|fold| a_period / (b_period * fold))
        .min_by(|left, right| left.ln().abs().total_cmp(&right.ln().abs()))?;
    let limit = if allow_tempo { 0.04 } else { 0.001 };
    let synced = rate.is_finite() && (rate - 1.0).abs() <= limit + 1e-6;
    let (rate, bar_counts): (f64, &[usize]) = if synced && allow_tempo {
        (rate.clamp(0.96, 1.04), &BEAT_MIX_BARS)
    } else if synced {
        (1.0, &BEAT_MIX_BARS)
    } else {
        (1.0, &FILTER_CUT_BARS)
    };
    // One bar of A in output seconds, which a synced B matches.
    let bar = 4.0 * a_period / rate;
    let a_end = (f64::from(*a.beats.last()?) + a_period * 0.5).min(effective_end);
    let b_end = f64::from(*b.beats.last()?) + b_period * 0.5;
    let a_downbeats = downbeats(a);
    let b_downbeats = downbeats(b);
    let phrase = phrase_anchor(outgoing, &a_downbeats, a.downbeat.is_some());
    let a_vocals = vocal_runs(outgoing, 0.8);
    let b_vocals = vocal_runs(incoming, 0.4);
    let last_line = a_vocals
        .iter()
        .rev()
        .map(|run| run.1.min(effective_end))
        .find(|end| *end > effective_end - 60.0);
    let first_b = *b_downbeats.first()?;
    let first_line_b = b_vocals
        .iter()
        .map(|run| run.0)
        .find(|start| *start >= first_b - 0.5);
    let tolerance = a_period.min(0.8);
    // Bar counts nearest twice the crossfade setting first (fewer bars on a tie): a club-length
    // 16 bars at 128 BPM is a 30 s overlap, far longer than a listener asked for.
    let target = (crossfade_duration * 2.0).clamp(MIN_TARGET, MAX_OVERLAP);
    let mut bar_counts = bar_counts.to_vec();
    bar_counts.sort_by(|x, y| {
        let off = |bars: usize| (bars as f64 * bar - target).abs();
        off(*x).total_cmp(&off(*y)).then(x.cmp(y))
    });

    for bars in bar_counts {
        let duration = bars as f64 * bar;
        if !(2.0..=MAX_OVERLAP).contains(&duration) {
            continue;
        }
        let span = duration * rate;
        let entry = choose_entry(&b_downbeats, first_line_b, duration, bar, incoming.duration);
        if entry + duration > b_end {
            continue;
        }
        let Some(exit) = choose_exit(
            &a_downbeats,
            phrase,
            baseline,
            effective_end,
            a_end - span,
            bar,
        ) else {
            continue;
        };
        // A still singing after the overlap would be cut off mid-line.
        if last_line.is_some_and(|end| end > exit + span + tolerance) {
            continue;
        }
        let handoff = handoff(last_line, exit, span, bars);
        let audible = duration * (handoff + 0.7 * (1.0 - handoff));
        if vocal_clash(&a_vocals, &b_vocals, exit, rate, entry, audible) > MAX_VOCAL_CLASH {
            continue;
        }
        let mut plan = baseline.clone();
        plan.start_time = exit;
        plan.duration = duration;
        plan.outgoing_rate = rate;
        plan.incoming_start = entry;
        plan.in_shape = 1.0;
        plan.out_shape = 1.0;
        if synced {
            plan.beat_synced = true;
            plan.style = FadeStyle::BeatMix(handoff as f32);
            let ramp = SWAP_RAMP_SECONDS / duration;
            plan.bass_swap = Some(BassSwap {
                start: (handoff - ramp) as f32,
                end: handoff as f32,
            });
        } else {
            plan.beat_synced = false;
            plan.style = FadeStyle::FilterCut;
            plan.bass_swap = None;
        }
        return Some(plan);
    }
    None
}

/// Bar starts when the grid knows them, otherwise every pulse.
fn downbeats(grid: &BeatRegion) -> Vec<f64> {
    let (first, step) = grid.downbeat.map_or((0, 1), |d| (usize::from(d), 4));
    grid.beats
        .iter()
        .skip(first)
        .step_by(step)
        .map(|t| f64::from(*t))
        .collect()
}

/// Index of the first bar at or after the outro, from which phrases count.
fn phrase_anchor(track: &TrackAnalysis, downbeats: &[f64], bars_known: bool) -> Option<usize> {
    if !bars_known {
        return None;
    }
    let outro = track
        .outro
        .as_ref()
        .and_then(|outro| outro.outro_section_start)
        .map(f64::from)
        .or_else(|| {
            (track.energy.outro_start_offset > 0.0)
                .then(|| f64::from(track.duration - track.energy.outro_start_offset))
        })?;
    downbeats.iter().position(|t| *t >= outro - 0.05)
}

/// The latest bar that leaves room for the overlap, on a phrase if one fits.
/// An emotional section cue has already chosen the moment; stay within a bar.
fn choose_exit(
    downbeats: &[f64],
    phrase: Option<usize>,
    baseline: &CrossfadePlan,
    effective_end: f64,
    latest: f64,
    bar: f64,
) -> Option<f64> {
    let earliest = (effective_end - EXIT_WINDOW).max(0.0);
    let candidates = downbeats
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, t)| *t <= latest + 0.05);
    if let Some(cue) = baseline.section_cue {
        return candidates
            .map(|(_, t)| t)
            .filter(|t| (t - cue).abs() <= bar)
            .min_by(|a, b| (a - cue).abs().total_cmp(&(b - cue).abs()));
    }
    let candidates: Vec<(usize, f64)> = candidates.filter(|(_, t)| *t >= earliest).collect();
    phrase
        .and_then(|p| {
            candidates
                .iter()
                .rev()
                .find(|(i, _)| (*i as isize - p as isize).rem_euclid(4) == 0)
        })
        .or(candidates.last())
        .map(|(_, t)| *t)
}

/// B's first downbeat, or later when its first line is far off: then the
/// overlap ends as B starts singing, instead of laying a long intro under A.
fn choose_entry(
    downbeats: &[f64],
    first_line: Option<f64>,
    duration: f64,
    bar: f64,
    track_duration: f32,
) -> f64 {
    let first = downbeats[0];
    let Some(line) = first_line.filter(|line| line - first >= duration + 2.0 * bar) else {
        return first;
    };
    downbeats
        .iter()
        .rev()
        .copied()
        .find(|t| *t <= line - duration + 0.25 * bar && *t > first + bar)
        .filter(|t| *t <= 0.4 * f64::from(track_duration))
        .unwrap_or(first)
}

/// Where A lets go: after its last line, on a bar line, mid-fade by default.
fn handoff(last_line: Option<f64>, exit: f64, span: f64, bars: usize) -> f64 {
    let wanted = last_line
        .filter(|end| *end > exit)
        .map_or(0.5, |end| ((end - exit) / span).clamp(0.5, 0.9));
    let bars = bars as f64;
    let latest = (bars - 1.0) / bars;
    ((wanted * bars).ceil() / bars)
        .min(latest)
        .max(0.5_f64.min(latest))
}

/// Seconds, in output time, during which both sides are singing.
fn vocal_clash(
    a: &[(f64, f64)],
    b: &[(f64, f64)],
    exit: f64,
    rate: f64,
    entry: f64,
    audible: f64,
) -> f64 {
    const STEP: f64 = 0.1;
    let sings = |runs: &[(f64, f64)], t: f64| runs.iter().any(|(from, to)| t >= *from && t < *to);
    (0..(audible / STEP) as usize)
        .map(|i| i as f64 * STEP)
        .filter(|tau| sings(a, exit + tau * rate) && sings(b, entry + tau))
        .count() as f64
        * STEP
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automix::{BPMResult, VocalActivityAnalysis};

    fn grid(period: f32, first: f32, count: usize, downbeat: Option<u8>) -> BeatRegion {
        BeatRegion {
            period,
            stability: 1.0,
            beats: (0..count).map(|i| first + i as f32 * period).collect(),
            downbeat,
        }
    }

    /// A 100 s song whose last 40 s carry `a_period` pulses, and a B whose first
    /// 40 s carry `b_period` pulses starting at `b_first`.
    fn pair(a_period: f32, b_period: f32, b_first: f32) -> (TrackAnalysis, TrackAnalysis) {
        let base = crate::automix::analyze_mono_samples(&vec![0.1; 48_000], 48_000, 1.0, false);
        let (mut a, mut b) = (base.clone(), base);
        a.duration = 100.0;
        b.duration = 200.0;
        let a_count = (40.0 / a_period) as usize;
        let tail = grid(
            a_period,
            100.0 - a_count as f32 * a_period,
            a_count,
            Some(0),
        );
        let head = grid(
            b_period,
            b_first,
            ((40.0 - b_first) / b_period) as usize,
            Some(0),
        );
        a.bpm = Some(BPMResult {
            bpm: 60.0 / a_period,
            confidence: 1.0,
            beat_grid: Vec::new(),
            analysis_offset: 0.0,
            head: None,
            tail: Some(tail),
        });
        b.bpm = Some(BPMResult {
            bpm: 60.0 / b_period,
            confidence: 1.0,
            beat_grid: Vec::new(),
            analysis_offset: 0.0,
            head: Some(head),
            tail: None,
        });
        (a, b)
    }

    fn sings(track: &mut TrackAnalysis, runs: &[(f32, f32)]) {
        let windows = (track.duration / 0.25) as usize;
        let risk = (0..windows)
            .map(|i| {
                let t = i as f32 * 0.25;
                if runs.iter().any(|(from, to)| t >= *from && t < *to) {
                    0.9
                } else {
                    0.0
                }
            })
            .collect();
        track.vocal_activity = Some(VocalActivityAnalysis {
            window_duration: 0.25,
            risk,
            confidence: 0.8,
            method: "test".into(),
        });
    }

    fn baseline() -> CrossfadePlan {
        super::super::plan::build_time_based_plan(&crate::types::AutoMixConfig::default(), 100.0)
    }

    #[test]
    fn matched_tempos_beat_mix_eight_bars_from_bs_first_downbeat() {
        let (a, b) = pair(0.469, 0.48, 1.7);
        let plan = align(&baseline(), &a, &b, 100.0, true, 8.0).unwrap();
        assert_eq!(plan.style, FadeStyle::BeatMix(0.5));
        assert!(plan.beat_synced);
        assert!((plan.outgoing_rate - 0.469 / 0.48).abs() < 1e-6);
        // At 125 BPM and the default 8 s setting: 8 bars (15 s), not a club-length 16.
        assert!((plan.duration - 8.0 * 4.0 * 0.48).abs() < 1e-3);
        // A long setting earns the long mix.
        let long = align(&baseline(), &a, &b, 100.0, true, 16.0).unwrap();
        assert!((long.duration - 16.0 * 4.0 * 0.48).abs() < 1e-3);
        assert!(
            (plan.incoming_start - 1.7).abs() < 1e-6,
            "B enters on its first downbeat"
        );
        // A leaves on one of its bar starts and the overlap stays inside its grid.
        let a_grid = a.bpm.as_ref().unwrap().tail.as_ref().unwrap();
        let bars_in = (plan.start_time - f64::from(a_grid.beats[0])) / (4.0 * 0.469);
        assert!((bars_in - bars_in.round()).abs() < 1e-3);
        assert!(plan.start_time + plan.duration * plan.outgoing_rate <= 100.0 + 1e-6);
        let swap = plan.bass_swap.unwrap();
        assert_eq!(swap.end, 0.5);
    }

    #[test]
    fn a_late_last_line_moves_the_handoff_onto_a_later_bar() {
        let (mut a, b) = pair(0.5, 0.5, 0.0);
        let free = align(&baseline(), &a, &b, 100.0, true, 8.0).unwrap();
        // A sings until three quarters into where the free plan would overlap.
        let end = free.start_time + free.duration * 0.72;
        sings(&mut a, &[(40.0, end as f32)]);
        let plan = align(&baseline(), &a, &b, 100.0, true, 8.0).unwrap();
        let FadeStyle::BeatMix(handoff) = plan.style else {
            panic!("{:?}", plan.style)
        };
        let reached = (end - plan.start_time) / plan.duration;
        assert!(
            f64::from(handoff) >= reached,
            "A keeps its line: {handoff} < {reached}"
        );
        let bars = plan.duration / 2.0;
        assert!(
            (f64::from(handoff) * bars).fract().abs() < 1e-6,
            "on a bar line"
        );
    }

    #[test]
    fn two_singers_shorten_the_overlap_until_they_barely_meet() {
        let (mut a, mut b) = pair(0.5, 0.5, 0.0);
        sings(&mut a, &[(40.0, 92.0)]);
        sings(&mut b, &[(4.0, 60.0)]);
        let plan = align(&baseline(), &a, &b, 100.0, true, 8.0).unwrap();
        let a_runs = vocal_runs(&a, 0.8);
        let b_runs = vocal_runs(&b, 0.4);
        let FadeStyle::BeatMix(handoff) = plan.style else {
            panic!()
        };
        let audible = plan.duration * (f64::from(handoff) + 0.7 * (1.0 - f64::from(handoff)));
        let clash = vocal_clash(
            &a_runs,
            &b_runs,
            plan.start_time,
            1.0,
            plan.incoming_start,
            audible,
        );
        assert!(clash <= MAX_VOCAL_CLASH, "clash={clash}");
        assert!(
            plan.duration < 8.0 * 2.0,
            "shorter than the eight bars it aims for"
        );
    }

    #[test]
    fn a_long_instrumental_intro_enters_one_overlap_before_bs_first_line() {
        let (a, mut b) = pair(0.5, 0.5, 0.0);
        sings(&mut b, &[(36.0, 60.0)]);
        let plan = align(&baseline(), &a, &b, 100.0, true, 8.0).unwrap();
        assert!(plan.incoming_start > 0.0);
        assert!(plan.incoming_start + plan.duration <= 36.0 + 0.5 + 1e-6);
    }

    #[test]
    fn tempos_out_of_stretch_range_filter_cut_at_native_speed() {
        let (a, b) = pair(0.469, 0.6, 0.0);
        let plan = align(&baseline(), &a, &b, 100.0, true, 8.0).unwrap();
        assert_eq!(plan.style, FadeStyle::FilterCut);
        assert_eq!(plan.outgoing_rate, 1.0);
        assert!(!plan.beat_synced && plan.bass_swap.is_none());
        assert!(plan.duration <= 8.0 * 4.0 * 0.469 + 1e-6);
        // With BPM matching off, a near match may not be stretched either.
        let (a, b) = pair(0.469, 0.48, 0.0);
        assert_eq!(
            align(&baseline(), &a, &b, 100.0, false, 8.0).unwrap().style,
            FadeStyle::FilterCut
        );
    }

    #[test]
    fn without_a_known_bar_every_pulse_may_anchor() {
        let (mut a, mut b) = pair(0.5, 0.5, 0.3);
        for grid in [
            a.bpm.as_mut().unwrap().tail.as_mut(),
            b.bpm.as_mut().unwrap().head.as_mut(),
        ] {
            grid.unwrap().downbeat = None;
        }
        let plan = align(&baseline(), &a, &b, 100.0, true, 8.0).unwrap();
        assert!((plan.incoming_start - 0.3).abs() < 1e-6);
        let phase = (plan.start_time - 60.0).rem_euclid(0.5);
        assert!(phase.min(0.5 - phase) < 1e-3);
    }
}
