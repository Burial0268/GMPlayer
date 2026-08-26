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

/// Pick the stream rate for one candidate config range.
///
/// The device's own mix rate wins whenever the range offers it, even when the
/// source rate is also on offer. Opening at the mix rate is the only way to keep
/// the OS out of the conversion business:
///
/// * On Windows, CPAL initializes every *output* stream with
///   `AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY`,
///   and for that reason its enumeration marks every probed format usable for
///   output without ever calling `IsFormatSupported` (`host/wasapi/device.rs`,
///   `let usable = is_output || is_format_supported(..)`). Requesting the source
///   rate therefore always succeeds and always inserts Microsoft's
///   default-quality SRC, replacing our polyphase filter with a worse one — and,
///   because nothing ever fails, giving the caller no way to detect it happened.
/// * On CoreAudio the cost is different but larger: CPAL does *not* leave the
///   device alone. `build_output_stream_raw` calls `set_physical_format` and
///   falls back to `set_sample_rate`, i.e.
///   `AudioObjectSetPropertyData(kAudioDevicePropertyNominalSampleRate)` with a
///   property-listener wait. Chasing the source rate there mutates a
///   system-wide device property — audible to every other app on the machine —
///   and blocks stream construction while the hardware relocks.
///
/// Note the *shape* of the choice this scores: CPAL enumerates one range per
/// rate with `min_sample_rate == max_sample_rate` (both on WASAPI and
/// CoreAudio), so `rate_supported` is a real test and this is a contest
/// *between* ranges, not a tiebreak within one. That is why the returned score
/// has to keep a mix-rate range ahead of a source-rate range by a wide margin
/// (1 vs 50) rather than by a distance-weighted amount — see
/// `select_sample_rate_prefers_a_mix_rate_range_over_a_native_rate_range`.
///
/// Matching the mix rate also keeps the stream config constant across a
/// playlist, so a rate change between tracks never has to be considered as a
/// reason to rebuild the output chain.
fn select_sample_rate(
    range: &cpal::SupportedStreamConfigRange,
    target_rate: u32,
    default_rate: Option<u32>,
) -> (SampleRate, u32) {
    if let Some(default_rate) = default_rate {
        if rate_supported(range, default_rate) {
            // Rate-matched content still scores best, so that among otherwise
            // equal ranges the one needing no conversion at all is chosen.
            return (default_rate, u32::from(default_rate != target_rate));
        }
    }

    // No device mix rate to match (or this range does not cover it): the source
    // rate is the next best thing, since our own resampler is then the only one
    // in the chain.
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
    fn select_sample_rate_prefers_the_device_mix_rate_over_the_source_rate() {
        let range = cpal::SupportedStreamConfigRange::new(
            2,
            44_100,
            48_000,
            SupportedBufferSize::Unknown,
            cpal::SampleFormat::F32,
        );

        // A 44.1 kHz track on a device mixing at 48 kHz. Both rates are on
        // offer, but opening at 44.1 would hand the conversion to the OS.
        let (sample_rate, score) = select_sample_rate(&range, 44_100, Some(48_000));

        assert_eq!(sample_rate, 48_000);
        assert!(score > 0);
    }

    #[test]
    fn select_sample_rate_keeps_the_device_rate_for_distant_sources() {
        let range = cpal::SupportedStreamConfigRange::new(
            2,
            44_100,
            192_000,
            SupportedBufferSize::Unknown,
            cpal::SampleFormat::F32,
        );

        // A 96 kHz source must not drag the endpoint up to 96 kHz just because
        // the gap to the mix rate is large; the device rate is not a proximity
        // contest.
        let (sample_rate, _) = select_sample_rate(&range, 96_000, Some(48_000));

        assert_eq!(sample_rate, 48_000);
    }

    #[test]
    fn select_sample_rate_scores_a_rate_matched_device_best() {
        let range = cpal::SupportedStreamConfigRange::new(
            2,
            44_100,
            48_000,
            SupportedBufferSize::Unknown,
            cpal::SampleFormat::F32,
        );

        let (matched, matched_score) = select_sample_rate(&range, 48_000, Some(48_000));
        let (converted, converted_score) = select_sample_rate(&range, 44_100, Some(48_000));

        assert_eq!((matched, converted), (48_000, 48_000));
        assert!(matched_score < converted_score);
    }

    #[test]
    fn select_sample_rate_falls_back_to_the_source_rate_without_a_device_rate() {
        let range = cpal::SupportedStreamConfigRange::new(
            2,
            44_100,
            48_000,
            SupportedBufferSize::Unknown,
            cpal::SampleFormat::F32,
        );

        // `default_output_config()` failed, or this range does not cover the
        // mix rate: our own resampler is then the only one in the chain, so
        // matching the source is what avoids a conversion.
        let (sample_rate, score) = select_sample_rate(&range, 44_100, None);

        assert_eq!(sample_rate, 44_100);
        assert!(score < 100);
    }

    #[test]
    fn select_sample_rate_prefers_a_mix_rate_range_over_a_native_rate_range() {
        // The ordering the whole policy rests on, and the only one the other
        // tests here cannot see: CPAL enumerates one range per rate with
        // `min == max`, so choosing the mix rate is a contest *between* ranges
        // that `select_output_config`'s `min_by_key` resolves on this score.
        // A device offering {44.1, 48, 96} kHz with a 48 kHz mix rate, playing a
        // 96 kHz source: the 48 kHz range has to win, or the endpoint is
        // retuned per track.
        let fixed = |rate| {
            cpal::SupportedStreamConfigRange::new(
                2,
                rate,
                rate,
                SupportedBufferSize::Unknown,
                cpal::SampleFormat::F32,
            )
        };

        let (mix_rate, mix_score) = select_sample_rate(&fixed(48_000), 96_000, Some(48_000));
        let (native_rate, native_score) = select_sample_rate(&fixed(96_000), 96_000, Some(48_000));

        assert_eq!((mix_rate, native_rate), (48_000, 96_000));
        // Distance-weighting the mix-rate score (an earlier implementation
        // scored it `10 + |default - target| / 100`, i.e. 490 here) loses this
        // contest to the source-rate range's 50 for every source more than
        // 4 kHz from the mix rate — every hi-res track.
        assert!(
            mix_score < native_score,
            "mix-rate range scored {mix_score}, native-rate range {native_score}"
        );
    }
}
