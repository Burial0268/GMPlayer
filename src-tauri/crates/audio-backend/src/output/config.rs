use cpal::traits::DeviceTrait;
use cpal::{SampleRate, StreamConfig};

use super::{platform, OutputTarget};

const PREFERRED_RATES: [u32; 2] = [48_000, 44_100];
const PREFERRED_FORMATS: [cpal::SampleFormat; 3] = [
    cpal::SampleFormat::F32,
    cpal::SampleFormat::I16,
    cpal::SampleFormat::U16,
];

#[cfg_attr(target_os = "android", allow(dead_code))]
pub(super) fn desired_output_channels(source_channels: u16) -> u16 {
    let source_channels = source_channels.max(1);
    if source_channels <= 2 {
        2
    } else {
        source_channels
    }
}

pub(super) fn select_any_output_config(
    device: &cpal::Device,
) -> Option<cpal::SupportedStreamConfig> {
    let configs = device.supported_output_configs().ok()?;
    configs
        .map(|range| {
            let (sample_rate, rate_score) = select_sample_rate(&range, 48_000, None);
            let score = (format_score(range.sample_format()), rate_score);
            (score, range.with_sample_rate(sample_rate))
        })
        .min_by_key(|(score, _)| *score)
        .map(|(_, config)| config)
}

pub(super) fn stable_stream_config(supported_config: &cpal::SupportedStreamConfig) -> StreamConfig {
    let mut config = supported_config.config();
    config.buffer_size =
        platform::stable_buffer_size(config.sample_rate, supported_config.buffer_size());
    config
}

pub(super) fn select_output_config(
    device: &cpal::Device,
    target: OutputTarget,
    default_config: Option<&cpal::SupportedStreamConfig>,
) -> Option<cpal::SupportedStreamConfig> {
    let default_rate = default_config.map(|config| config.sample_rate());
    let configs = device.supported_output_configs().ok()?;

    configs
        .map(|range| {
            let channels = range.channels();
            let sample_format = range.sample_format();
            let (sample_rate, rate_score) =
                select_sample_rate(&range, target.sample_rate, default_rate);
            let score = (
                channel_score(channels, target.channels),
                rate_score,
                format_score(sample_format),
            );
            (score, range.with_sample_rate(sample_rate))
        })
        .min_by_key(|(score, _)| *score)
        .map(|(_, config)| config)
}

fn select_sample_rate(
    range: &cpal::SupportedStreamConfigRange,
    target_rate: u32,
    default_rate: Option<u32>,
) -> (SampleRate, u32) {
    if let Some(default_rate) = default_rate {
        if rate_supported(range, default_rate) {
            let score = if default_rate == target_rate {
                0
            } else {
                10 + default_rate.abs_diff(target_rate) / 100
            };
            return (default_rate, score);
        }
    }

    if rate_supported(range, target_rate) {
        return (target_rate, 50);
    }

    for preferred_rate in PREFERRED_RATES {
        if rate_supported(range, preferred_rate) {
            return (
                preferred_rate,
                100 + preferred_rate.abs_diff(target_rate) / 100,
            );
        }
    }

    let min = range.min_sample_rate();
    let max = range.max_sample_rate();
    let clamped = target_rate.clamp(min, max);
    (clamped, 1_000 + clamped.abs_diff(target_rate) / 100)
}

fn rate_supported(range: &cpal::SupportedStreamConfigRange, rate: u32) -> bool {
    range.min_sample_rate() <= rate && rate <= range.max_sample_rate()
}

fn channel_score(channels: u16, target_channels: u16) -> u32 {
    if channels == target_channels {
        return 0;
    }
    if target_channels > 2 && channels > target_channels {
        return 10 + u32::from(channels - target_channels);
    }
    if channels > target_channels {
        return 50 + u32::from(channels - target_channels);
    }
    100 + u32::from(target_channels - channels)
}

fn format_score(format: cpal::SampleFormat) -> u32 {
    PREFERRED_FORMATS
        .iter()
        .position(|preferred| *preferred == format)
        .map(|index| index as u32)
        .unwrap_or(10)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpal::SupportedBufferSize;

    #[test]
    fn desired_channels_keep_stereo_for_music_sources() {
        assert_eq!(desired_output_channels(1), 2);
        assert_eq!(desired_output_channels(2), 2);
        assert_eq!(desired_output_channels(6), 6);
    }

    #[test]
    fn channel_score_prefers_exact_then_wider_for_multichannel() {
        assert!(channel_score(6, 6) < channel_score(8, 6));
        assert!(channel_score(8, 6) < channel_score(2, 6));
        assert!(channel_score(2, 2) < channel_score(6, 2));
    }

    #[test]
    fn select_sample_rate_prefers_default_mix_rate_over_source_rate() {
        let range = cpal::SupportedStreamConfigRange::new(
            2,
            44_100,
            48_000,
            SupportedBufferSize::Unknown,
            cpal::SampleFormat::F32,
        );

        let (sample_rate, score) = select_sample_rate(&range, 44_100, Some(48_000));

        assert_eq!(sample_rate, 48_000);
        assert!(score > 0);
    }
}
