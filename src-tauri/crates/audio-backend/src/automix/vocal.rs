//! Lightweight vocal-risk analysis for AutoMix.
//!
//! This is intentionally heuristic. It identifies likely vocal-heavy windows so
//! the planner can prefer safer mix points; it is not source separation.

use super::analysis::{design_biquad_bandpass, iir_process};
use super::{VocalActivityAnalysis, VOCAL_WINDOW_SECONDS};

pub(super) fn analyze_vocal_activity(
    data: &[f32],
    sample_rate: u32,
    duration: f32,
) -> Option<VocalActivityAnalysis> {
    if data.is_empty() || sample_rate == 0 || duration < 4.0 {
        return None;
    }

    let window_samples = (sample_rate as f32 * VOCAL_WINDOW_SECONDS).round() as usize;
    if window_samples < 1 {
        return None;
    }

    let sample_rate_f = sample_rate as f32;
    let nyquist = sample_rate_f / 2.0;
    if nyquist <= 450.0 {
        return None;
    }

    let low_c = design_biquad_bandpass(80.0, 300.0_f32.min(nyquist - 50.0), sample_rate_f);
    let mid_c = design_biquad_bandpass(300.0, 4000.0_f32.min(nyquist - 50.0), sample_rate_f);
    let high_c = if nyquist > 4300.0 {
        Some(design_biquad_bandpass(
            4000.0,
            (nyquist - 100.0).min(12000.0),
            sample_rate_f,
        ))
    } else {
        None
    };

    let (mut lz1, mut lz2, mut mz1, mut mz2, mut hz1, mut hz2) =
        (0f32, 0f32, 0f32, 0f32, 0f32, 0f32);
    let mut low = Vec::with_capacity(data.len() / window_samples + 1);
    let mut mid = Vec::with_capacity(low.capacity());
    let mut high = Vec::with_capacity(low.capacity());

    let mut accum_low = 0f64;
    let mut accum_mid = 0f64;
    let mut accum_high = 0f64;
    let mut samples_in_window = 0usize;

    for &sample in data {
        let filtered_low = iir_process(&low_c, &mut lz1, &mut lz2, sample);
        let filtered_mid = iir_process(&mid_c, &mut mz1, &mut mz2, sample);
        let filtered_high = if let Some(coeffs) = high_c.as_ref() {
            iir_process(coeffs, &mut hz1, &mut hz2, sample)
        } else {
            0.0
        };

        accum_low += (filtered_low as f64) * (filtered_low as f64);
        accum_mid += (filtered_mid as f64) * (filtered_mid as f64);
        accum_high += (filtered_high as f64) * (filtered_high as f64);
        samples_in_window += 1;

        if samples_in_window >= window_samples {
            push_window(
                &mut low,
                &mut mid,
                &mut high,
                accum_low,
                accum_mid,
                accum_high,
                samples_in_window,
            );
            accum_low = 0.0;
            accum_mid = 0.0;
            accum_high = 0.0;
            samples_in_window = 0;
        }
    }

    if samples_in_window >= window_samples / 2 {
        push_window(
            &mut low,
            &mut mid,
            &mut high,
            accum_low,
            accum_mid,
            accum_high,
            samples_in_window,
        );
    }

    if mid.len() < 4 {
        return None;
    }

    let mut total = Vec::with_capacity(mid.len());
    let mut max_total = 0.0001f32;
    for i in 0..mid.len() {
        let value = low[i] + mid[i] + high[i];
        if value > max_total {
            max_total = value;
        }
        total.push(value);
    }

    let mut risk = vec![0f32; mid.len()];
    for i in 0..mid.len() {
        let energy = total[i];
        if energy < max_total * 0.015 || energy < 0.00001 {
            continue;
        }

        let mid_ratio = mid[i] / energy;
        let low_ratio = low[i] / energy;
        let high_ratio = high[i] / energy;
        let prev_mid_ratio = if i > 0 {
            mid[i - 1] / total[i - 1].max(0.00001)
        } else {
            mid_ratio
        };
        let next_mid_ratio = if i + 1 < mid.len() {
            mid[i + 1] / total[i + 1].max(0.00001)
        } else {
            mid_ratio
        };
        let persistence = (prev_mid_ratio + mid_ratio + next_mid_ratio) / 3.0;
        let flux_ratio = if i > 0 {
            (energy - total[i - 1]).abs() / energy.max(total[i - 1]).max(0.00001)
        } else {
            0.0
        };

        let band_score = smoothstep(0.48, 0.74, mid_ratio);
        let persistence_score = smoothstep(0.45, 0.68, persistence);
        let energy_gate = smoothstep(0.04, 0.16, energy / max_total);
        let low_penalty = smoothstep(0.42, 0.62, low_ratio) * 0.25;
        let high_penalty = smoothstep(0.42, 0.62, high_ratio) * 0.15;
        let transient_penalty = smoothstep(0.32, 0.65, flux_ratio) * 0.25;

        risk[i] = ((band_score * 0.55 + persistence_score * 0.45) * energy_gate
            - low_penalty
            - high_penalty
            - transient_penalty)
            .clamp(0.0, 1.0);
    }

    suppress_isolated_windows(&mut risk);
    let active_windows = risk.iter().filter(|&&value| value >= 0.55).count();
    let confidence = if risk.len() >= 16 {
        (0.55 + (active_windows as f32 / risk.len() as f32) * 0.25).clamp(0.55, 0.8)
    } else {
        0.45
    };

    Some(VocalActivityAnalysis {
        window_duration: VOCAL_WINDOW_SECONDS,
        risk,
        confidence,
        method: "multibandHeuristic".to_string(),
    })
}

fn push_window(
    low: &mut Vec<f32>,
    mid: &mut Vec<f32>,
    high: &mut Vec<f32>,
    accum_low: f64,
    accum_mid: f64,
    accum_high: f64,
    sample_count: usize,
) {
    let count = sample_count.max(1) as f64;
    low.push((accum_low / count).sqrt() as f32);
    mid.push((accum_mid / count).sqrt() as f32);
    high.push((accum_high / count).sqrt() as f32);
}

fn suppress_isolated_windows(risk: &mut [f32]) {
    if risk.len() < 3 {
        return;
    }

    let original = risk.to_vec();
    for i in 0..risk.len() {
        if original[i] < 0.55 {
            continue;
        }
        let prev = i > 0 && original[i - 1] >= 0.4;
        let next = i + 1 < original.len() && original[i + 1] >= 0.4;
        if !prev && !next {
            risk[i] *= 0.5;
        }
    }
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    if edge1 <= edge0 {
        return if value >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
