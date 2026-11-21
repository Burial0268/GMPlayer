//! Pure transition planning from compact track analysis.

use super::beat_plan;
use crate::automix as automix_analysis;
use crate::automix::emotion::{self, EmotionalArc};
use crate::decoder::tempo::RATE_RANGE;
use crate::player::mixer::{BandMorph, BandStage, BassSwap, EchoOut, FadeStyle, ECHO_MAX_DELAY};
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
/// Liquidsoap autocue's thresholds: A's decay starts where it falls `TAIL_DROP_LU` below its loud
/// level, or `LONG_TAIL_DROP_LU` when that tail would be longer than `LONG_TAIL_SECONDS`.
const TAIL_DROP_LU: f32 = 8.0;
const LONG_TAIL_DROP_LU: f32 = 20.0;
const LONG_TAIL_SECONDS: f64 = 15.0;
/// Overlap for two songs that are both loud at the seam (Liquidsoap `cross.smart`).
const LOUD_PAIR_SECONDS: f64 = 4.0;
/// Shorter than this, B opens at full level and there is no lead-in to lay under A.
const MIN_LEAD_IN: f64 = 0.5;
/// How far B's beat may sit from A's for B's first beat to continue A's pulse at B's spacing.
const MEET_STEP_TOLERANCE: f64 = 0.1;
/// How fast an echo-out into B's first beat opens.
const ECHO_OUT_ATTACK: f64 = 0.03;
/// Longest tail after A's drop that may ring out under B: B is at full from its first beat, so a
/// longer, still-audible decay would stack on it; that keeps its measured fade.
const MEET_MAX_TAIL: f64 = 4.0;
/// B's beat, in seconds, when its head has no grid: 120 BPM.
const FALLBACK_BEAT: f64 = 0.5;
/// Beats over which A eases into B's tempo before B's first beat: two bars.
const RIDE_BEATS: f64 = 8.0;
/// Tempo gap under which B's first beat simply continues A's pulse at B's spacing; one longer beat
/// is not heard below it.
const RIDE_MIN_CHANGE: f64 = 0.02;

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
    /// A's source seconds over which it eases from its own speed to `outgoing_rate`; without one
    /// the rate applies from `start_time`.
    pub tempo_ramp: Option<(f64, f64)>,
    pub beat_synced: bool,
    pub bass_swap: Option<BassSwap>,
    pub morph: Option<BandMorph>,
    pub stage: Option<BandStage>,
    pub echo: Option<EchoOut>,
    pub style: FadeStyle,
    /// B's source second the overlap starts from: its first downbeat on a
    /// paired plan, so an intro before the beat is not laid under A.
    pub incoming_start: f64,
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
        tempo_ramp: None,
        beat_synced: false,
        bass_swap: None,
        morph: None,
        stage: None,
        echo: None,
        style: FadeStyle::Blend,
        incoming_start: 0.0,
    }
}

/// `outgoing_gain` is the gain the outgoing deck is playing at now (`active_norm_gain`).
pub(super) fn build_analysis_backed_plan(
    config: &AutoMixConfig,
    current_duration: f64,
    current_analysis: Option<&automix_analysis::TrackAnalysis>,
    next_analysis: Option<&automix_analysis::TrackAnalysis>,
    outgoing_gain: f64,
) -> CrossfadePlan {
    let mut plan = build_time_based_plan(config, current_duration);
    plan.incoming_gain_adjustment =
        incoming_gain(config, outgoing_gain, current_analysis, next_analysis);

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

    let measured = loudness_placement(config, current, next_analysis, effective_end);
    let baseline_start = match measured {
        Some((start, duration)) => {
            plan.duration = duration;
            start
        }
        None => analysis_baseline_start(current, current_duration, effective_end, plan.duration)
            .unwrap_or(plan.start_time),
    };
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
            let end = plan.start_time + plan.duration;
            apply_emotional_arc(&mut plan, config, current, next, effective_end);
            // A fade placed over A's decay keeps its end when the arc shortens it, or A's last
            // seconds would play at zero gain.
            if measured.is_some() && plan.section_cue.is_none() {
                plan.start_time = end - plan.duration;
            }
        }
    }

    // Fit the pair before the legacy clamp shortens a late candidate: a real
    // pulse may move it earlier and recover the configured overlap budget.
    // No outro-type skip here: a fade-out usually keeps its drums, and the pair
    // fit already refuses an overlap A's measured grid does not cover.
    // Where A's measured full level stops, and the longest fade a placement may take.
    let placed = (measured.is_some() && plan.section_cue.is_none())
        .then(|| (plan.start_time, longest_fade(config, effective_end)));
    let aligned = next_analysis
        .filter(|_| config.bpm_match && config.beat_align)
        .and_then(|next| {
            beat_plan::align(
                &plan,
                current,
                next,
                effective_end,
                config.bpm_match,
                config.crossfade_duration,
                placed,
            )
            .map(|aligned| (aligned, next))
        });
    if let Some((mut aligned, next)) = aligned {
        // FilterCut's sweep is its own tonal handover.
        if aligned.style != FadeStyle::FilterCut {
            aligned.morph = choose_morph(&aligned, current, next);
        }
        aligned.stage = choose_stage(&aligned);
        aligned.echo = choose_echo(config, current, &aligned);
        return aligned;
    }
    let mut plan = clamp_plan_to_content(plan, effective_end);
    let mut met = None;
    if let Some(next) = next_analysis {
        met = meet_on_arrival(&mut plan, config, current, next, effective_end);
        shape_for_pair(&mut plan, current, next);
        // Whether the bass swaps is still `choose_bass_swap`'s call; when is B's first beat.
        if let Some(meet) = &met {
            plan.bass_swap = plan.bass_swap.map(|_| meet.swap);
        }
    }
    hold_for_last_vocal(&mut plan, current);
    plan.stage = choose_stage(&plan);
    plan.echo = match &met {
        Some(meet) => echo_into_arrival(config, &plan, meet),
        None => choose_echo(config, current, &plan),
    };
    plan
}

/// How hard the echo-out throws, the Web's `1 - compatibility`; native plans keep no
/// compatibility score.
/// shortcut: one fixed intensity, upgrade when native plans score pair compatibility.
const ECHO_INTENSITY: f32 = 0.5;

/// An echo-out where nothing else carries A out: a filter-cut (tempos too far apart to blend
/// beats) or a short fade off an abrupt ending. Off without transition effects, and for the calm
/// arcs, as on the Web. Parameters are the Web echo throw's (`TransitionEffects.ts`): an
/// eighth-note delay from A's tempo, and a duration of about half the fade, 1 to 4 s.
pub(super) fn choose_echo(
    config: &AutoMixConfig,
    current: &automix_analysis::TrackAnalysis,
    plan: &CrossfadePlan,
) -> Option<EchoOut> {
    if !config.transition_effects
        || matches!(plan.arc, EmotionalArc::Afterglow | EmotionalArc::Drift)
    {
        return None;
    }
    let hard = current
        .outro
        .as_ref()
        .is_some_and(|outro| outro.outro_type == "hard" && outro.outro_confidence >= 0.6)
        // A section cue moved the fade off the ending the outro type describes.
        && plan.section_cue.is_none();
    if plan.style != FadeStyle::FilterCut && !(hard && plan.duration < 4.0) {
        return None;
    }
    let beat = current.bpm.as_ref().and_then(|bpm| {
        bpm.tail
            .as_ref()
            .map(|grid| grid.period)
            .filter(|period| *period > 0.0)
            .or((bpm.bpm > 0.0).then(|| 60.0 / bpm.bpm))
    });
    let intensity = ECHO_INTENSITY;
    let duration = (plan.duration as f32 * 0.55).clamp(1.0, 4.0);
    Some(EchoOut {
        delay: beat.map_or(0.32, |beat| (beat * 0.5).clamp(0.16, ECHO_MAX_DELAY)),
        feedback: 0.22 + intensity * 0.18,
        cutoff_hz: 4_200.0 - intensity * 1_800.0,
        wet: 10f32.powf((-22.0 + intensity * 8.0) / 20.0),
        start: 0.0,
        attack: duration * 0.15,
        duration,
    })
}

/// Staged entry for every style but `FilterCut` (whose sweep is its own handover). The crossover
/// is where B takes the bass — the swap's end — or A's handoff when A holds the floor longer,
/// since A's highs must not dull while it is still singing.
pub(in crate::player) fn choose_stage(plan: &CrossfadePlan) -> Option<BandStage> {
    let handoff = match plan.style {
        FadeStyle::FilterCut => return None,
        FadeStyle::Handoff(handoff)
        | FadeStyle::BeatMix(handoff)
        | FadeStyle::RingOut { handoff, .. } => handoff,
        FadeStyle::Blend => 0.0,
    };
    let crossover = plan.bass_swap.map_or(0.5, |swap| swap.end).max(handoff);
    Some(BandStage { crossover })
}

/// The incoming deck's gain, matched to what the listener hears now rather than to an absolute
/// target. Taking B's absolute normalisation (`gain_adjustment`, 0.1..3.0) as-is was a step of
/// several dB whenever A played at another gain — after any ordinary load A is at 1.0 — and the
/// step arrived as the fade finished. Mirrors `_computeIncomingGainAdjustment` in
/// TransitionStateMachine.ts: 3/4 relative to A's effective loudness, 1/4 toward B's own target,
/// so a playlist still drifts to the configured loudness. Web's energy-contrast term is not
/// carried. One deliberate difference: with B unanalysed the Web returns 1.0, here the outgoing
/// gain carries over, because 1.0 is itself a step when A is not at unity.
pub(super) fn incoming_gain(
    config: &AutoMixConfig,
    outgoing_gain: f64,
    current: Option<&automix_analysis::TrackAnalysis>,
    next: Option<&automix_analysis::TrackAnalysis>,
) -> f64 {
    if !config.volume_norm {
        return 1.0;
    }
    let Some(next) = next else {
        return outgoing_gain;
    };
    let absolute = f64::from(next.volume.gain_adjustment).clamp(0.55, 1.8);
    let gain = match current {
        Some(current) => {
            let heard =
                f64::from(current.volume.estimated_lufs) + 20.0 * outgoing_gain.max(0.001).log10();
            let relative = 10f64
                .powf((heard - f64::from(next.volume.estimated_lufs)) / 20.0)
                .clamp(0.5, 2.0);
            relative.powf(0.75) * absolute.powf(0.25)
        }
        None => absolute,
    };
    gain.clamp(0.55, 1.8)
}

/// A still singing inside an unsynced fade dominates until its last line ends,
/// with B waiting underneath at 0.6 of it (HyPlayer-Nextgen's crossfade).
fn hold_for_last_vocal(plan: &mut CrossfadePlan, current: &automix_analysis::TrackAnalysis) {
    let Some(end) = vocal_runs(current, 0.8).last().map(|run| run.1) else {
        return;
    };
    let handoff = ((end - plan.start_time) / plan.duration).clamp(0.0, 0.8);
    // A handoff already placed later (A's drop under B's arrival) is kept.
    match &mut plan.style {
        FadeStyle::RingOut { handoff: held, .. } => {
            *held = held.max(handoff as f32);
        }
        style => {
            let held = match style {
                FadeStyle::Handoff(held) => f64::from(*held),
                _ => 0.0,
            };
            if handoff >= 0.1 && handoff > held {
                *style = FadeStyle::Handoff(handoff as f32);
            }
        }
    }
}

/// Sung stretches at least `min_seconds` long, as (start, end) source seconds.
pub(super) fn vocal_runs(
    track: &automix_analysis::TrackAnalysis,
    min_seconds: f64,
) -> Vec<(f64, f64)> {
    let Some(vocal) = track.vocal_activity.as_ref() else {
        return Vec::new();
    };
    let width = f64::from(vocal.window_duration);
    let mut runs = Vec::new();
    let mut open: Option<usize> = None;
    for (i, risk) in vocal.risk.iter().chain([&0.0]).enumerate() {
        match (*risk >= VOCAL_WINDOW_ACTIVE, open) {
            (true, None) => open = Some(i),
            (false, Some(first)) => {
                if (i - first) as f64 * width >= min_seconds {
                    runs.push((first as f64 * width, i as f64 * width));
                }
                open = None;
            }
            _ => {}
        }
    }
    runs
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
    let (outgoing, incoming) = overlap_windows(plan);
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
    plan.morph = choose_morph(plan, current, next);
}

/// Source-second windows the overlap covers on A and on B.
fn overlap_windows(plan: &CrossfadePlan) -> ((f64, f64), (f64, f64)) {
    (
        (
            plan.start_time,
            plan.start_time + plan.duration * plan.outgoing_rate,
        ),
        (plan.incoming_start, plan.incoming_start + plan.duration),
    )
}

/// Largest tonal lean a morph applies to a band, as the Web clamps it.
const MORPH_MAX_DB: f32 = 6.0;
/// Below this in every band the two sides already sound alike.
const MORPH_MIN_DB: f32 = 1.5;

/// The tonal half of the handover: how far each band of B's opening sits from A's ending, in dB,
/// for `BandMorph` to lean the decks across. From the same overlap windows as the bass swap; the
/// Web derives it from the last and first 2 s. The bands' common offset is removed first, because
/// that part is loudness, which `incoming_gain` already matches — morphing it too would move the
/// level twice. `10 * log10` over RMS ratios gives half the real dB difference; kept as the Web has
/// it, so the two lean alike. The low band is left out while a bass swap runs.
pub(super) fn choose_morph(
    plan: &CrossfadePlan,
    current: &automix_analysis::TrackAnalysis,
    next: &automix_analysis::TrackAnalysis,
) -> Option<BandMorph> {
    let ((out_from, out_to), (in_from, in_to)) = overlap_windows(plan);
    let outro = &current.outro.as_ref()?.multiband_energy;
    let intro = next.intro.as_ref()?.multiband_energy.as_ref()?;
    // Outro windows end at the content end, 0.25 s each.
    let origin = f64::from(current.duration - current.energy.trailing_silence)
        - outro.low.len() as f64 * 0.25;
    let mut db = [0f32; 3];
    for (band, (a, b)) in [
        (&outro.low, &intro.low),
        (&outro.mid, &intro.mid),
        (&outro.high, &intro.high),
    ]
    .into_iter()
    .enumerate()
    {
        let a = mean_window(a, (out_from - origin) / 0.25, (out_to - origin) / 0.25)?;
        let b = mean_window(b, in_from / 0.25, in_to / 0.25)?;
        db[band] = match (a < 1e-4, b < 1e-4) {
            (true, true) => 0.0,
            (true, false) => MORPH_MAX_DB,
            (false, true) => -MORPH_MAX_DB,
            (false, false) => 10.0 * (b / a).log10(),
        };
    }
    // The offset comes from the bands the morph will move: with a bass swap running, B's bass is
    // the swap's, and counting it would push its difference onto the mids and highs.
    let moved = if plan.bass_swap.is_some() { 1 } else { 0 };
    let offset = db[moved..].iter().sum::<f32>() / (3 - moved) as f32;
    for d in &mut db {
        *d = (*d - offset).clamp(-MORPH_MAX_DB, MORPH_MAX_DB);
    }
    if moved == 1 {
        db[0] = 0.0;
    }
    (db.iter().any(|d| d.abs() >= MORPH_MIN_DB)).then_some(BandMorph { db })
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
    let active = windows
        .iter()
        .filter(|risk| **risk >= VOCAL_WINDOW_ACTIVE)
        .count();
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
    let origin =
        f64::from(current.duration - current.energy.trailing_silence) - low.len() as f64 * 0.25;
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
            Some(BassSwap {
                start: start as f32,
                end: end as f32,
            })
        });
    Some(beat.unwrap_or(BassSwap::MIDDLE))
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
    // Never longer than the setting, or than a measured decay already is.
    plan.duration = (plan.duration * f64::from(profile.duration_scale)).clamp(
        MIN_CROSSFADE_DURATION,
        configured_duration(config, effective_end).max(plan.duration),
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

/// Start and length of a fade over A's own decay. Reads A's outro short-term loudness (250 ms
/// windows ending at the content end) the way Liquidsoap's autocue does: the decay starts after
/// the last window within `TAIL_DROP_LU` of the outro's loud level (its 80th percentile). B
/// arriving there is heard as A lets go, rather than at a fixed time that is either still inside
/// A's last full bars or already in its silence. A hard or silent ending keeps its short fade.
/// When A is loud to the very end, B opening loud gets a short overlap; B opening quietly is left
/// to the ordinary placement, where A covers its intro.
fn loudness_placement(
    config: &AutoMixConfig,
    current: &automix_analysis::TrackAnalysis,
    next: Option<&automix_analysis::TrackAnalysis>,
    effective_end: f64,
) -> Option<(f64, f64)> {
    let outro = current.outro.as_ref()?;
    if outro.outro_confidence >= 0.6 && matches!(outro.outro_type.as_str(), "hard" | "silence") {
        return None;
    }
    let loudness = &outro.short_term_loudness;
    // Under 4 s of windows says nothing about a decay.
    if loudness.len() < 16 {
        return None;
    }
    let mut sorted = loudness.clone();
    let rank = (sorted.len() - 1) * 8 / 10;
    let reference = *sorted.select_nth_unstable_by(rank, f32::total_cmp).1;
    let content_end = f64::from(current.duration - current.energy.trailing_silence);
    let tail_start = |drop: f32| {
        loudness
            .iter()
            .rposition(|lufs| *lufs >= reference - drop)
            .map(|i| content_end - (loudness.len() - 1 - i) as f64 * 0.25)
    };
    let mut start = tail_start(TAIL_DROP_LU)?;
    if content_end - start > LONG_TAIL_SECONDS {
        start = tail_start(LONG_TAIL_DROP_LU)?;
    }
    let tail = content_end - start;
    let duration = if tail < MIN_CROSSFADE_DURATION {
        let intro = next?.intro.as_ref()?;
        if intro.quiet_intro_duration >= 1.0 || intro.intro_energy_ratio < 0.8 {
            return None;
        }
        configured_duration(config, effective_end).min(LOUD_PAIR_SECONDS)
    } else {
        tail.clamp(MIN_CROSSFADE_DURATION, longest_fade(config, effective_end))
    };
    Some((content_end - duration, duration))
}

/// Longest fade a measured placement may take: max(1.5 x the setting, 12 s), at most a quarter of A.
fn longest_fade(config: &AutoMixConfig, effective_end: f64) -> f64 {
    (config.crossfade_duration * 1.5)
        .max(12.0)
        .min((effective_end / 4.0).max(MIN_CROSSFADE_DURATION))
}

/// Band power in dB per 250 ms window, and the first and last window within `TAIL_DROP_LU` (here
/// dB, not LU) of their 80th percentile: where a song's full level starts and stops. Unsmoothed,
/// unlike the outro's short-term loudness, which lags a sudden drop.
fn loud_windows(bands: &automix_analysis::MultibandEnergy) -> Option<(usize, usize, usize)> {
    let level: Vec<f32> = bands
        .low
        .iter()
        .zip(&bands.mid)
        .zip(&bands.high)
        .map(|((low, mid), high)| 10.0 * (low * low + mid * mid + high * high).max(1e-12).log10())
        .collect();
    // Under 4 s of windows says nothing.
    if level.len() < 16 {
        return None;
    }
    let mut sorted = level.clone();
    let rank = (sorted.len() - 1) * 8 / 10;
    let floor = *sorted.select_nth_unstable_by(rank, f32::total_cmp).1 - TAIL_DROP_LU;
    let first = level.iter().position(|db| *db >= floor)?;
    let last = level.iter().rposition(|db| *db >= floor)?;
    Some((first, last, level.len()))
}

/// Where A's full level stops, in song seconds: the end of its last loud outro window (they end at
/// the content end). Also where B's first beat should land, which continues A's pulse when A's
/// grid ends there: one step after its last pulse, B's own beat when the two are within
/// `MEET_STEP_TOLERANCE` (so repeats of A's last beat at that spacing land on B's beats, and the
/// step is returned), else A's. Without that pulse it is the drop itself. When B's beat is more
/// than `RIDE_MIN_CHANGE` off A's, A rides into it over its last `RIDE_BEATS` (pitch kept): its
/// last beats then fall at B's spacing, and B's first beat lands on A's own next beat.
fn departure(
    current: &automix_analysis::TrackAnalysis,
    next: &automix_analysis::TrackAnalysis,
) -> Option<(f64, f64, Option<f64>, Option<Ride>)> {
    let (_, last, len) = loud_windows(&current.outro.as_ref()?.multiband_energy)?;
    let content_end = f64::from(current.duration - current.energy.trailing_silence);
    let drop = content_end - (len - 1 - last) as f64 * 0.25;
    let b_beat = next
        .bpm
        .as_ref()
        .and_then(|bpm| bpm.head.as_ref())
        .map(|grid| f64::from(grid.period));
    let pulse = current
        .bpm
        .as_ref()
        .and_then(|bpm| bpm.tail.as_ref())
        .and_then(|grid| {
            let (last, period) = (f64::from(*grid.beats.last()?), f64::from(grid.period));
            if last > drop || drop - last > 1.5 * period {
                return None;
            }
            let step = b_beat.filter(|b| (b / period - 1.0).abs() <= MEET_STEP_TOLERANCE);
            let ride = step.map(|step| Ride {
                from: last - RIDE_BEATS * period,
                to: last,
                rate: period / step,
            });
            let ride = ride.filter(|ride| {
                (ride.rate - 1.0).abs() > RIDE_MIN_CHANGE
                    && RATE_RANGE.contains(&ride.rate)
                    && ride.from >= 0.0
            });
            match ride {
                Some(ride) => Some((last + period, step, Some(ride))),
                None => Some((last + step.unwrap_or(period), step, None)),
            }
        });
    let (meet, step, ride) = pulse.unwrap_or((drop, None, None));
    Some((drop, meet.min(content_end), step, ride))
}

/// A easing linearly, by source position, from its own speed at `from` to `rate` at `to`, and
/// held there after. `rate` is source seconds per output second.
#[derive(Debug, Clone, Copy)]
struct Ride {
    from: f64,
    to: f64,
    rate: f64,
}

impl Ride {
    /// Output seconds A takes to play its source seconds `start..end`.
    fn played(ride: Option<&Self>, start: f64, end: f64) -> f64 {
        let Some(ride) = ride else {
            return end - start;
        };
        let before = (end.min(ride.from) - start).max(0.0);
        let after = (end - start.max(ride.to)).max(0.0) / ride.rate;
        let (x, y) = (start.max(ride.from), end.min(ride.to));
        let slope = (ride.rate - 1.0) / (ride.to - ride.from);
        let speed = |s: f64| 1.0 + slope * (s - ride.from);
        let during = if y > x {
            (speed(y) / speed(x)).ln() / slope
        } else {
            0.0
        };
        before + during + after
    }

    /// The source second from which A takes `seconds` of output to reach `end`.
    fn start_before(ride: Option<&Self>, end: f64, seconds: f64) -> f64 {
        let (min, max) = (*RATE_RANGE.start(), *RATE_RANGE.end());
        let (mut early, mut late) = (end - seconds * max, end - seconds * min);
        for _ in 0..48 {
            let mid = 0.5 * (early + late);
            if Self::played(ride, mid, end) > seconds {
                early = mid;
            } else {
                late = mid;
            }
        }
        0.5 * (early + late)
    }
}

/// How far into B its full level starts: the start of its first loud intro window, or its first
/// beat when that falls just after (the window can only say which quarter second).
fn arrival(next: &automix_analysis::TrackAnalysis) -> Option<f64> {
    let (first, _, _) = loud_windows(next.intro.as_ref()?.multiband_energy.as_ref()?)?;
    let arrive = first as f64 * 0.25;
    let first_beat = next
        .bpm
        .as_ref()
        .and_then(|bpm| bpm.head.as_ref())
        .and_then(|grid| grid.beats.first())
        .map(|beat| f64::from(*beat))
        .filter(|beat| (arrive - 0.25..=arrive + 0.5).contains(beat));
    Some(first_beat.unwrap_or(arrive))
}

/// B's energy arrives as A's leaves. B opening with a quiet lead-in starts that long before A's
/// full level stops, so its first beat lands where A lets go (`departure`); started at its top
/// instead, B was still in its lead-in as A decayed, a hole of up to 18 dB between two loud songs.
/// A holds until its own drop, then releases over the rest of its tail, which rings out under B
/// (`FadeStyle::RingOut`, B at full from its first beat): the bass changes hands from A's drop to
/// B's first beat (the returned window). Ending the fade just after B's first beat instead cut A's tail off and handed over in
/// under half a second, which read as a hard cut between two moods. Only for a fade over A's
/// ending (one a section cue moved well before it keeps its place) whose tail after the drop is
/// at most `MEET_MAX_TAIL`; without A's ending or B's opening measured, or with a lead-in too long
/// to lay under A, nothing changes.
fn meet_on_arrival(
    plan: &mut CrossfadePlan,
    config: &AutoMixConfig,
    current: &automix_analysis::TrackAnalysis,
    next: &automix_analysis::TrackAnalysis,
    effective_end: f64,
) -> Option<Meet> {
    let (drop, meet, step, ride) = departure(current, next)?;
    let arrive = arrival(next)?;
    // B plays at its own speed from the fade's start, so its first beat is `arrive` of output in.
    let start = Ride::start_before(ride.as_ref(), meet, arrive);
    let duration = Ride::played(ride.as_ref(), start, effective_end);
    if arrive < MIN_LEAD_IN
        || effective_end - drop > MEET_MAX_TAIL
        || plan.start_time + plan.duration <= drop
        || start < 0.0
        || duration > longest_fade(config, effective_end)
    {
        return None;
    }
    plan.start_time = start;
    plan.duration = duration;
    plan.incoming_start = 0.0;
    plan.section_cue = None;
    if let Some(ride) = ride {
        plan.outgoing_rate = ride.rate;
        plan.tempo_ramp = Some((ride.from, ride.to));
    }
    let handoff = (Ride::played(ride.as_ref(), start, drop.min(meet)) / duration).max(0.0) as f32;
    let landed = (arrive / duration) as f32;
    plan.style = FadeStyle::RingOut { handoff, landed };
    Some(Meet {
        swap: arrival_swap(handoff, landed, b_beat(next, step) / duration),
        beat: arrive,
        step,
    })
}

/// B's beat in seconds: the spacing its first beat continues A's pulse at, else its head grid's.
fn b_beat(next: &automix_analysis::TrackAnalysis, step: Option<f64>) -> f64 {
    step.or_else(|| {
        let grid = next.bpm.as_ref()?.head.as_ref()?;
        Some(f64::from(grid.period))
    })
    .filter(|beat| *beat > 0.0)
    .unwrap_or(FALLBACK_BEAT)
}

/// When the bass changes hands in a ring-out: from A's drop to B's first beat, and never in less
/// than one of B's beats. A's drop routinely falls on B's first beat, and the window then
/// collapsed to the beat-mix's 30 ms step — right for a beat-mix, whose kicks coincide, but here B
/// is also reaching full level on that beat, so its whole low end arrived in one step (+14 dB of
/// B's lows in 100 ms while A's fell 17 dB, measured on a real pair). Too short, the window is
/// centred on B's first beat instead, so that kick arrives with half its low band and A's low
/// end leaves across the same beat. `beat` is a share of the fade.
pub(super) fn arrival_swap(handoff: f32, landed: f32, beat: f64) -> BassSwap {
    let beat = beat as f32;
    if landed - handoff >= beat {
        return BassSwap {
            start: handoff,
            end: landed,
        };
    }
    BassSwap {
        start: (landed - 0.5 * beat).max(0.0),
        end: (landed + 0.5 * beat).min(1.0),
    }
}

/// What `meet_on_arrival` decided beyond the plan itself.
struct Meet {
    /// When the bass changes hands: from A's drop to B's first beat.
    swap: BassSwap,
    /// Seconds into the fade of B's first beat.
    beat: f64,
    /// B's beat, when B's first beat continues A's pulse at it.
    step: Option<f64>,
}

/// A DJ's echo-out on A's last beat, to carry A's mood into B instead of dropping it: from B's
/// first beat, A's last beat repeats at B's own spacing — on B's beats — and dies away over B's
/// opening, about four repeats. Opens 30 ms before that beat, so nothing earlier in the line is
/// heard. Only when B's first beat continues A's pulse at B's beat (`departure`), and like every
/// echo-out not without transition effects or on a calm arc. The Web's echo throw is a lighter
/// effect over the whole fade (`choose_echo`).
fn echo_into_arrival(config: &AutoMixConfig, plan: &CrossfadePlan, meet: &Meet) -> Option<EchoOut> {
    let step = meet
        .step
        .filter(|step| *step <= f64::from(ECHO_MAX_DELAY))?;
    if !config.transition_effects
        || matches!(plan.arc, EmotionalArc::Afterglow | EmotionalArc::Drift)
    {
        return None;
    }
    Some(EchoOut {
        delay: step as f32,
        feedback: 0.6,
        cutoff_hz: 2_500.0,
        wet: 0.5,
        start: (meet.beat - ECHO_OUT_ATTACK).max(0.0) as f32,
        attack: ECHO_OUT_ATTACK as f32,
        duration: 12.0,
    })
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
        assert_eq!(
            pair_shape_scale(Some(0.8), Some(0.7), None, None),
            (1.2, 1.2)
        );
        assert_eq!(
            pair_shape_scale(Some(0.1), Some(0.7), None, None),
            (1.0, 1.2)
        );
        assert_eq!(
            pair_shape_scale(Some(0.7), Some(0.1), None, None),
            (0.85, 1.0)
        );
        // In between neither side is clearly singing or clear: leave it alone.
        assert_eq!(
            pair_shape_scale(Some(0.4), Some(0.4), None, None),
            (1.0, 1.0)
        );
        assert_eq!(pair_shape_scale(None, None, None, None), (1.0, 1.0));
    }

    #[test]
    fn a_quiet_opening_comes_in_early_and_a_loud_one_is_held_back() {
        assert_eq!(
            pair_shape_scale(None, None, Some(0.8), Some(0.3)),
            (0.85, 1.0)
        );
        assert_eq!(
            pair_shape_scale(None, None, Some(0.3), Some(0.8)),
            (1.15, 1.0)
        );
        assert_eq!(
            pair_shape_scale(None, None, Some(0.6), Some(0.7)),
            (1.0, 1.0)
        );
    }
}
