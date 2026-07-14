use crate::output;
use crate::types::AudioInfo;

// Android and Linux share the latency-tolerant prebuffer profile: both sit
// behind schedulers (Android audio HAL, Pulse/PipeWire server) that punish
// underruns with added sink latency, so playback starts/seeks wait for a
// deeper PCM watermark before unpausing the callback. The remaining desktop
// platforms keep the near-instant start.
#[cfg(any(target_os = "android", target_os = "linux"))]
const START_PREBUFFER_MS: u32 = 160;
#[cfg(any(target_os = "android", target_os = "linux"))]
const SEEK_PREBUFFER_MS: u32 = 48;
#[cfg(any(target_os = "android", target_os = "linux"))]
const MIN_START_PREBUFFER_FRAMES: usize = 8_192;
#[cfg(any(target_os = "android", target_os = "linux"))]
const MIN_SEEK_PREBUFFER_FRAMES: usize = 2_048;
#[cfg(not(any(target_os = "android", target_os = "linux")))]
const START_PREBUFFER_FRAMES: usize = 512;
#[cfg(any(target_os = "android", target_os = "linux"))]
pub(super) const START_PREBUFFER_WAIT_MS: u64 = 200;
#[cfg(not(any(target_os = "android", target_os = "linux")))]
pub(super) const START_PREBUFFER_WAIT_MS: u64 = 20;
#[cfg(any(target_os = "android", target_os = "linux"))]
pub(super) const SEEK_PREBUFFER_WAIT_MS: u64 = 80;
#[cfg(not(any(target_os = "android", target_os = "linux")))]
pub(super) const SEEK_PREBUFFER_WAIT_MS: u64 = 20;

pub(super) fn start_prebuffer_samples(channels: usize, sample_rate: u32) -> usize {
    let channels = channels.max(1);
    channels * start_prebuffer_frames(sample_rate)
}

pub(super) fn seek_prebuffer_samples(channels: usize, sample_rate: u32) -> usize {
    let channels = channels.max(1);
    channels * seek_prebuffer_frames(sample_rate)
}

fn start_prebuffer_frames(sample_rate: u32) -> usize {
    #[cfg(any(target_os = "android", target_os = "linux"))]
    {
        let frame_ms = sample_rate.max(1) as u64 * START_PREBUFFER_MS as u64;
        let frames = (frame_ms + 999) / 1_000;
        (frames as usize).max(MIN_START_PREBUFFER_FRAMES)
    }

    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    {
        let _ = sample_rate;
        START_PREBUFFER_FRAMES
    }
}

fn seek_prebuffer_frames(sample_rate: u32) -> usize {
    #[cfg(any(target_os = "android", target_os = "linux"))]
    {
        let frame_ms = sample_rate.max(1) as u64 * SEEK_PREBUFFER_MS as u64;
        let frames = (frame_ms + 999) / 1_000;
        (frames as usize).max(MIN_SEEK_PREBUFFER_FRAMES)
    }

    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    {
        let _ = sample_rate;
        START_PREBUFFER_FRAMES
    }
}

// Android and Linux keep one device-native stream and resample into it. Android
// avoids mobile output churn; Linux avoids Pulse/PipeWire/ALSA edge cases around
// per-track stream layouts. Windows/macOS keep the source-aware channel target.
pub(super) fn output_target_for_source(audio_info: &AudioInfo) -> Option<output::OutputTarget> {
    #[cfg(any(target_os = "android", target_os = "linux"))]
    {
        let _ = audio_info;
        None
    }

    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    {
        Some(output::OutputTarget::for_source(
            audio_info.channels,
            audio_info.sample_rate,
        ))
    }
}

pub(super) fn output_refresh_target(
    config: output::OutputConfigKey,
) -> Option<output::OutputTarget> {
    #[cfg(any(target_os = "android", target_os = "linux"))]
    {
        let _ = config;
        None
    }

    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    {
        Some(output::OutputTarget {
            channels: config.channels,
            sample_rate: config.sample_rate,
        })
    }
}

pub(super) fn output_target_matches(
    current: Option<output::OutputTarget>,
    requested: Option<output::OutputTarget>,
) -> bool {
    match (current, requested) {
        (Some(current), Some(requested)) => current.channels == requested.channels,
        (None, None) => true,
        _ => false,
    }
}
