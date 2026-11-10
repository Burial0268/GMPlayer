//! Local onset evidence. Grids stop where the measured stable run stops.

use rustfft::{num_complex::Complex, FftPlanner};

use super::BeatRegion;

const RATE: usize = 11_025;
const WINDOW: usize = 1024;
const HOP: usize = 128;
const SECONDS: usize = 40;

pub(super) fn region(data: &[f32], sample_rate: u32, tail: bool) -> Option<BeatRegion> {
    if sample_rate == 0 {
        return None;
    }
    let frames = (SECONDS * sample_rate as usize).min(data.len());
    let start = if tail { data.len() - frames } else { 0 };
    let input = &data[start..start + frames];
    let len = input.len() * RATE / sample_rate as usize;
    if len < WINDOW * 2 {
        return None;
    }
    let mut fft = FftPlanner::<f32>::new();
    let fft = fft.plan_fft_forward(WINDOW);
    let mut scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
    let mut buffer = vec![Complex::default(); WINDOW];
    let mut previous = vec![0.0f32; WINDOW / 2];
    let mut flux = Vec::with_capacity(len / HOP);
    for at in (0..=len - WINDOW).step_by(HOP) {
        for (i, slot) in buffer.iter_mut().enumerate() {
            let source = (at + i) as f64 * sample_rate as f64 / RATE as f64;
            let left = source as usize;
            let fraction = (source - left as f64) as f32;
            let sample =
                input[left] + (input[(left + 1).min(input.len() - 1)] - input[left]) * fraction;
            let window = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / WINDOW as f32).cos();
            *slot = Complex::new(sample * window, 0.0);
        }
        fft.process_with_scratch(&mut buffer, &mut scratch);
        let mut novelty = 0.0;
        for bin in 3..WINDOW / 2 {
            let magnitude = buffer[bin].norm();
            novelty += (magnitude - previous[bin]).max(0.0);
            previous[bin] = magnitude;
        }
        flux.push(novelty);
    }
    let peak = flux.iter().copied().fold(0.0f32, f32::max);
    let mean = flux.iter().sum::<f32>() / flux.len() as f32;
    if peak < 1e-6 || peak < mean * 2.5 {
        return None;
    }
    let mut beats = Vec::new();
    for i in 2..flux.len().saturating_sub(2) {
        let local = &flux[i.saturating_sub(20)..(i + 21).min(flux.len())];
        let threshold = (local.iter().sum::<f32>() / local.len() as f32 * 1.5).max(peak * 0.10);
        if flux[i] < threshold || flux[i - 2..=i + 2].iter().any(|value| *value > flux[i]) {
            continue;
        }
        let time = (i * HOP + WINDOW / 2) as f64 / RATE as f64 + start as f64 / sample_rate as f64;
        if beats.last().is_none_or(|last| time - last >= 0.24) {
            beats.push(time);
        }
    }
    fit(&beats, tail)
}

fn fit(beats: &[f64], tail: bool) -> Option<BeatRegion> {
    let mut best: Option<BeatRegion> = None;
    for start in 0..beats.len().saturating_sub(7) {
        let period = beats[start + 1] - beats[start];
        if !(0.29..=1.05).contains(&period) {
            continue;
        }
        let tolerance = (period * 0.06).max(0.025);
        let mut end = start + 1;
        while end + 1 < beats.len() && ((beats[end + 1] - beats[end]) - period).abs() <= tolerance {
            end += 1;
        }
        let run = &beats[start..=end];
        if run.len() < 8 || run[run.len() - 1] - run[0] < 3.0 {
            continue;
        }
        let mean_index = (run.len() - 1) as f64 / 2.0;
        let mean_time = run.iter().sum::<f64>() / run.len() as f64;
        let numerator = run
            .iter()
            .enumerate()
            .map(|(i, t)| (i as f64 - mean_index) * (t - mean_time))
            .sum::<f64>();
        let denominator = (0..run.len())
            .map(|i| (i as f64 - mean_index).powi(2))
            .sum::<f64>();
        let period = numerator / denominator;
        let origin = mean_time - mean_index * period;
        let stability = run
            .iter()
            .enumerate()
            .filter(|(i, t)| (**t - origin - *i as f64 * period).abs() <= tolerance)
            .count() as f64
            / run.len() as f64;
        if stability < 0.9 {
            continue;
        }
        let replace = best.as_ref().is_none_or(|old| {
            if tail {
                run[run.len() - 1] > *old.beats.last().unwrap() as f64
            } else {
                run.len() > old.beats.len()
            }
        });
        if replace {
            best = Some(BeatRegion {
                period: period as f32,
                stability: stability as f32,
                beats: (0..run.len())
                    .map(|i| (origin + i as f64 * period) as f32)
                    .collect(),
            });
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clicks(period: f64, offset: f64, seconds: usize) -> Vec<f32> {
        let rate = 22_050;
        let mut data = vec![0.0; rate * seconds];
        let mut at = offset;
        while at < seconds as f64 {
            let start = (at * rate as f64) as usize;
            for i in 0..220 {
                if start + i < data.len() {
                    data[start + i] = (i as f32 * 0.7).sin() * (1.0 - i as f32 / 220.0);
                }
            }
            at += period;
        }
        data
    }

    #[test]
    fn measured_grid_keeps_the_pulse_phase_and_the_tail_offset() {
        let data = clicks(0.5, 0.173, 90);
        for tail in [false, true] {
            let grid = region(&data, 22_050, tail).expect("stable clicks");
            assert!((grid.period - 0.5).abs() < 0.002);
            let phase = (grid.beats[0] as f64 - 0.173).rem_euclid(0.5);
            assert!(phase.min(0.5 - phase) < 0.035, "phase={phase}");
            if tail {
                assert!(grid.beats[0] > 49.0);
            }
        }
    }

    #[test]
    fn silence_and_irregular_pulses_do_not_produce_a_stable_grid() {
        assert!(region(&vec![0.0; 22_050 * 8], 22_050, false).is_none());
        let beats = [0.1, 0.52, 1.13, 1.48, 2.2, 2.67, 3.4, 3.71, 4.25];
        assert!(fit(&beats, false).is_none());
    }
}
