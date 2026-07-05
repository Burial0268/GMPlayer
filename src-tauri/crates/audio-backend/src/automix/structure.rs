//! Song-structure analysis for AutoMix: intro/outro multiband detection and
//! section (verse/chorus/bridge/...) segmentation.

use super::analysis::{design_biquad_bandpass, design_k_weight_shelf, iir_process};
use super::{
    BPMResult, EnergyAnalysis, IntroAnalysis, MixPointAnalysis, MixPointCandidate, MultibandEnergy,
    OutroAnalysis, PhraseAnalysis, SectionAnalysis, SongSection, SongSectionKind,
    VocalActivityAnalysis, INTRO_SCAN_SECONDS, MAX_SONG_SECTIONS, OUTRO_ANALYSIS_SECONDS,
    OUTRO_WINDOW_MS, SECTION_FALLBACK_SECONDS, SECTION_MAX_SECONDS, SECTION_MIN_SECONDS,
    SECTION_PHRASE_BEATS,
};

// ─── Multiband Outro Analysis ──────────────────────────────────────

pub(super) fn analyze_outro_multiband(
    data: &[f32],
    sample_rate: u32,
    duration: f32,
    trailing_silence: f32,
) -> Option<OutroAnalysis> {
    let content_duration = duration - trailing_silence;
    if content_duration < 10.0 {
        return None;
    }

    let analysis_duration = OUTRO_ANALYSIS_SECONDS.min(content_duration);
    let window_samples = (sample_rate as f32 * OUTRO_WINDOW_MS / 1000.0) as usize;
    let total_windows = ((analysis_duration * 1000.0) / OUTRO_WINDOW_MS) as usize;

    if total_windows < 4 || window_samples < 1 {
        return None;
    }

    let content_end_sample = ((duration - trailing_silence) * sample_rate as f32) as usize;
    let analysis_start_sample =
        content_end_sample.saturating_sub((analysis_duration * sample_rate as f32) as usize);
    let analysis_start_sample = analysis_start_sample.max(0);

    let sample_rate_f = sample_rate as f32;

    // Design filters
    let low_c = design_biquad_bandpass(20.0, 300.0, sample_rate_f);
    let mid_c = design_biquad_bandpass(300.0, 4000.0, sample_rate_f);
    let high_c = design_biquad_bandpass(
        4000.0,
        (sample_rate_f / 2.0 - 100.0).min(16000.0),
        sample_rate_f,
    );
    let k_c = design_k_weight_shelf(sample_rate_f);

    let (mut lz1, mut lz2, mut mz1, mut mz2, mut hz1, mut hz2, mut kz1, mut kz2) =
        (0f32, 0f32, 0f32, 0f32, 0f32, 0f32, 0f32, 0f32);

    // Warm-up (200 samples)
    let warmup_start = analysis_start_sample.saturating_sub(200).max(0);
    for &s in &data[warmup_start..analysis_start_sample] {
        iir_process(&low_c, &mut lz1, &mut lz2, s);
        iir_process(&mid_c, &mut mz1, &mut mz2, s);
        iir_process(&high_c, &mut hz1, &mut hz2, s);
        iir_process(&k_c, &mut kz1, &mut kz2, s);
    }

    let mut low = vec![0f32; total_windows];
    let mut mid = vec![0f32; total_windows];
    let mut high = vec![0f32; total_windows];
    let mut loudness = vec![0f32; total_windows];
    let mut flux = vec![0f32; total_windows];

    let mut accum_low = 0f64;
    let mut accum_mid = 0f64;
    let mut accum_high = 0f64;
    let mut accum_k = 0f64;
    let mut sample_in_window = 0usize;
    let mut window_idx = 0usize;
    let mut prev_low = 0f32;
    let mut prev_mid = 0f32;
    let mut prev_high = 0f32;

    let end_sample = (analysis_start_sample + total_windows * window_samples).min(data.len());

    for &s in &data[analysis_start_sample..end_sample] {
        if window_idx >= total_windows {
            break;
        }

        let f_low = iir_process(&low_c, &mut lz1, &mut lz2, s);
        let f_mid = iir_process(&mid_c, &mut mz1, &mut mz2, s);
        let f_high = iir_process(&high_c, &mut hz1, &mut hz2, s);
        let f_k = iir_process(&k_c, &mut kz1, &mut kz2, s);

        accum_low += (f_low as f64) * (f_low as f64);
        accum_mid += (f_mid as f64) * (f_mid as f64);
        accum_high += (f_high as f64) * (f_high as f64);
        accum_k += (f_k as f64) * (f_k as f64);
        sample_in_window += 1;

        if sample_in_window >= window_samples {
            let rms_low = (accum_low / sample_in_window as f64).sqrt() as f32;
            let rms_mid = (accum_mid / sample_in_window as f64).sqrt() as f32;
            let rms_high = (accum_high / sample_in_window as f64).sqrt() as f32;
            let mean_sq_k = (accum_k / sample_in_window as f64) as f32;

            low[window_idx] = rms_low;
            mid[window_idx] = rms_mid;
            high[window_idx] = rms_high;
            loudness[window_idx] = if mean_sq_k > 0.0 {
                -0.691 + 10.0 * mean_sq_k.log10()
            } else {
                -70.0
            };

            flux[window_idx] = if window_idx == 0 {
                0.0
            } else {
                let d_low = if rms_low > prev_low {
                    rms_low - prev_low
                } else {
                    0.0
                };
                let d_mid = if rms_mid > prev_mid {
                    rms_mid - prev_mid
                } else {
                    0.0
                };
                let d_high = if rms_high > prev_high {
                    rms_high - prev_high
                } else {
                    0.0
                };
                d_low + d_mid + d_high
            };

            prev_low = rms_low;
            prev_mid = rms_mid;
            prev_high = rms_high;

            accum_low = 0.0;
            accum_mid = 0.0;
            accum_high = 0.0;
            accum_k = 0.0;
            sample_in_window = 0;
            window_idx += 1;
        }
    }

    let actual_windows = window_idx;
    if actual_windows < 4 {
        return None;
    }

    low.truncate(actual_windows);
    mid.truncate(actual_windows);
    high.truncate(actual_windows);
    loudness.truncate(actual_windows);
    flux.truncate(actual_windows);

    // Classify outro (simplified fast path — full classifier stays in JS for now)
    let classification = classify_outro_simple(
        &low,
        &mid,
        &high,
        &flux,
        &loudness,
        duration,
        trailing_silence,
        analysis_duration,
        actual_windows,
    );

    Some(OutroAnalysis {
        outro_type: classification.outro_type,
        outro_confidence: classification.outro_confidence,
        musical_end_offset: classification.musical_end_offset,
        suggested_crossfade_start: classification.suggested_crossfade_start,
        multiband_energy: MultibandEnergy { low, mid, high },
        spectral_flux: flux,
        short_term_loudness: loudness,
        deceleration_start: classification.deceleration_start,
        sustain_onset: classification.sustain_onset,
        outro_section_start: classification.outro_section_start,
        loop_period: classification.loop_period,
    })
}

// ─── Intro Multiband Analysis ──────────────────────────────────────

fn analyze_intro_multiband(
    data: &[f32],
    sample_rate: u32,
    duration: f32,
) -> Option<MultibandEnergy> {
    if duration < 5.0 {
        return None;
    }

    let analysis_duration = 20f32.min(duration);
    let window_samples = (sample_rate as f32 * OUTRO_WINDOW_MS / 1000.0) as usize;
    let total_windows = ((analysis_duration * 1000.0) / OUTRO_WINDOW_MS) as usize;

    if total_windows < 4 || window_samples < 1 {
        return None;
    }

    let sample_rate_f = sample_rate as f32;
    let low_c = design_biquad_bandpass(20.0, 300.0, sample_rate_f);
    let mid_c = design_biquad_bandpass(300.0, 4000.0, sample_rate_f);
    let high_c = design_biquad_bandpass(
        4000.0,
        (sample_rate_f / 2.0 - 100.0).min(16000.0),
        sample_rate_f,
    );

    let (mut lz1, mut lz2, mut mz1, mut mz2, mut hz1, mut hz2) =
        (0f32, 0f32, 0f32, 0f32, 0f32, 0f32);

    let mut low = vec![0f32; total_windows];
    let mut mid = vec![0f32; total_windows];
    let mut high = vec![0f32; total_windows];

    let mut accum_low = 0f64;
    let mut accum_mid = 0f64;
    let mut accum_high = 0f64;
    let mut sample_in_window = 0usize;
    let mut window_idx = 0usize;

    let end_sample = (total_windows * window_samples).min(data.len());

    for &s in &data[..end_sample] {
        if window_idx >= total_windows {
            break;
        }

        let f_low = iir_process(&low_c, &mut lz1, &mut lz2, s);
        let f_mid = iir_process(&mid_c, &mut mz1, &mut mz2, s);
        let f_high = iir_process(&high_c, &mut hz1, &mut hz2, s);

        accum_low += (f_low as f64) * (f_low as f64);
        accum_mid += (f_mid as f64) * (f_mid as f64);
        accum_high += (f_high as f64) * (f_high as f64);
        sample_in_window += 1;

        if sample_in_window >= window_samples {
            low[window_idx] = (accum_low / sample_in_window as f64).sqrt() as f32;
            mid[window_idx] = (accum_mid / sample_in_window as f64).sqrt() as f32;
            high[window_idx] = (accum_high / sample_in_window as f64).sqrt() as f32;

            accum_low = 0.0;
            accum_mid = 0.0;
            accum_high = 0.0;
            sample_in_window = 0;
            window_idx += 1;
        }
    }

    if window_idx < 4 {
        return None;
    }
    low.truncate(window_idx);
    mid.truncate(window_idx);
    high.truncate(window_idx);

    Some(MultibandEnergy { low, mid, high })
}

// ─── Intro Analysis ────────────────────────────────────────────────

pub(super) fn analyze_intro(
    energy_per_second: &[f32],
    average_energy: f32,
    data: &[f32],
    sample_rate: u32,
    duration: f32,
) -> Option<IntroAnalysis> {
    let scan_len = INTRO_SCAN_SECONDS.min(energy_per_second.len());
    if scan_len < 4 {
        return None;
    }

    let quiet_threshold = average_energy * 0.5;
    let mut quiet_intro_duration = scan_len as f32;

    for i in 0..scan_len.saturating_sub(1) {
        if energy_per_second[i] >= quiet_threshold && energy_per_second[i + 1] >= quiet_threshold {
            quiet_intro_duration = i as f32;
            break;
        }
    }

    let build_threshold = average_energy * 0.8;
    let mut energy_build_duration = scan_len as f32;

    for i in 0..scan_len.saturating_sub(1) {
        if energy_per_second[i] >= build_threshold && energy_per_second[i + 1] >= build_threshold {
            energy_build_duration = i as f32;
            break;
        }
    }

    let sum: f32 = energy_per_second[..scan_len].iter().sum();
    let intro_energy_ratio = if average_energy > 0.001 {
        sum / scan_len as f32 / average_energy
    } else {
        1.0
    };

    let multiband_energy = analyze_intro_multiband(data, sample_rate, duration);

    Some(IntroAnalysis {
        quiet_intro_duration,
        energy_build_duration,
        intro_energy_ratio,
        multiband_energy,
    })
}

// ─── Song Section Analysis ────────────────────────────────────────

#[derive(Debug, Clone)]
struct SectionCandidate {
    start: f32,
    end: f32,
    avg_energy: f32,
    peak_energy: f32,
    flux: f32,
    index: u32,
}

#[derive(Debug, Clone, Copy)]
struct SectionEnergyProfile {
    median: f32,
    low_threshold: f32,
    high_threshold: f32,
    spread: f32,
    content_count: usize,
}

#[derive(Debug, Clone)]
struct LabeledSectionCandidate {
    candidate: SectionCandidate,
    section_type: SongSectionKind,
    confidence: f32,
    vocal_risk: f32,
    mix_suitability: f32,
}

pub(super) fn analyze_song_sections(
    energy: &EnergyAnalysis,
    bpm: Option<&BPMResult>,
    intro: Option<&IntroAnalysis>,
    outro: Option<&OutroAnalysis>,
    vocal_activity: Option<&VocalActivityAnalysis>,
    duration: f32,
) -> Option<SectionAnalysis> {
    if duration < SECTION_MIN_SECONDS * 2.0 || energy.energy_per_second.len() < 4 {
        return None;
    }

    let content_end = (duration - energy.trailing_silence).max(0.0).min(duration);
    if content_end < SECTION_MIN_SECONDS {
        return None;
    }

    let intro_boundary = section_intro_boundary(energy, intro, content_end);
    let outro_boundary = section_outro_boundary(energy, outro, duration, content_end);
    let (step_seconds, method, bpm_confidence) = section_grid_step(bpm, content_end);
    let candidates = build_section_candidates(
        &energy.energy_per_second,
        duration,
        content_end,
        intro_boundary,
        outro_boundary,
        step_seconds,
    );
    if candidates.len() < 2 {
        return None;
    }

    let profile = build_section_energy_profile(&candidates, intro_boundary, outro_boundary)
        .unwrap_or(SectionEnergyProfile {
            median: energy.average_energy,
            low_threshold: energy.average_energy * 0.65,
            high_threshold: (energy.average_energy + 0.15).min(0.85),
            spread: 0.0,
            content_count: candidates.len(),
        });

    let mut labeled = candidates
        .into_iter()
        .map(|candidate| {
            let (section_type, confidence) = classify_section_candidate(
                &candidate,
                profile,
                intro_boundary,
                outro_boundary,
                content_end,
                duration,
                bpm_confidence,
            );
            let vocal_risk =
                section_vocal_risk(vocal_activity, candidate.start, candidate.end).unwrap_or(0.5);
            let mix_suitability =
                section_mix_suitability(section_type, candidate.avg_energy, confidence, vocal_risk);
            LabeledSectionCandidate {
                candidate,
                section_type,
                confidence,
                vocal_risk,
                mix_suitability,
            }
        })
        .collect::<Vec<_>>();

    promote_best_chorus_if_needed(&mut labeled, profile);

    let mut sections = merge_labeled_sections(labeled);
    if sections.is_empty() {
        return None;
    }

    for (index, section) in sections.iter_mut().enumerate() {
        section.index = index as u32;
    }

    let avg_confidence = sections
        .iter()
        .map(|section| section.confidence)
        .sum::<f32>()
        / sections.len() as f32;
    let confidence = (avg_confidence * 0.7
        + bpm_confidence * 0.15
        + (profile.spread / 0.35).clamp(0.0, 1.0) * 0.15)
        .clamp(0.0, 1.0);

    Some(SectionAnalysis {
        sections,
        confidence,
        method,
    })
}

fn section_intro_boundary(
    energy: &EnergyAnalysis,
    intro: Option<&IntroAnalysis>,
    content_end: f32,
) -> f32 {
    let intro_energy = intro
        .map(|intro| intro.energy_build_duration.max(intro.quiet_intro_duration))
        .unwrap_or(energy.intro_end_offset);
    intro_energy
        .max(energy.intro_end_offset)
        .clamp(0.0, content_end.min(SECTION_MAX_SECONDS))
}

fn section_outro_boundary(
    energy: &EnergyAnalysis,
    outro: Option<&OutroAnalysis>,
    duration: f32,
    content_end: f32,
) -> f32 {
    let outro_from_energy = duration - energy.outro_start_offset;
    let outro_from_classifier = outro
        .and_then(|outro| outro.outro_section_start)
        .unwrap_or(outro_from_energy);
    outro_from_classifier
        .min(outro_from_energy.max(content_end - SECTION_FALLBACK_SECONDS))
        .clamp(0.0, content_end)
}

fn section_grid_step(bpm: Option<&BPMResult>, content_end: f32) -> (f32, String, f32) {
    let max_sections = (MAX_SONG_SECTIONS.saturating_sub(2)).max(1) as f32;
    let min_step_for_limit = (content_end / max_sections).max(SECTION_MIN_SECONDS);

    if let Some(bpm) = bpm {
        if bpm.confidence >= 0.3 && bpm.bpm.is_finite() && bpm.bpm > 0.0 {
            let phrase_seconds = (60.0 / bpm.bpm) * SECTION_PHRASE_BEATS;
            if (SECTION_MIN_SECONDS..=SECTION_MAX_SECONDS).contains(&phrase_seconds) {
                let step = phrase_seconds.max(min_step_for_limit);
                return (
                    step,
                    "bpmPhraseEnergy".to_string(),
                    bpm.confidence.clamp(0.0, 1.0),
                );
            }
        }
    }

    (
        SECTION_FALLBACK_SECONDS.max(min_step_for_limit),
        "energyWindow".to_string(),
        0.0,
    )
}

fn build_section_candidates(
    energy_per_second: &[f32],
    duration: f32,
    content_end: f32,
    intro_boundary: f32,
    outro_boundary: f32,
    step_seconds: f32,
) -> Vec<SectionCandidate> {
    let mut boundaries = Vec::new();
    push_section_boundary(&mut boundaries, 0.0, duration);

    if intro_boundary >= SECTION_MIN_SECONDS && intro_boundary < content_end - SECTION_MIN_SECONDS {
        push_section_boundary(&mut boundaries, intro_boundary, duration);
    }

    let mut cursor = boundaries.last().copied().unwrap_or(0.0);
    let body_end = if outro_boundary >= cursor + SECTION_MIN_SECONDS {
        outro_boundary
    } else {
        content_end
    };

    while cursor + step_seconds < body_end - SECTION_MIN_SECONDS {
        cursor += step_seconds;
        push_section_boundary(&mut boundaries, cursor, duration);
    }

    if outro_boundary >= SECTION_MIN_SECONDS && outro_boundary < content_end - 1.0 {
        push_section_boundary(&mut boundaries, outro_boundary, duration);
    }

    push_section_boundary(&mut boundaries, content_end, duration);

    if duration - content_end >= 1.0 {
        push_section_boundary(&mut boundaries, duration, duration);
    }

    boundaries.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    boundaries.dedup_by(|a, b| (*a - *b).abs() < 0.25);

    let mut candidates = Vec::new();
    for pair in boundaries.windows(2) {
        let start = pair[0];
        let end = pair[1];
        if end - start < 1.0 {
            continue;
        }
        let (avg_energy, peak_energy, flux) = section_energy_stats(energy_per_second, start, end);
        candidates.push(SectionCandidate {
            start,
            end,
            avg_energy,
            peak_energy,
            flux,
            index: candidates.len() as u32,
        });
    }

    candidates
}

fn push_section_boundary(boundaries: &mut Vec<f32>, value: f32, duration: f32) {
    let value = value.clamp(0.0, duration);
    if boundaries
        .last()
        .is_none_or(|last| (value - *last).abs() >= 0.25)
    {
        boundaries.push(value);
    }
}

fn section_energy_stats(energy_per_second: &[f32], start: f32, end: f32) -> (f32, f32, f32) {
    if energy_per_second.is_empty() {
        return (0.0, 0.0, 0.0);
    }

    let start_idx = start.floor().max(0.0) as usize;
    let end_idx = end.ceil().max(start + 1.0) as usize;
    let start_idx = start_idx.min(energy_per_second.len() - 1);
    let end_idx = end_idx.clamp(start_idx + 1, energy_per_second.len());
    let slice = &energy_per_second[start_idx..end_idx];

    let mut sum = 0.0;
    let mut peak = 0.0;
    let mut flux_sum = 0.0;
    let mut prev = slice[0];

    for (i, &value) in slice.iter().enumerate() {
        sum += value;
        if value > peak {
            peak = value;
        }
        if i > 0 {
            flux_sum += (value - prev).abs();
            prev = value;
        }
    }

    let avg = sum / slice.len() as f32;
    let flux = if slice.len() > 1 {
        flux_sum / (slice.len() - 1) as f32
    } else {
        0.0
    };

    (avg, peak, flux)
}

fn build_section_energy_profile(
    candidates: &[SectionCandidate],
    intro_boundary: f32,
    outro_boundary: f32,
) -> Option<SectionEnergyProfile> {
    let mut values = candidates
        .iter()
        .filter(|candidate| {
            candidate.end > intro_boundary + 0.5 && candidate.start < outro_boundary - 0.5
        })
        .map(|candidate| candidate.avg_energy)
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();

    if values.is_empty() {
        return None;
    }

    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p25 = percentile_sorted(&values, 0.25);
    let median = percentile_sorted(&values, 0.5);
    let p75 = percentile_sorted(&values, 0.75);
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    let spread = (p75 - p25).max(0.0);
    let high_threshold = if spread < 0.08 {
        (mean + 0.08).max(p75)
    } else {
        (p75 * 0.7 + mean * 0.3).max(mean + spread * 0.25)
    }
    .clamp(0.35, 0.9);
    let low_threshold = (p25 * 0.85).min(mean * 0.8).clamp(0.05, 0.7);

    Some(SectionEnergyProfile {
        median,
        low_threshold,
        high_threshold,
        spread,
        content_count: values.len(),
    })
}

fn percentile_sorted(values: &[f32], percentile: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }

    let index = ((values.len() - 1) as f32 * percentile.clamp(0.0, 1.0)).round() as usize;
    values[index.min(values.len() - 1)]
}

fn classify_section_candidate(
    candidate: &SectionCandidate,
    profile: SectionEnergyProfile,
    intro_boundary: f32,
    outro_boundary: f32,
    content_end: f32,
    duration: f32,
    bpm_confidence: f32,
) -> (SongSectionKind, f32) {
    let bpm_bonus = bpm_confidence * 0.12;

    if candidate.start >= content_end - 0.25 && duration - content_end >= 1.0 {
        return (SongSectionKind::Silence, (0.85 + bpm_bonus).min(1.0));
    }

    if candidate.end <= intro_boundary + 0.5 || (candidate.index == 0 && candidate.start < 0.5) {
        let cue_confidence = if intro_boundary >= SECTION_MIN_SECONDS {
            0.72
        } else {
            0.5
        };
        return (
            SongSectionKind::Start,
            (cue_confidence + bpm_bonus).min(1.0),
        );
    }

    if candidate.start >= outro_boundary - 0.25 || candidate.end > content_end - 0.5 {
        return (SongSectionKind::Outro, (0.68 + bpm_bonus).min(1.0));
    }

    if candidate.avg_energy <= profile.low_threshold
        && candidate.peak_energy < profile.median.max(profile.low_threshold) * 1.05
    {
        return (SongSectionKind::Breakdown, (0.48 + bpm_bonus).min(1.0));
    }

    if profile.content_count >= 2 && candidate.avg_energy >= profile.high_threshold {
        let energy_margin = candidate.avg_energy - profile.high_threshold;
        return (
            SongSectionKind::Chorus,
            (0.58 + energy_margin * 0.8 + bpm_bonus).clamp(0.0, 1.0),
        );
    }

    if profile.spread >= 0.1
        && candidate.flux >= 0.12
        && candidate.avg_energy < profile.median * 0.95
    {
        return (SongSectionKind::Bridge, (0.45 + bpm_bonus).min(1.0));
    }

    let verse_confidence = if candidate.avg_energy >= profile.low_threshold {
        0.55
    } else {
        0.42
    };
    (
        SongSectionKind::Verse,
        (verse_confidence + bpm_bonus).min(1.0),
    )
}

fn promote_best_chorus_if_needed(
    labeled: &mut [LabeledSectionCandidate],
    profile: SectionEnergyProfile,
) {
    if labeled
        .iter()
        .any(|section| section.section_type == SongSectionKind::Chorus)
    {
        return;
    }

    let Some((index, best)) = labeled
        .iter()
        .enumerate()
        .filter(|(_, section)| {
            matches!(
                section.section_type,
                SongSectionKind::Verse | SongSectionKind::Bridge
            )
        })
        .max_by(|(_, a), (_, b)| {
            a.candidate
                .avg_energy
                .partial_cmp(&b.candidate.avg_energy)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    else {
        return;
    };

    if best.candidate.avg_energy >= (profile.median + 0.04).max(0.45) || profile.spread >= 0.08 {
        labeled[index].section_type = SongSectionKind::Chorus;
        labeled[index].confidence = labeled[index].confidence.max(0.44);
    }
}

fn merge_labeled_sections(labeled: Vec<LabeledSectionCandidate>) -> Vec<SongSection> {
    let mut sections: Vec<SongSection> = Vec::new();

    for item in labeled {
        let duration = (item.candidate.end - item.candidate.start).max(0.001);
        if let Some(last) = sections.last_mut() {
            if last.section_type == item.section_type
                && (item.candidate.start - last.end).abs() <= 0.25
            {
                let last_duration = (last.end - last.start).max(0.001);
                let total_duration = last_duration + duration;
                last.confidence =
                    (last.confidence * last_duration + item.confidence * duration) / total_duration;
                last.energy = (last.energy * last_duration + item.candidate.avg_energy * duration)
                    / total_duration;
                last.vocal_risk =
                    (last.vocal_risk * last_duration + item.vocal_risk * duration) / total_duration;
                last.mix_suitability = (last.mix_suitability * last_duration
                    + item.mix_suitability * duration)
                    / total_duration;
                last.end = item.candidate.end;
                continue;
            }
        }

        if sections.len() >= MAX_SONG_SECTIONS {
            break;
        }

        sections.push(SongSection {
            section_type: item.section_type,
            start: item.candidate.start,
            end: item.candidate.end,
            index: 0,
            confidence: item.confidence,
            energy: item.candidate.avg_energy,
            vocal_risk: item.vocal_risk,
            mix_suitability: item.mix_suitability,
        });
    }

    sections
}

fn section_vocal_risk(
    vocal_activity: Option<&VocalActivityAnalysis>,
    start: f32,
    end: f32,
) -> Option<f32> {
    let vocal = vocal_activity?;
    if vocal.risk.is_empty() || vocal.window_duration <= 0.0 || end <= start {
        return None;
    }

    let start_idx = (start / vocal.window_duration).floor().max(0.0) as usize;
    let end_idx = (end / vocal.window_duration)
        .ceil()
        .max(start_idx as f32 + 1.0) as usize;
    let start_idx = start_idx.min(vocal.risk.len() - 1);
    let end_idx = end_idx.clamp(start_idx + 1, vocal.risk.len());
    let slice = &vocal.risk[start_idx..end_idx];
    let mut sorted = slice.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(percentile_sorted(&sorted, 0.7).clamp(0.0, 1.0))
}

fn vocal_risk_at(vocal_activity: Option<&VocalActivityAnalysis>, time: f32) -> f32 {
    let Some(vocal) = vocal_activity else {
        return 0.5;
    };
    if vocal.risk.is_empty() || vocal.window_duration <= 0.0 {
        return 0.5;
    }

    let idx = (time / vocal.window_duration).round().max(0.0) as usize;
    vocal.risk[idx.min(vocal.risk.len() - 1)].clamp(0.0, 1.0)
}

fn section_mix_suitability(
    section_type: SongSectionKind,
    energy: f32,
    confidence: f32,
    vocal_risk: f32,
) -> f32 {
    let base = match section_type {
        SongSectionKind::Silence => 0.92,
        SongSectionKind::Outro => 0.82,
        SongSectionKind::Breakdown => 0.72,
        SongSectionKind::Bridge => 0.55,
        SongSectionKind::Verse => 0.26,
        SongSectionKind::Start => 0.14,
        SongSectionKind::Chorus => 0.08,
    };

    (base + (1.0 - energy.clamp(0.0, 1.0)) * 0.18 + confidence * 0.12 - vocal_risk * 0.35)
        .clamp(0.0, 1.0)
}

pub(super) fn build_mix_point_analysis(
    energy: &EnergyAnalysis,
    bpm: Option<&BPMResult>,
    outro: Option<&OutroAnalysis>,
    phrases: Option<&PhraseAnalysis>,
    sections: Option<&SectionAnalysis>,
    vocal_activity: Option<&VocalActivityAnalysis>,
    duration: f32,
) -> Option<MixPointAnalysis> {
    if duration < SECTION_MIN_SECONDS * 2.0 || energy.energy_per_second.is_empty() {
        return None;
    }

    let content_end = (duration - energy.trailing_silence).max(0.0).min(duration);
    let latest = (content_end - SECTION_MIN_SECONDS).max(0.0);
    if latest <= 0.0 {
        return None;
    }

    let mut candidates = Vec::new();
    if let Some(outro) = outro {
        push_mix_candidate(
            &mut candidates,
            outro.suggested_crossfade_start,
            "outroClassifier",
            None,
            0.55 + outro.outro_confidence * 0.25,
            energy,
            vocal_activity,
            latest,
        );

        if let Some(outro_start) = outro.outro_section_start {
            push_mix_candidate(
                &mut candidates,
                outro_start,
                "outroSection",
                Some(SongSectionKind::Outro),
                0.72 + outro.outro_confidence * 0.18,
                energy,
                vocal_activity,
                latest,
            );
        }
    }

    push_mix_candidate(
        &mut candidates,
        duration - energy.outro_start_offset,
        "energyOutro",
        Some(SongSectionKind::Outro),
        if energy.is_fade_out { 0.74 } else { 0.58 },
        energy,
        vocal_activity,
        latest,
    );

    if let Some(phrases) = phrases {
        if let Some(phrase) = phrases.mix_out_phrase.as_ref() {
            push_mix_candidate(
                &mut candidates,
                phrase.start,
                "phraseBoundary",
                None,
                0.62,
                energy,
                vocal_activity,
                latest,
            );
        }
    }

    if let Some(sections) = sections {
        for section in &sections.sections {
            let section_duration = section.end - section.start;
            if section_duration < 1.0 {
                continue;
            }
            let section_time = match section.section_type {
                SongSectionKind::Outro | SongSectionKind::Silence => section.start,
                SongSectionKind::Breakdown | SongSectionKind::Bridge => {
                    section.start + section_duration * 0.25
                }
                SongSectionKind::Verse => section.start + section_duration * 0.85,
                SongSectionKind::Chorus => section.start + section_duration * 0.95,
                SongSectionKind::Start => continue,
            };
            push_mix_candidate(
                &mut candidates,
                section_time,
                "section",
                Some(section.section_type),
                section.mix_suitability,
                energy,
                vocal_activity,
                latest,
            );
        }
    }

    if let Some(bpm) = bpm {
        if bpm.confidence >= 0.3 && bpm.bpm.is_finite() && bpm.bpm > 0.0 {
            for beat in &bpm.beat_grid {
                let time = *beat + bpm.analysis_offset;
                if time < latest - SECTION_MAX_SECONDS || time > latest {
                    continue;
                }
                push_mix_candidate(
                    &mut candidates,
                    time,
                    "beatNearOutro",
                    None,
                    0.46 + bpm.confidence * 0.15,
                    energy,
                    vocal_activity,
                    latest,
                );
            }
        }
    }

    dedupe_mix_candidates(&mut candidates);
    if candidates.is_empty() {
        return None;
    }

    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let selected = candidates.first().cloned();

    Some(MixPointAnalysis {
        candidates,
        selected,
    })
}

fn push_mix_candidate(
    candidates: &mut Vec<MixPointCandidate>,
    time: f32,
    reason: &str,
    section_type: Option<SongSectionKind>,
    base_score: f32,
    energy: &EnergyAnalysis,
    vocal_activity: Option<&VocalActivityAnalysis>,
    latest: f32,
) {
    if !time.is_finite() {
        return;
    }

    let time = time.clamp(0.0, latest);
    let sec = time.floor().max(0.0) as usize;
    let local_energy = energy
        .energy_per_second
        .get(sec.min(energy.energy_per_second.len().saturating_sub(1)))
        .copied()
        .unwrap_or(energy.average_energy)
        .clamp(0.0, 1.0);
    let vocal_risk = vocal_risk_at(vocal_activity, time);
    let score = (base_score + (1.0 - local_energy) * 0.14 - vocal_risk * 0.38).clamp(0.0, 1.0);

    candidates.push(MixPointCandidate {
        time,
        score,
        reason: reason.to_string(),
        section_type,
        vocal_risk,
        energy: local_energy,
    });
}

fn dedupe_mix_candidates(candidates: &mut Vec<MixPointCandidate>) {
    candidates.sort_by(|a, b| {
        a.time
            .partial_cmp(&b.time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut deduped: Vec<MixPointCandidate> = Vec::with_capacity(candidates.len());
    for candidate in candidates.drain(..) {
        if let Some(last) = deduped.last_mut() {
            if (candidate.time - last.time).abs() < 0.75 {
                if candidate.score > last.score {
                    *last = candidate;
                }
                continue;
            }
        }
        deduped.push(candidate);
    }

    *candidates = deduped;
}

// ─── Outro Classification (Simplified) ─────────────────────────────

struct ClassificationResult {
    outro_type: String,
    outro_confidence: f32,
    musical_end_offset: f32,
    suggested_crossfade_start: f32,
    deceleration_start: Option<f32>,
    sustain_onset: Option<f32>,
    outro_section_start: Option<f32>,
    loop_period: Option<f32>,
}

fn classify_outro_simple(
    low: &[f32],
    mid: &[f32],
    high: &[f32],
    _flux: &[f32],
    _loudness: &[f32],
    duration: f32,
    trailing_silence: f32,
    _analysis_duration: f32,
    window_count: usize,
) -> ClassificationResult {
    // Total energy per window
    let total_energy: Vec<f32> = low
        .iter()
        .zip(mid.iter())
        .zip(high.iter())
        .map(|((&l, &m), &h)| l + m + h)
        .collect();

    // Check for fade-out: monotonically decreasing over last 20+ windows (5s+)
    let min_fade_windows = 20usize;
    let mut fade_detected = false;

    if window_count >= min_fade_windows {
        for start in
            (window_count.saturating_sub(60))..(window_count.saturating_sub(min_fade_windows))
        {
            let fade_len = window_count - start;
            if window_count - (start + fade_len) > 4 {
                continue;
            }

            let start_energy = total_energy[start];
            let end_energy = total_energy[window_count - 1];
            if start_energy < 0.01 || end_energy / start_energy >= 0.15 {
                continue;
            }

            // Check monotonic decrease (allow 10% increases)
            let max_increases = (fade_len as f32 * 0.1) as usize;
            let mut increases = 0usize;
            let mut decreasing = true;

            for i in (start + 1)..window_count {
                if total_energy[i] > total_energy[i - 1] * 1.05 {
                    increases += 1;
                    if increases > max_increases {
                        decreasing = false;
                        break;
                    }
                }
            }

            if decreasing {
                fade_detected = true;
                break;
            }
        }
    }

    if fade_detected {
        let musical_end_offset = trailing_silence + 2.0;
        ClassificationResult {
            outro_type: "fadeOut".to_string(),
            outro_confidence: 0.7,
            musical_end_offset,
            suggested_crossfade_start: (duration - musical_end_offset * 1.3).max(15.0),
            deceleration_start: None,
            sustain_onset: None,
            outro_section_start: None,
            loop_period: None,
        }
    } else {
        // Default: hard ending
        let musical_end_offset = trailing_silence;
        ClassificationResult {
            outro_type: "hard".to_string(),
            outro_confidence: 0.3,
            musical_end_offset: musical_end_offset,
            suggested_crossfade_start: (duration
                - musical_end_offset
                - (musical_end_offset * 0.5).min(4.0)
                - 4.0)
                .max(15.0),
            deceleration_start: None,
            sustain_onset: None,
            outro_section_start: None,
            loop_period: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::analyze_song_sections;
    use crate::automix::{BPMResult, EnergyAnalysis, IntroAnalysis, SongSectionKind};

    #[test]
    fn song_sections_detect_basic_pop_structure() {
        let mut energy_per_second = Vec::new();
        energy_per_second.extend(std::iter::repeat(0.20).take(8));
        energy_per_second.extend(std::iter::repeat(0.45).take(16));
        energy_per_second.extend(std::iter::repeat(0.88).take(16));
        energy_per_second.extend(std::iter::repeat(0.48).take(16));
        energy_per_second.extend(std::iter::repeat(0.90).take(16));
        energy_per_second.extend(std::iter::repeat(0.25).take(8));

        let average_energy = energy_per_second.iter().sum::<f32>() / energy_per_second.len() as f32;
        let energy = EnergyAnalysis {
            energy_per_second,
            outro_start_offset: 8.0,
            intro_end_offset: 8.0,
            average_energy,
            trailing_silence: 0.0,
            is_fade_out: false,
        };
        let bpm = BPMResult {
            bpm: 120.0,
            confidence: 0.9,
            beat_grid: Vec::new(),
            analysis_offset: 0.0,
        };
        let intro = IntroAnalysis {
            quiet_intro_duration: 4.0,
            energy_build_duration: 8.0,
            intro_energy_ratio: 0.6,
            multiband_energy: None,
        };

        let analysis = analyze_song_sections(&energy, Some(&bpm), Some(&intro), None, None, 80.0)
            .expect("expected section analysis");
        let section_types = analysis
            .sections
            .iter()
            .map(|section| section.section_type)
            .collect::<Vec<_>>();

        assert_eq!(section_types.first(), Some(&SongSectionKind::Start));
        assert!(section_types.contains(&SongSectionKind::Verse));
        assert!(section_types.contains(&SongSectionKind::Chorus));
        assert_eq!(section_types.last(), Some(&SongSectionKind::Outro));
        assert_eq!(analysis.method, "bpmPhraseEnergy");
    }

    #[test]
    fn song_sections_skip_too_short_tracks() {
        let energy = EnergyAnalysis {
            energy_per_second: vec![0.4, 0.5, 0.4],
            outro_start_offset: 3.0,
            intro_end_offset: 0.0,
            average_energy: 0.43,
            trailing_silence: 0.0,
            is_fade_out: false,
        };

        assert!(analyze_song_sections(&energy, None, None, None, None, 3.0).is_none());
    }
}
