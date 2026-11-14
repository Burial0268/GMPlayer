use std::path::Path;

use crate::error::AudioResult;
use crate::source::{self, SourceLocator};
use crate::types::AudioInfo;

use super::{estimate_bitrate_bps, parse_leading_number, parse_year, CoverArt, TrackTags};

pub(super) fn extract_track_tags(
    path: &Path,
    want_cover: bool,
) -> AudioResult<(AudioInfo, TrackTags, bool)> {
    let opened = source::open(SourceLocator::from_path(path))?;
    let file_size = opened.size();
    let extension = opened.extension().unwrap_or("unknown").to_string();
    let needs_cache = opened.was_spooled();
    let reader = crate::decoder::ffmpeg::reader(opened)?;
    let (channels, sample_rate) = crate::decoder::ffmpeg::pcm_format(&reader)?;
    let source_info = reader.source_info();
    let duration_secs = reader
        .duration()
        .map_or(0.0, |duration| duration.as_secs_f64());
    let metadata = reader.metadata();
    let mut tags = TrackTags::default();
    for (key, value) in &metadata {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.to_ascii_lowercase().as_str() {
            "title" => tags.title = Some(value.to_string()),
            "artist" | "author" => tags.artist = Some(value.to_string()),
            "album" => tags.album = Some(value.to_string()),
            "album_artist" | "albumartist" => tags.album_artist = Some(value.to_string()),
            "genre" => tags.genre = Some(value.to_string()),
            "track" | "tracknumber" => tags.track_no = parse_leading_number(value),
            "disc" | "discnumber" => tags.disc_no = parse_leading_number(value),
            "date" | "year" => tags.year = tags.year.or_else(|| parse_year(value)),
            key if key == "lyrics" || key.starts_with("lyrics-") || key == "unsyncedlyrics" => {
                tags.lyrics = Some(value.to_string());
            }
            _ => {}
        }
    }
    if want_cover {
        tags.cover = reader.cover().map(|cover| CoverArt {
            media_type: cover
                .mime_type
                .unwrap_or_else(|| "application/octet-stream".into()),
            data: cover.data,
        });
    }
    let info = AudioInfo {
        codec: source_info
            .codec_name
            .clone()
            .unwrap_or_else(|| extension.clone()),
        sample_rate: sample_rate.get(),
        channels: channels.get(),
        duration_secs,
        bitrate_bps: u64::try_from(source_info.bit_rate)
            .ok()
            .filter(|rate| *rate > 0)
            .or_else(|| estimate_bitrate_bps(file_size, duration_secs)),
        // FFmpeg reports a codec frame count, not a PCM sample count. Do not
        // reinterpret it as the n_frames field used by the rest of the player.
        total_frames: None,
        container_format: extension,
        metadata_tags: metadata.into_iter().collect(),
    };
    Ok((info, tags, needs_cache))
}
