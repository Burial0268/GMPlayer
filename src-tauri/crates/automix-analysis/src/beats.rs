//! Local onset evidence. Grids stop where the measured stable run stops.

use rustfft::{num_complex::Complex, FftPlanner};

use super::BeatRegion;

const RATE: usize = 11_025;
const WINDOW: usize = 1024;
const HOP: usize = 128;
pub(super) const SECONDS: usize = 40;

pub(super) fn region(data: &[f32], sample_rate: u32, tail: bool) -> Option<BeatRegion> {
    if sample_rate == 0 {
        return None;
    }
    let frames = (SECONDS * sample_rate as usize).min(data.len());
    let start = if tail { data.len() - frames } else { 0 };
    let input = &data[start..start + frames];
    window_region(input, sample_rate, start, tail)
}

pub(super) fn window_region(
    input: &[f32],
    sample_rate: u32,
    start: usize,
    tail: bool,
) -> Option<BeatRegion> {
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
    let start_seconds = start as f64 / sample_rate as f64 + (WINDOW / 2) as f64 / RATE as f64;
    grid(&flux, start_seconds, tail)
}

/// On-phase over off-phase strength that marks the beat level (measured split 1.6 / 2.9).
const BEAT_CONTRAST: f32 = 2.2;
/// Seconds per flux frame.
const FRAME: f64 = HOP as f64 / RATE as f64;

/// Fits one fixed grid to the onset envelope. Extra onsets (hats, ghost notes)
/// and a few missed beats do not break it, which a run of equal peak-to-peak
/// intervals cannot survive on a real groove.
fn grid(flux: &[f32], offset: f64, tail: bool) -> Option<BeatRegion> {
    let n = flux.len();
    let (min_lag, max_lag) = ((0.29 / FRAME).ceil() as usize, (1.05 / FRAME) as usize);
    if n < max_lag * 4 || flux.iter().all(|v| *v < 1e-6) {
        return None;
    }
    // Flux above its ~0.25 s local mean, so a loudness swell does not read as a beat.
    let onset: Vec<f32> = (0..n)
        .map(|i| {
            let local = &flux[i.saturating_sub(20)..(i + 21).min(n)];
            (flux[i] - local.iter().sum::<f32>() / local.len() as f32).max(0.0)
        })
        .collect();
    // Coarse tempo: autocorrelation weighted towards 120 BPM, one octave of spread.
    let score = |lag: usize| {
        let ac = onset[lag..].iter().zip(&onset).map(|(a, b)| a * b).sum::<f32>() / (n - lag) as f32;
        let octaves = (lag as f64 * FRAME / 0.5).log2();
        ac as f64 * (-0.5 * octaves * octaves).exp()
    };
    let lag = (min_lag..=max_lag).max_by(|a, b| score(*a).total_cmp(&score(*b)))?;
    // Fine period: a 0.02-frame error would already drift ~20 ms across 40 s.
    let mut period = (0..=100)
        .map(|step| lag as f64 - 1.0 + step as f64 * 0.02)
        .max_by(|a, b| best_phase(&onset, *a).1.total_cmp(&best_phase(&onset, *b).1))?;
    // Metrical level: the beat's "and" carries only hats, so its on-phase beats its
    // off-phase ~3-7x; a half-bar's off-phase is the snare against the kick, ~1.6x.
    while period * 2.0 <= max_lag as f64 && contrast(&onset, period * 2.0) >= BEAT_CONTRAST {
        period *= 2.0;
    }
    while period / 2.0 >= min_lag as f64 && contrast(&onset, period) < BEAT_CONTRAST {
        period /= 2.0;
    }
    let (phase, strength) = best_phase(&onset, period);
    if strength <= 0.0 {
        return None;
    }
    let tolerance = (period * FRAME * 0.06).max(0.025) / FRAME;
    // Predicted beat -> the strongest onset near it, if strong enough to count.
    let hits: Vec<Option<f64>> = (0..)
        .map(|k| phase + k as f64 * period)
        .take_while(|at| *at + tolerance < n as f64)
        .map(|at| {
            let from = (at - tolerance).max(0.0).round() as usize;
            let to = ((at + tolerance).round() as usize).min(n - 1);
            let peak = (from..=to).max_by(|a, b| onset[*a].total_cmp(&onset[*b]))?;
            (onset[peak] >= strength * 0.3).then_some(peak as f64)
        })
        .collect();
    // Runs of hits bridging at most two consecutive misses.
    let mut best: Option<(usize, usize)> = None;
    let mut run: Option<(usize, usize)> = None;
    for (k, hit) in hits.iter().enumerate() {
        if hit.is_some() {
            run = Some(match run {
                Some((first, last)) if k - last <= 3 => (first, k),
                _ => (k, k),
            });
            let (first, last) = run.unwrap();
            let replace = best.is_none_or(|(old_first, old_last)| {
                if tail {
                    last > old_last
                } else {
                    last - first > old_last - old_first
                }
            });
            // A tail run is only replaced by one still long enough to fit.
            if replace && (!tail || hits[first..=last].iter().flatten().count() >= 8) {
                best = Some((first, last));
            }
        }
    }
    let (first, last) = best?;
    let observed: Vec<(f64, f64)> = (first..=last)
        .filter_map(|k| hits[k].map(|frame| ((k - first) as f64, frame)))
        .collect();
    if observed.len() < 8 || (last - first) as f64 * period * FRAME < 3.0 {
        return None;
    }
    // Least squares over the observed onsets, then how many beats fall on the line.
    let count = observed.len() as f64;
    let mean_index = observed.iter().map(|(i, _)| i).sum::<f64>() / count;
    let mean_frame = observed.iter().map(|(_, f)| f).sum::<f64>() / count;
    let slope = observed
        .iter()
        .map(|(i, f)| (i - mean_index) * (f - mean_frame))
        .sum::<f64>()
        / observed.iter().map(|(i, _)| (i - mean_index).powi(2)).sum::<f64>();
    let origin = mean_frame - mean_index * slope;
    let fitting = observed
        .iter()
        .filter(|(i, f)| (f - origin - i * slope).abs() <= tolerance)
        .count();
    let beats = last - first + 1;
    Some(BeatRegion {
        period: (slope * FRAME) as f32,
        stability: (fitting as f64 / beats as f64) as f32,
        beats: (0..beats)
            .map(|i| (offset + (origin + i as f64 * slope) * FRAME) as f32)
            .collect(),
    })
}

/// Mean onset strength on the grid `phase + k * period`, allowing ±1 frame.
fn comb(onset: &[f32], period: f64, phase: f64) -> f32 {
    let (mut sum, mut count) = (0.0, 0);
    let mut at = phase;
    while at.round() as usize + 1 < onset.len() {
        let i = at.round() as usize;
        sum += onset[i.saturating_sub(1)..=i + 1].iter().copied().fold(0.0, f32::max);
        count += 1;
        at += period;
    }
    if count == 0 {
        0.0
    } else {
        sum / count as f32
    }
}

fn best_phase(onset: &[f32], period: f64) -> (f64, f32) {
    (0..period.ceil() as usize)
        .map(|phase| (phase as f64, comb(onset, period, phase as f64)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((0.0, 0.0))
}

/// How much stronger the grid's best phase is than the phase half a period away.
fn contrast(onset: &[f32], period: f64) -> f32 {
    let (phase, on) = best_phase(onset, period);
    on / comb(onset, period, phase + period / 2.0).max(1e-9)
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
        let rate = 22_050;
        let mut data = vec![0.0; rate * 30];
        let mut at = 0.1;
        let mut step = 0usize;
        while at < 29.0 {
            let start = (at * rate as f64) as usize;
            for i in 0..220 {
                data[start + i] = (i as f32 * 0.7).sin() * (1.0 - i as f32 / 220.0);
            }
            // Irregular gaps between 0.31 s and 0.89 s.
            at += 0.31 + ((step * 7919) % 59) as f64 / 100.0;
            step += 1;
        }
        assert!(region(&data, rate as u32, false).is_none());
    }
}

#[cfg(test)]
mod drum_probe {
    use super::*;

    /// Kick on 1, 3 and the "and" of 3, snare on 2/4, closed hats on every
    /// eighth, a sustained pad underneath: what a pop/EDM groove looks like.
    pub(super) fn groove(bpm: f64, offset: f64, seconds: usize, rate: usize) -> Vec<f32> {
        let beat = 60.0 / bpm;
        let mut data: Vec<f32> = (0..rate * seconds)
            .map(|i| {
                let t = i as f32 / rate as f32;
                0.15 * ((t * 220.0 * std::f32::consts::TAU).sin()
                    + (t * 277.2 * std::f32::consts::TAU).sin())
            })
            .collect();
        let mut seed = 12345u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (seed >> 16) as f32 / 32_768.0 - 1.0
        };
        let mut hit = |data: &mut Vec<f32>, at: f64, kind: u8, gain: f32| {
            let start = (at * rate as f64) as usize;
            let len = rate / if kind == 2 { 30 } else { 8 };
            for i in 0..len {
                let Some(slot) = data.get_mut(start + i) else { break };
                let t = i as f32 / rate as f32;
                let env = (-t * if kind == 2 { 60.0 } else { 18.0 }).exp();
                let s = match kind {
                    0 => (std::f32::consts::TAU * (50.0 + 90.0 * (-t * 30.0).exp()) * t).sin(),
                    1 => 0.5 * (std::f32::consts::TAU * 190.0 * t).sin() + 0.6 * noise(),
                    _ => 0.35 * noise(),
                };
                *slot += s * env * gain;
            }
        };
        let mut n = 0usize;
        while offset + n as f64 * beat / 2.0 < seconds as f64 {
            let at = offset + n as f64 * beat / 2.0;
            let step = n % 8; // eighths within a bar
            hit(&mut data, at, 2, if n % 2 == 0 { 0.8 } else { 0.5 });
            if matches!(step, 0 | 4 | 5) {
                hit(&mut data, at, 0, 1.0);
            }
            if matches!(step, 2 | 6) {
                hit(&mut data, at, 1, 0.9);
            }
            n += 1;
        }
        data
    }

    #[test]
    fn a_drum_groove_locks_to_the_beat_and_its_phase() {
        for bpm in [92.0, 122.0, 128.0, 140.0, 174.0] {
            let data = groove(bpm, 0.31, 60, 22_050);
            for tail in [false, true] {
                let grid = region(&data, 22_050, tail).expect("a groove has a grid");
                let found = 60.0 / grid.period as f64;
                assert!((found / bpm - 1.0).abs() < 0.003, "bpm={bpm} tail={tail} found={found}");
                assert!(grid.stability >= 0.9 && grid.beats.len() >= 30, "bpm={bpm} tail={tail}");
                let beat = 60.0 / bpm;
                let phase = (grid.beats[0] as f64 - 0.31).rem_euclid(beat);
                assert!(phase.min(beat - phase) < 0.02, "bpm={bpm} tail={tail} phase={phase}");
            }
        }
    }
}
