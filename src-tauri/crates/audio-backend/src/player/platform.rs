use crate::output;
use crate::types::AudioInfo;

#[cfg(target_os = "android")]
const START_PREBUFFER_MS: u32 = 160;
#[cfg(target_os = "android")]
const MIN_START_PREBUFFER_FRAMES: usize = 8_192;
#[cfg(not(target_os = "android"))]
const START_PREBUFFER_FRAMES: usize = 512;
#[cfg(target_os = "android")]
pub(super) const START_PREBUFFER_WAIT_MS: u64 = 200;
#[cfg(not(target_os = "android"))]
pub(super) const START_PREBUFFER_WAIT_MS: u64 = 20;

pub(super) fn start_prebuffer_samples(channels: usize, sample_rate: u32) -> usize {
    let channels = channels.max(1);
    channels * start_prebuffer_frames(sample_rate)
}

fn start_prebuffer_frames(sample_rate: u32) -> usize {
    #[cfg(target_os = "android")]
    {
        let frame_ms = sample_rate.max(1) as u64 * START_PREBUFFER_MS as u64;
        let frames = (frame_ms + 999) / 1_000;
        (frames as usize).max(MIN_START_PREBUFFER_FRAMES)
    }

    #[cfg(not(target_os = "android"))]
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
