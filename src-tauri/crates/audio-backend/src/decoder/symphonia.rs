use std::fs::File;
use std::path::Path;

use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::error::{AudioError, AudioResult};
use crate::types::AudioInfo;

/// Extract metadata only — the heavy lifting of decoding is handled
/// by rodio::Decoder.  This just opens the file, reads header info,
/// and returns an AudioInfo struct.
pub fn extract_metadata_only(path: &Path) -> AudioResult<AudioInfo> {
  let file = File::open(path)?;
  let mss = MediaSourceStream::new(Box::new(file), Default::default());

  let mut hint = Hint::new();
  if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
    hint.with_extension(ext);
  }

  let format_opts = FormatOptions::default();
  let metadata_opts = MetadataOptions::default();

  let mut probed = symphonia::default::get_probe()
    .format(&hint, mss, &format_opts, &metadata_opts)
    .map_err(|e| AudioError::Decode(e.to_string()))?;

  let format = &mut probed.format;
  let track = format
    .default_track()
    .ok_or(AudioError::NoAudioTrack)?;

  let codec_params = &track.codec_params;
  let sample_rate = codec_params.sample_rate.unwrap_or(44100);
  let channels = codec_params
    .channels
    .map(|c| c.count() as u16)
    .unwrap_or(2);
  let total_frames = codec_params.n_frames;
  let time_base = codec_params
    .time_base
    .unwrap_or(symphonia::core::units::TimeBase::new(1, sample_rate));

  let codec_name = format!("{:?}", codec_params.codec).to_lowercase();

  let duration_secs = total_frames
    .map(|f| f as f64 * time_base.numer as f64 / time_base.denom as f64)
    .unwrap_or(0.0);

  let container_format = path
    .extension()
    .and_then(|e| e.to_str())
    .unwrap_or("unknown")
    .to_string();

  let mut metadata_tags = Vec::new();
  if let Some(reader) = format.metadata().current() {
    for tag in reader.tags() {
      metadata_tags.push((tag.key.clone(), tag.value.to_string()));
    }
  }

  Ok(AudioInfo {
    codec: codec_name,
    sample_rate,
    channels,
    duration_secs,
    bitrate_bps: None,
    total_frames,
    container_format,
    metadata_tags,
  })
}
