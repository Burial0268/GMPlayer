use std::fs::File;
use std::path::Path;

use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::error::{AudioError, AudioResult};
use crate::types::AudioInfo;

pub fn extract_metadata_only(path: &Path) -> AudioResult<AudioInfo> {
    let file_size = std::fs::metadata(path).ok().map(|metadata| metadata.len());
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let mut probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| AudioError::Decode(e.to_string()))?;

    let format = &mut probed.format;
    let track = format.default_track().ok_or(AudioError::NoAudioTrack)?;
    let codec_params = &track.codec_params;

    let sample_rate = codec_params.sample_rate.unwrap_or(44_100).max(1);
    let channels = codec_params.channels.map(|c| c.count() as u16).unwrap_or(2);
    let total_frames = codec_params.n_frames;
    let duration_secs = total_frames
        .map(|frames| {
            if let Some(time_base) = codec_params.time_base {
                frames as f64 * time_base.numer as f64 / time_base.denom as f64
            } else {
                frames as f64 / sample_rate as f64
            }
        })
        .unwrap_or(0.0);

    let codec = format!("{:?}", codec_params.codec).to_lowercase();
    let container_format = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("unknown")
        .to_string();
    let bitrate_bps = estimate_bitrate_bps(file_size, duration_secs);

    let mut metadata_tags = Vec::new();
    if let Some(metadata) = format.metadata().current() {
        for tag in metadata.tags() {
            metadata_tags.push((tag.key.clone(), tag.value.to_string()));
        }
    }

    Ok(AudioInfo {
        codec,
        sample_rate,
        channels,
        duration_secs,
        bitrate_bps,
        total_frames,
        container_format,
        metadata_tags,
    })
}

fn estimate_bitrate_bps(file_size: Option<u64>, duration_secs: f64) -> Option<u64> {
    if duration_secs <= 0.0 || !duration_secs.is_finite() {
        return None;
    }

    file_size
        .and_then(|bytes| bytes.checked_mul(8))
        .map(|bits| (bits as f64 / duration_secs).round() as u64)
        .filter(|bitrate| *bitrate > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_bitrate_from_file_size_and_duration() {
        assert_eq!(estimate_bitrate_bps(Some(320_000), 10.0), Some(256_000));
    }

    #[test]
    fn estimate_bitrate_rejects_missing_duration() {
        assert_eq!(estimate_bitrate_bps(Some(320_000), 0.0), None);
        assert_eq!(estimate_bitrate_bps(Some(320_000), f64::NAN), None);
    }
}
