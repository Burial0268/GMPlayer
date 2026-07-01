//! Signal-analysis primitives for AutoMix: biquad filters, volume/energy,
//! BPM detection, and spectral fingerprinting. Pure functions over PCM.

use super::{
    BPMResult, EnergyAnalysis, SpectralFingerprint, VolumeAnalysis, BPM_ANALYSIS_DURATION,
    BPM_ANALYSIS_RATE, MAX_BPM, MIN_BPM, REFERENCE_RMS, SILENCE_THRESHOLD, TARGET_LUFS,
};

// ─── Biquad IIR Filter ──────────────────────────────────────────────

pub(super) struct BiquadCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

pub(super) fn design_biquad_bandpass(f_low: f32, f_high: f32, sample_rate: f32) -> BiquadCoeffs {
    let center = (f_low * f_high).sqrt();
    let bandwidth = f_high - f_low;
    let w0 = 2.0 * std::f32::consts::PI * center / sample_rate;
    let q = center / bandwidth;
    let alpha = w0.sin() / (2.0 * q);
    let a0 = 1.0 + alpha;

    BiquadCoeffs {
        b0: alpha / a0,
        b1: 0.0,
        b2: -alpha / a0,
        a1: -2.0 * w0.cos() / a0,
        a2: (1.0 - alpha) / a0,
    }
}

pub(super) fn design_k_weight_shelf(sample_rate: f32) -> BiquadCoeffs {
    let f0 = 2000f32;
    let gain_db = 4f32;
    let w0 = 2.0 * std::f32::consts::PI * f0 / sample_rate;
    let a = 10f32.powf(gain_db / 40.0);
    let alpha = w0.sin() / (2.0 * 0.707);

    let a0 = a + 1.0 - (a - 1.0) * w0.cos() + 2.0 * a.sqrt() * alpha;
    BiquadCoeffs {
        b0: (a * (a + 1.0 + (a - 1.0) * w0.cos() + 2.0 * a.sqrt() * alpha)) / a0,
        b1: (-2.0 * a * (a - 1.0 + (a + 1.0) * w0.cos())) / a0,
        b2: (a * (a + 1.0 + (a - 1.0) * w0.cos() - 2.0 * a.sqrt() * alpha)) / a0,
        a1: (2.0 * (a - 1.0 - (a + 1.0) * w0.cos())) / a0,
        a2: (a + 1.0 - (a - 1.0) * w0.cos() - 2.0 * a.sqrt() * alpha) / a0,
    }
}

#[inline(always)]
pub(super) fn iir_process(coeffs: &BiquadCoeffs, z1: &mut f32, z2: &mut f32, x: f32) -> f32 {
    let y = coeffs.b0 * x + *z1;
    *z1 = coeffs.b1 * x - coeffs.a1 * y + *z2;
    *z2 = coeffs.b2 * x - coeffs.a2 * y;
    y
}

// ─── Volume Analysis ────────────────────────────────────────────────

pub(super) fn analyze_volume(data: &[f32]) -> VolumeAnalysis {
    let mut peak = 0f32;
    let mut sum_squares = 0f64;

    for &s in data {
        let abs = if s < 0.0 { -s } else { s };
        if abs > peak {
            peak = abs;
        }
        sum_squares += (s as f64) * (s as f64);
    }

    let rms = (sum_squares / data.len() as f64).sqrt() as f32;
    let estimated_lufs = if rms > 0.0 {
        20.0 * (rms / REFERENCE_RMS).log10() - 0.691
    } else {
        -70.0
    };

    let lufs_offset = TARGET_LUFS - estimated_lufs;
    let raw_gain = 10f32.powf(lufs_offset / 20.0);
    let gain_adjustment = raw_gain.clamp(0.1, 3.0);

    VolumeAnalysis {
        peak,
        rms,
        estimated_lufs,
        gain_adjustment,
    }
}

// ─── Energy Analysis ────────────────────────────────────────────────

pub(super) fn analyze_energy(data: &[f32], sample_rate: u32, duration: f32) -> EnergyAnalysis {
    let len = data.len();
    let second_count = duration.ceil() as usize;
    let sample_rate = sample_rate as usize;
    let mut energy_per_second = vec![0f32; second_count];

    for sec in 0..second_count {
        let start = sec * sample_rate;
        let end = ((sec + 1) * sample_rate).min(len);
        let count = end.saturating_sub(start);
        if count == 0 {
            continue;
        }
        let mut sum_sq = 0f64;
        for &s in &data[start..end] {
            sum_sq += (s as f64) * (s as f64);
        }
        energy_per_second[sec] = (sum_sq / count as f64).sqrt() as f32;
    }

    // Trailing silence (100ms windows, absolute threshold)
    let window_samples = (sample_rate as f32 * 0.1) as usize;
    let window_samples = window_samples.min(len);
    let mut trailing_silence = 0f32;

    if window_samples > 0 {
        let mut pos = len.saturating_sub(window_samples);
        loop {
            let win_end = (pos + window_samples).min(len);
            let mut sum_sq = 0f64;
            for &s in &data[pos..win_end] {
                sum_sq += (s as f64) * (s as f64);
            }
            let rms = (sum_sq / (win_end - pos) as f64).sqrt() as f32;
            if rms > SILENCE_THRESHOLD {
                trailing_silence = (len - pos - window_samples) as f32 / sample_rate as f32;
                break;
            }
            if pos < window_samples {
                break;
            }
            pos = pos.saturating_sub(window_samples);
        }
        if trailing_silence == 0.0 && energy_per_second[0] < SILENCE_THRESHOLD {
            trailing_silence = duration;
        }
        trailing_silence = (trailing_silence * 10.0).round() / 10.0;
    }

    // Normalize via 95th percentile (exclude trailing silence)
    let content_seconds = (second_count as f32 - trailing_silence.floor()).max(1.0) as usize;
    let content_seconds = content_seconds.min(second_count);

    let mut sorted = energy_per_second[..content_seconds].to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p95_idx = ((sorted.len() as f32 * 0.95) as usize).min(sorted.len() - 1);
    let norm_factor = sorted[p95_idx].max(0.001);

    for e in &mut energy_per_second {
        *e = (*e / norm_factor).min(1.0);
    }

    // Average energy (content portion)
    let sum: f32 = energy_per_second[..content_seconds].iter().sum();
    let average_energy = sum / content_seconds as f32;

    // Outro detection
    let outro_threshold = average_energy * 0.3;
    let last_content_second = content_seconds.saturating_sub(1).max(0);
    let mut outro_start_offset = 8.0f32;

    let scan_start = last_content_second.saturating_sub(45).max(0);
    for i in (scan_start..=last_content_second).rev() {
        if i < energy_per_second.len() && energy_per_second[i] > outro_threshold {
            outro_start_offset = (last_content_second - i + 1) as f32;
            break;
        }
    }
    outro_start_offset += trailing_silence;
    outro_start_offset = outro_start_offset.max(3.0);

    // Intro detection
    let intro_threshold = average_energy * 0.4;
    let mut intro_end_offset = 2.0f32;
    let scan_len = 30usize.min(second_count);

    for i in 0..scan_len.saturating_sub(1) {
        if energy_per_second[i] >= intro_threshold && energy_per_second[i + 1] >= intro_threshold {
            intro_end_offset = i as f32;
            break;
        }
    }
    intro_end_offset = intro_end_offset.max(0.0).min(10.0);

    // Fade-out detection
    let mut is_fade_out = false;
    let outro_content_duration = outro_start_offset - trailing_silence;

    if outro_content_duration > 5.0 {
        let fade_start_sec =
            (last_content_second as f32 - outro_content_duration).max(0.0) as usize;
        let fade_end_sec = last_content_second.saturating_sub(1).max(0);

        if fade_start_sec < energy_per_second.len() && fade_end_sec < energy_per_second.len() {
            let start_energy = energy_per_second[fade_start_sec];
            let end_energy = energy_per_second[fade_end_sec];

            if start_energy > 0.05 && end_energy / start_energy < 0.3 {
                let mid_sec =
                    ((fade_start_sec + fade_end_sec) / 2).min(energy_per_second.len() - 1);
                let mid_energy = energy_per_second[mid_sec];
                if mid_energy < start_energy * 0.85 && mid_energy > end_energy * 0.8 {
                    is_fade_out = true;
                }
            }
        }
    }

    EnergyAnalysis {
        energy_per_second,
        outro_start_offset,
        intro_end_offset,
        average_energy,
        trailing_silence,
        is_fade_out,
    }
}

// ─── BPM Detection ──────────────────────────────────────────────────

fn downsample(data: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if src_rate == dst_rate {
        return data.to_vec();
    }
    let ratio = src_rate as f32 / dst_rate as f32;
    let out_len = (data.len() as f32 / ratio) as usize;
    let mut out = vec![0f32; out_len];
    for (i, sample) in out.iter_mut().enumerate() {
        *sample = data[(i as f32 * ratio) as usize];
    }
    out
}

fn extract_window(data: &[f32], sample_rate: usize, window_sec: usize) -> (&[f32], f32) {
    let total_samples = data.len();
    let window_samples = (window_sec * sample_rate).min(total_samples);
    let start = (total_samples.saturating_sub(window_samples) / 2).max(0);
    let offset_sec = start as f32 / sample_rate as f32;
    (&data[start..start + window_samples], offset_sec)
}

fn compute_onset_envelope(samples: &[f32], hop_size: usize, frame_size: usize) -> Vec<f32> {
    let len = samples.len();
    if len < frame_size {
        return vec![];
    }

    let frame_count = (len - frame_size) / hop_size;
    if frame_count <= 1 {
        return vec![];
    }

    // Per-frame RMS energy
    let mut energy = vec![0f32; frame_count];
    for f in 0..frame_count {
        let offset = f * hop_size;
        let mut sum_sq = 0f64;
        for &s in &samples[offset..offset + frame_size] {
            sum_sq += (s as f64) * (s as f64);
        }
        energy[f] = (sum_sq / frame_size as f64).sqrt() as f32;
    }

    // Half-wave rectified first difference
    let mut envelope = vec![0f32; frame_count];
    for f in 1..frame_count {
        let diff = energy[f] - energy[f - 1];
        envelope[f] = if diff > 0.0 { diff } else { 0.0 };
    }

    envelope
}

fn detect_bpm_from_envelope(envelope: &[f32], hops_per_second: f32) -> (f32, f32) {
    if envelope.len() < 2 {
        return (120.0, 0.0);
    }

    // Remove DC offset
    let mean: f32 = envelope.iter().sum::<f32>() / envelope.len() as f32;
    let len = envelope.len();

    // Lag range
    let min_lag = ((hops_per_second * 60.0) / MAX_BPM).round() as usize;
    let min_lag = min_lag.max(1);
    let max_lag = ((hops_per_second * 60.0) / MIN_BPM).round() as usize;
    let max_lag = max_lag.min(len - 1);

    // Zero-lag autocorrelation
    let mut zero_corr = 0f64;
    for &e in envelope {
        let c = (e - mean) as f64;
        zero_corr += c * c;
    }
    zero_corr /= len as f64;

    let mut max_corr = -f64::INFINITY;
    let mut best_lag = min_lag;

    for lag in min_lag..=max_lag {
        let n = len - lag;
        let mut sum = 0f64;
        for i in 0..n {
            sum += (envelope[i] - mean) as f64 * (envelope[i + lag] - mean) as f64;
        }
        let corr = sum / n as f64;

        if corr > max_corr {
            max_corr = corr;
            best_lag = lag;
        }
    }

    // Simplified: use best_lag directly. Comb filter refinement requires storing
    // the full correlations array and adds marginally for typical music.
    let bpm = ((hops_per_second * 60.0) / best_lag as f32 * 10.0).round() / 10.0;
    let confidence = if zero_corr > 0.0 {
        (max_corr / zero_corr).max(0.0).min(1.0) as f32
    } else {
        0.0
    };

    (bpm, confidence)
}

pub(super) fn run_bpm_detection(
    data: &[f32],
    sample_rate: u32,
    duration: f32,
) -> Option<BPMResult> {
    if duration < 5.0 {
        return None;
    }

    // Downsample to analysis rate
    let downsampled = downsample(data, sample_rate, BPM_ANALYSIS_RATE);

    // Extract 30s window from middle
    let (samples, offset_sec) = extract_window(
        &downsampled,
        BPM_ANALYSIS_RATE as usize,
        BPM_ANALYSIS_DURATION,
    );

    if samples.is_empty() {
        return None;
    }

    let hop_size = 256usize;
    let frame_size = 1024usize;
    let hops_per_second = BPM_ANALYSIS_RATE as f32 / hop_size as f32;
    let envelope = compute_onset_envelope(samples, hop_size, frame_size);

    if envelope.is_empty() {
        return None;
    }

    let (bpm, confidence) = detect_bpm_from_envelope(&envelope, hops_per_second);
    let window_duration = samples.len() as f32 / BPM_ANALYSIS_RATE as f32;

    // Generate beat grid
    let interval = 60.0 / bpm;
    let mut beat_grid = Vec::new();
    let mut t = 0f32;
    while t < window_duration {
        beat_grid.push(t);
        t += interval;
    }

    Some(BPMResult {
        bpm,
        confidence,
        beat_grid,
        analysis_offset: offset_sec,
    })
}

// ─── Spectral Fingerprint ─────────────────────────────────────────

pub(super) fn compute_fingerprint(data: &[f32], sample_rate: u32) -> SpectralFingerprint {
    let len = data.len();
    let window_size = 2048usize;
    let window_count = (len / window_size).min(8);

    let band_edges: [(f32, f32); 8] = [
        (20.0, 150.0),
        (150.0, 400.0),
        (400.0, 800.0),
        (800.0, 1500.0),
        (1500.0, 3000.0),
        (3000.0, 6000.0),
        (6000.0, 10000.0),
        (10000.0, (sample_rate as f32 / 2.0 - 100.0).min(16000.0)),
    ];

    let band_count = band_edges.len();
    let mut bands = vec![0f32; band_count];

    if window_count == 0 {
        return SpectralFingerprint { bands };
    }

    let sample_rate_f = sample_rate as f32;
    let step = len / window_count;

    for w in 0..window_count {
        let offset = w * step;
        let win_end = (offset + window_size).min(len);
        let w_len = win_end - offset;
        if w_len < window_size / 2 {
            continue;
        }

        for b in 0..band_count {
            let (f_low, f_high) = band_edges[b];
            if f_low >= sample_rate_f / 2.0 {
                continue;
            }
            let clamped_high = (f_high).min(sample_rate_f / 2.0 - 100.0);
            if clamped_high <= f_low {
                continue;
            }

            let coeffs = design_biquad_bandpass(f_low, clamped_high, sample_rate_f);
            let (mut z1, mut z2) = (0f32, 0f32);

            // Warm-up (64 samples)
            let warmup_end = (offset + 64).min(win_end);
            for &s in &data[offset..warmup_end] {
                let y = coeffs.b0 * s + z1;
                z1 = coeffs.b1 * s - coeffs.a1 * y + z2;
                z2 = coeffs.b2 * s - coeffs.a2 * y;
            }

            // Measure RMS
            let mut sum_sq = 0f64;
            for &s in &data[offset..win_end] {
                let y = coeffs.b0 * s + z1;
                z1 = coeffs.b1 * s - coeffs.a1 * y + z2;
                z2 = coeffs.b2 * s - coeffs.a2 * y;
                sum_sq += (y as f64) * (y as f64);
            }
            bands[b] += (sum_sq / w_len as f64).sqrt() as f32;
        }
    }

    // Average and normalize
    for b in &mut bands {
        *b /= window_count as f32;
    }
    let max_b = bands
        .iter()
        .fold(0.0001f32, |acc, &x| if x > acc { x } else { acc });
    for b in &mut bands {
        *b /= max_b;
    }

    SpectralFingerprint { bands }
}
