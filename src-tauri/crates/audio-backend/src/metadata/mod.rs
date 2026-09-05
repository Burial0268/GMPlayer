use std::path::Path;

use symphonia::core::codecs::{CodecType, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::error::{AudioError, AudioResult};
use crate::source::{self, SourceLocator};
use crate::types::AudioInfo;

pub fn extract_metadata_only(path: &Path) -> AudioResult<AudioInfo> {
    let opened = source::open(SourceLocator::from_path(path))?;
    // Not `std::fs::metadata`: a `content://` URI has no path to stat, and the
    // opened source can answer for itself.
    let file_size = opened.size();
    let extension = opened.extension().map(str::to_string);

    let mut hint = Hint::new();
    if let Some(ext) = extension.as_deref() {
        hint.with_extension(ext);
    }
    let mss = MediaSourceStream::new(Box::new(opened), Default::default());

    let mut probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| AudioError::Decode(e.to_string()))?;

    let format = &mut probed.format;
    audio_info_from_reader(format.as_mut(), file_size, extension)
}

/// Read the technical fields out of an already-probed reader.
///
/// Shared by [`extract_metadata_only`] and [`extract_track_tags`] so the two can
/// never disagree about a duration or a bitrate — the player and the library
/// showing different lengths for the same file is the kind of thing nobody
/// reports as a bug, they just stop trusting the numbers.
fn audio_info_from_reader(
    format: &mut dyn symphonia::core::formats::FormatReader,
    file_size: Option<u64>,
    extension: Option<String>,
) -> AudioResult<AudioInfo> {
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

    let codec = codec_name(codec_params.codec, extension.as_deref());
    let container_format = extension.unwrap_or_else(|| "unknown".to_string());
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

/// A human-readable name for the decoded codec.
///
/// `CodecType` is a newtype over an id and its `Debug` prints `CodecType(4099)`,
/// which is what the library showed as the codec of every MP3 (`0x1003`). The
/// names live in Symphonia's own codec registry, so ask that rather than keeping a
/// table here: the registry is built from the same feature set that decides what
/// this build can decode at all, so the two cannot drift apart.
///
/// It lists only codecs whose *decoder* is registered, though, and a container we
/// can parse may hold one we cannot — an M4A of AC-3 parses through `isomp4`.
/// Those fall back to the container extension, which is the more useful half of
/// the answer anyway: nothing in the UI shows the container separately, and a name
/// beats an id nobody can look up.
fn codec_name(codec: CodecType, extension: Option<&str>) -> String {
    if codec != CODEC_TYPE_NULL {
        if let Some(descriptor) = symphonia::default::get_codecs().get_codec(codec) {
            // Already lowercase upstream; normalised anyway because the frontend
            // upper-cases it for display and expects one convention here.
            return descriptor.short_name.to_ascii_lowercase();
        }
        tracing::debug!(
            "no registered decoder names {codec:?}; falling back to the container"
        );
    }
    extension
        .map(str::to_ascii_lowercase)
        .filter(|ext| !ext.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
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

// ── Library-scan tag extraction ──────────────────────────────────

/// Embedded cover art, exactly as the container stored it.
///
/// Not decoded or re-encoded here: doing so would mean an image codec in the
/// audio backend, and the two consumers (an `<img>` in the WebView, Android's
/// `BitmapFactory`) both decode it themselves anyway.
#[derive(Debug, Clone)]
pub struct CoverArt {
    /// MIME type as the container declared it, e.g. `image/jpeg`.
    pub media_type: String,
    pub data: Vec<u8>,
}

/// Everything the local library needs from one file, in one probe.
///
/// Separate from [`extract_metadata_only`] because that one is on the playback
/// path and deliberately cheap: it does not want cover bytes, and the player
/// takes its display strings from the manifest. The scanner wants the opposite.
#[derive(Debug, Clone, Default)]
pub struct TrackTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<i32>,
    /// Unsynchronised lyrics from the tag, when present. The first thing
    /// `local_lyric_for` tries, before a sibling `.lrc`.
    pub lyrics: Option<String>,
    pub cover: Option<CoverArt>,
}

/// Probe `path` for tags and (optionally) cover art, alongside the same
/// technical fields [`extract_metadata_only`] reports.
///
/// Goes through `source::open`, so a `content://` document is read from its file
/// descriptor like any other file — which is the only reason an Android SAF
/// track can show a title instead of its filename.
pub fn extract_track_tags(
    path: &Path,
    want_cover: bool,
) -> AudioResult<(AudioInfo, TrackTags, bool)> {
    let locator = SourceLocator::from_path(path);
    let opened = source::open(locator)?;
    let file_size = opened.size();
    let extension = opened.extension().map(str::to_string);
    // A source that had to be spooled is one the player will have to spool
    // again; the UI says so rather than letting it look like an ordinary file.
    let needs_cache = opened.was_spooled();

    let mut hint = Hint::new();
    if let Some(ext) = extension.as_deref() {
        hint.with_extension(ext);
    }
    let mss = MediaSourceStream::new(Box::new(opened), Default::default());

    let mut probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| AudioError::Decode(e.to_string()))?;

    let mut tags = TrackTags::default();
    // Metadata found *before* the container was identified (an ID3v2 block in
    // front of a FLAC stream, ID3v1 at the end of an MP3) lands in the probe's
    // own log rather than in the reader, so both have to be consulted or a large
    // share of real-world files come back untitled. The reader's copy is applied
    // second so in-container tags win.
    if let Some(metadata) = probed.metadata.get().as_ref().and_then(|m| m.current()) {
        absorb_revision(&mut tags, metadata, want_cover);
    }
    if let Some(metadata) = probed.format.metadata().current() {
        absorb_revision(&mut tags, metadata, want_cover);
    }

    let info = audio_info_from_reader(probed.format.as_mut(), file_size, extension)?;
    Ok((info, tags, needs_cache))
}

fn absorb_revision(
    tags: &mut TrackTags,
    revision: &symphonia::core::meta::MetadataRevision,
    want_cover: bool,
) {
    use symphonia::core::meta::StandardTagKey as Key;

    for tag in revision.tags() {
        let value = tag.value.to_string().trim().to_string();
        if value.is_empty() {
            continue;
        }
        match tag.std_key {
            Some(Key::TrackTitle) => tags.title = Some(value),
            Some(Key::Artist) => tags.artist = Some(value),
            Some(Key::Album) => tags.album = Some(value),
            Some(Key::AlbumArtist) => tags.album_artist = Some(value),
            Some(Key::Genre) => tags.genre = Some(value),
            Some(Key::TrackNumber) => tags.track_no = parse_leading_number(&value),
            Some(Key::DiscNumber) => tags.disc_no = parse_leading_number(&value),
            Some(Key::Date) | Some(Key::ReleaseDate) | Some(Key::OriginalDate) => {
                if tags.year.is_none() {
                    tags.year = parse_year(&value);
                }
            }
            Some(Key::Lyrics) => tags.lyrics = Some(value),
            _ => {}
        }
    }

    if !want_cover || tags.cover.is_some() {
        return;
    }
    // First visual wins. Containers routinely carry a front cover plus a back
    // cover and an artist photo, and picking through `usage` is not worth it:
    // encoders put the front cover first in overwhelming practice.
    if let Some(visual) = revision.visuals().first() {
        tags.cover = Some(CoverArt {
            media_type: visual.media_type.clone(),
            data: visual.data.to_vec(),
        });
    }
}

/// `"3/12"`, `"03"`, `"3"` → `3`.
fn parse_leading_number(value: &str) -> Option<u32> {
    let digits: String = value
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// `"1997"`, `"1997-08-15"`, `"15/08/1997"` → `1997`.
fn parse_year(value: &str) -> Option<i32> {
    let trimmed = value.trim();
    if let Ok(year) = trimmed.parse::<i32>() {
        if (1000..=9999).contains(&year) {
            return Some(year);
        }
    }
    trimmed
        .split(['-', '/', '.', ' ', 'T'])
        .find_map(|part| part.parse::<i32>().ok().filter(|y| (1000..=9999).contains(y)))
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

    /// The bug this replaced: `CodecType`'s `Debug` is `CodecType(4099)`, so every
    /// MP3 in the library reported its codec as `codectype(4099)`.
    #[test]
    fn codec_names_come_out_readable() {
        use symphonia::core::codecs::{
            CODEC_TYPE_AAC, CODEC_TYPE_ALAC, CODEC_TYPE_FLAC, CODEC_TYPE_MP3, CODEC_TYPE_VORBIS,
        };

        assert_eq!(codec_name(CODEC_TYPE_MP3, Some("mp3")), "mp3");
        assert_eq!(codec_name(CODEC_TYPE_FLAC, Some("flac")), "flac");
        assert_eq!(codec_name(CODEC_TYPE_AAC, Some("m4a")), "aac");
        assert_eq!(codec_name(CODEC_TYPE_ALAC, Some("m4a")), "alac");
        assert_eq!(codec_name(CODEC_TYPE_VORBIS, Some("ogg")), "vorbis");
        // Whatever comes out, it must never be the raw id again.
        for codec in [CODEC_TYPE_MP3, CODEC_TYPE_FLAC, CODEC_TYPE_AAC] {
            assert!(!codec_name(codec, None).contains("codectype"));
        }
    }

    /// A container we can parse may hold a codec we cannot decode — an M4A of AC-3
    /// parses through `isomp4`. Naming the container beats naming an id.
    #[test]
    fn an_undecodable_codec_falls_back_to_the_container() {
        use symphonia::core::codecs::CODEC_TYPE_EAC3;

        assert_eq!(codec_name(CODEC_TYPE_EAC3, Some("M4A")), "m4a");
        assert_eq!(codec_name(CODEC_TYPE_NULL, Some("wav")), "wav");
        assert_eq!(codec_name(CODEC_TYPE_NULL, None), "unknown");
        assert_eq!(codec_name(CODEC_TYPE_NULL, Some("")), "unknown");
    }
}
