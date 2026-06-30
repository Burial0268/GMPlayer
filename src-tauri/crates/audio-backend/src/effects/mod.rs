use crate::types::{DspConfig, EqualizerBand, EqualizerConfig, EqualizerFilterType, LimiterConfig};

const MAX_EQ_BANDS: usize = 64;
const MIN_FILTER_FREQ: f32 = 10.0;
const MIN_Q: f32 = 0.1;
const MAX_Q: f32 = 18.0;
const MAX_GAIN_DB: f32 = 24.0;
const MIN_GAIN_DB: f32 = -48.0;

pub(crate) struct DspChain {
    sample_rate: f32,
    channels: usize,
    input_gain: f32,
    output_gain: f32,
    bands: Vec<EqBandRuntime>,
    limiter: Option<LimiterRuntime>,
}

impl DspChain {
    pub(crate) fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_rate: sample_rate.max(1) as f32,
            channels: channels.max(1) as usize,
            input_gain: 1.0,
            output_gain: 1.0,
            bands: Vec::new(),
            limiter: None,
        }
    }

    pub(crate) fn set_dsp(&mut self, config: &DspConfig) {
        self.bands.clear();
        self.limiter = None;

        if !config.enabled {
            self.input_gain = 1.0;
            self.output_gain = 1.0;
            return;
        }

        let eq_preamp_db = if config.equalizer.enabled {
            config.equalizer.preamp_db
        } else {
            0.0
        };
        self.input_gain =
            db_to_gain((config.input_gain_db + eq_preamp_db).clamp(MIN_GAIN_DB, MAX_GAIN_DB));
        self.output_gain = db_to_gain(config.output_gain_db.clamp(MIN_GAIN_DB, MAX_GAIN_DB));

        if config.equalizer.enabled {
            self.configure_bands(&config.equalizer);
        }

        if config.limiter.enabled {
            self.limiter = Some(LimiterRuntime::new(config.limiter, self.sample_rate));
        }

        self.normalize_bypass_gains();
    }

    fn configure_bands(&mut self, config: &EqualizerConfig) {
        let nyquist = self.sample_rate * 0.5;
        let max_freq = (nyquist * 0.95).max(MIN_FILTER_FREQ);

        for band in config.bands.iter().take(MAX_EQ_BANDS) {
            if let Some(runtime) = self.create_band_runtime(*band, max_freq) {
                self.bands.push(runtime);
            }
        }
    }

    fn create_band_runtime(&self, band: EqualizerBand, max_freq: f32) -> Option<EqBandRuntime> {
        if !band.enabled {
            return None;
        }

        let gain_db = band.gain_db.clamp(-MAX_GAIN_DB, MAX_GAIN_DB);
        if gain_db.abs() < 0.001 {
            return None;
        }

        let frequency = band.frequency.clamp(MIN_FILTER_FREQ, max_freq);
        let q = band.q.clamp(MIN_Q, MAX_Q);
        let coeffs = match band.filter_type {
            EqualizerFilterType::Peaking => {
                BiquadCoeffs::peaking(self.sample_rate, frequency, q, gain_db)
            }
            EqualizerFilterType::LowShelf => {
                BiquadCoeffs::low_shelf(self.sample_rate, frequency, q, gain_db)
            }
            EqualizerFilterType::HighShelf => {
                BiquadCoeffs::high_shelf(self.sample_rate, frequency, q, gain_db)
            }
        };

        Some(EqBandRuntime::new(coeffs, self.channels))
    }

    fn normalize_bypass_gains(&mut self) {
        if (self.input_gain - 1.0).abs() < 0.000_001 {
            self.input_gain = 1.0;
        }
        if (self.output_gain - 1.0).abs() < 0.000_001 {
            self.output_gain = 1.0;
        }
    }

    #[inline]
    pub(crate) fn is_bypassed(&self) -> bool {
        self.input_gain == 1.0
            && self.output_gain == 1.0
            && self.bands.is_empty()
            && self.limiter.is_none()
    }

    #[inline]
    pub(crate) fn process_interleaved(&mut self, samples: &mut [f32]) {
        if self.is_bypassed() {
            return;
        }

        if self.input_gain != 1.0 {
            apply_gain(samples, self.input_gain);
        }

        for band in &mut self.bands {
            band.process_interleaved(samples, self.channels);
        }

        if self.output_gain != 1.0 {
            apply_gain(samples, self.output_gain);
        }

        if let Some(limiter) = &mut self.limiter {
            limiter.process_interleaved(samples, self.channels);
        }
    }
}

#[inline]
fn apply_gain(samples: &mut [f32], gain: f32) {
    for sample in samples {
        *sample *= gain;
    }
}

struct EqBandRuntime {
    coeffs: BiquadCoeffs,
    states: Vec<BiquadState>,
}

impl EqBandRuntime {
    fn new(coeffs: BiquadCoeffs, channels: usize) -> Self {
        Self {
            coeffs,
            states: vec![BiquadState::default(); channels],
        }
    }

    #[inline]
    fn process_interleaved(&mut self, samples: &mut [f32], channels: usize) {
        for frame in samples.chunks_exact_mut(channels) {
            for (sample, state) in frame.iter_mut().zip(&mut self.states) {
                *sample = state.process(self.coeffs, *sample);
            }
        }
    }
}

struct LimiterRuntime {
    threshold_gain: f32,
    ceiling_gain: f32,
    release_coeff: f32,
    gain: f32,
}

impl LimiterRuntime {
    fn new(config: LimiterConfig, sample_rate: f32) -> Self {
        let ceiling_db = config.ceiling_db.clamp(MIN_GAIN_DB, 0.0);
        let threshold_db = config.threshold_db.clamp(MIN_GAIN_DB, ceiling_db);
        let release_ms = config.release_ms.clamp(5.0, 2_000.0);
        let release_samples = (release_ms * 0.001 * sample_rate.max(1.0)).max(1.0);
        let release_coeff = 1.0 - (-1.0 / release_samples).exp();

        Self {
            threshold_gain: db_to_gain(threshold_db),
            ceiling_gain: db_to_gain(ceiling_db),
            release_coeff,
            gain: 1.0,
        }
    }

    #[inline]
    fn process_interleaved(&mut self, samples: &mut [f32], channels: usize) {
        for frame in samples.chunks_exact_mut(channels) {
            let mut peak = 0.0f32;
            for sample in frame.iter() {
                peak = peak.max(sample.abs());
            }

            let target_gain = if peak > self.threshold_gain && peak > 0.0 {
                (self.ceiling_gain / peak).min(1.0)
            } else {
                1.0
            };

            if target_gain < self.gain {
                self.gain = target_gain;
            } else {
                self.gain += (1.0 - self.gain) * self.release_coeff;
            }

            if self.gain != 1.0 {
                for sample in frame {
                    *sample *= self.gain;
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct BiquadCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl BiquadCoeffs {
    fn peaking(sample_rate: f32, frequency: f32, q: f32, gain_db: f32) -> Self {
        let a = db_to_gain(gain_db).sqrt();
        let omega = std::f32::consts::TAU * frequency / sample_rate;
        let (sin, cos) = omega.sin_cos();
        let alpha = sin / (2.0 * q);

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos;
        let a2 = 1.0 - alpha / a;

        Self::normalize(b0, b1, b2, a0, a1, a2)
    }

    fn low_shelf(sample_rate: f32, frequency: f32, q: f32, gain_db: f32) -> Self {
        let a = db_to_gain(gain_db).sqrt();
        let omega = std::f32::consts::TAU * frequency / sample_rate;
        let (sin, cos) = omega.sin_cos();
        let beta = a.sqrt() / q;

        let b0 = a * ((a + 1.0) - (a - 1.0) * cos + beta * sin);
        let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos);
        let b2 = a * ((a + 1.0) - (a - 1.0) * cos - beta * sin);
        let a0 = (a + 1.0) + (a - 1.0) * cos + beta * sin;
        let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos);
        let a2 = (a + 1.0) + (a - 1.0) * cos - beta * sin;

        Self::normalize(b0, b1, b2, a0, a1, a2)
    }

    fn high_shelf(sample_rate: f32, frequency: f32, q: f32, gain_db: f32) -> Self {
        let a = db_to_gain(gain_db).sqrt();
        let omega = std::f32::consts::TAU * frequency / sample_rate;
        let (sin, cos) = omega.sin_cos();
        let beta = a.sqrt() / q;

        let b0 = a * ((a + 1.0) + (a - 1.0) * cos + beta * sin);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos);
        let b2 = a * ((a + 1.0) + (a - 1.0) * cos - beta * sin);
        let a0 = (a + 1.0) - (a - 1.0) * cos + beta * sin;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos);
        let a2 = (a + 1.0) - (a - 1.0) * cos - beta * sin;

        Self::normalize(b0, b1, b2, a0, a1, a2)
    }

    fn normalize(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        let inv_a0 = if a0.abs() > f32::EPSILON {
            1.0 / a0
        } else {
            1.0
        };
        Self {
            b0: b0 * inv_a0,
            b1: b1 * inv_a0,
            b2: b2 * inv_a0,
            a1: a1 * inv_a0,
            a2: a2 * inv_a0,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct BiquadState {
    z1: f32,
    z2: f32,
}

impl BiquadState {
    #[inline]
    fn process(&mut self, coeffs: BiquadCoeffs, input: f32) -> f32 {
        let output = coeffs.b0 * input + self.z1;
        self.z1 = coeffs.b1 * input - coeffs.a1 * output + self.z2;
        self.z2 = coeffs.b2 * input - coeffs.a2 * output;
        output
    }
}

#[inline]
fn db_to_gain(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EqualizerBand;

    #[test]
    fn default_chain_is_bypassed() {
        let chain = DspChain::new(48_000, 2);
        assert!(chain.is_bypassed());
    }

    #[test]
    fn disabled_dsp_clears_runtime() {
        let mut chain = DspChain::new(48_000, 2);
        chain.set_dsp(&DspConfig {
            enabled: true,
            input_gain_db: 3.0,
            equalizer: equalizer_with_band(6.0),
            output_gain_db: -1.0,
            limiter: LimiterConfig {
                enabled: true,
                ..LimiterConfig::default()
            },
        });
        assert!(!chain.is_bypassed());

        chain.set_dsp(&DspConfig::default());

        assert!(chain.is_bypassed());
    }

    #[test]
    fn gain_stages_process_in_place() {
        let mut chain = DspChain::new(48_000, 2);
        chain.set_dsp(&DspConfig {
            enabled: true,
            input_gain_db: -6.0,
            equalizer: EqualizerConfig::default(),
            output_gain_db: 0.0,
            limiter: LimiterConfig::default(),
        });
        let mut samples = [1.0, -0.5, 0.25, -0.25];

        chain.process_interleaved(&mut samples);

        assert!(samples[0] < 0.502 && samples[0] > 0.500);
        assert!(samples[1] > -0.251 && samples[1] < -0.250);
    }

    #[test]
    fn peaking_band_keeps_samples_finite() {
        let mut chain = DspChain::new(48_000, 2);
        chain.set_dsp(&DspConfig {
            enabled: true,
            input_gain_db: 0.0,
            equalizer: equalizer_with_band(6.0),
            output_gain_db: 0.0,
            limiter: LimiterConfig::default(),
        });
        let mut samples = [0.1; 128];

        chain.process_interleaved(&mut samples);

        assert!(samples.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn limiter_caps_peak_without_nan() {
        let mut chain = DspChain::new(48_000, 2);
        chain.set_dsp(&DspConfig {
            enabled: true,
            input_gain_db: 0.0,
            equalizer: EqualizerConfig::default(),
            output_gain_db: 0.0,
            limiter: LimiterConfig {
                enabled: true,
                threshold_db: -6.0,
                ceiling_db: -6.0,
                release_ms: 50.0,
            },
        });
        let mut samples = [1.0, -1.0, 0.25, -0.25];

        chain.process_interleaved(&mut samples);

        assert!(samples.iter().all(|sample| sample.is_finite()));
        assert!(samples[0].abs() <= 0.502);
        assert!(samples[1].abs() <= 0.502);
    }

    fn equalizer_with_band(gain_db: f32) -> EqualizerConfig {
        EqualizerConfig {
            enabled: true,
            preamp_db: 0.0,
            bands: vec![EqualizerBand {
                enabled: true,
                filter_type: EqualizerFilterType::Peaking,
                frequency: 1_000.0,
                gain_db,
                q: 0.707,
            }],
        }
    }
}
