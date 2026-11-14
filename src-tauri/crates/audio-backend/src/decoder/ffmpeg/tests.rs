use std::io::Write;

use super::*;
use crate::decoder::{convert::FrameConverter, SeekableSymphoniaSource};

const FIXTURES: &[(&str, &[u8], &str)] = &[
    (
        "opus",
        include_bytes!("../../../tests/fixtures/codecs/tone.opus"),
        "opus",
    ),
    (
        "wma",
        include_bytes!("../../../tests/fixtures/codecs/tone.wma"),
        "wmav2",
    ),
    (
        "wv",
        include_bytes!("../../../tests/fixtures/codecs/tone.wv"),
        "wavpack",
    ),
];

fn fixture(extension: &str, bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::Builder::new()
        .suffix(&format!(".{extension}"))
        .tempfile()
        .unwrap();
    file.write_all(bytes).unwrap();
    file.flush().unwrap();
    file
}

fn collect(source: &mut FfmpegSource) -> Vec<f32> {
    let mut samples = Vec::new();
    let channels = source.channels().get() as usize;
    while source.append_interleaved_frames(&mut samples, channels, 512) > 0 {}
    assert!(source.take_error().is_none());
    samples
}

#[test]
fn unsupported_symphonia_codecs_decode_as_finite_pcm() {
    for &(extension, bytes, _) in FIXTURES {
        let file = fixture(extension, bytes);
        assert!(
            SeekableSymphoniaSource::open(file.path()).is_err(),
            "fixture must exercise fallback: {extension}"
        );
        let mut source = FfmpegSource::open(file.path()).unwrap();
        assert_eq!(source.sample_rate().get(), 48_000);
        assert_eq!(source.channels().get(), 2);
        let samples = collect(&mut source);
        assert!(
            (180_000..=204_800).contains(&samples.len()),
            "unexpected sample count for {extension}: {}",
            samples.len()
        );
        assert!(samples
            .iter()
            .all(|sample| sample.is_finite() && sample.abs() < 1.0));
        assert!(samples.iter().any(|sample| sample.abs() > 0.1));
        if extension != "wma" {
            assert_eq!(
                samples.len(),
                192_000,
                "lost leading or trailing samples: {extension}"
            );
        }
    }
}

#[test]
fn downloaded_temp_paths_are_detected_from_their_bytes() {
    for &(_, bytes, codec) in FIXTURES {
        let file = fixture("tmp", bytes);
        let mut source = FfmpegSource::open(file.path()).unwrap();
        assert!(!collect(&mut source).is_empty());
        let info = crate::metadata::extract_metadata_only(file.path()).unwrap();
        assert_eq!(info.codec, codec);
    }
}

#[test]
fn opus_and_lossless_seeks_land_on_the_requested_audio() {
    for &(extension, bytes, _) in FIXTURES
        .iter()
        .filter(|(extension, _, _)| *extension != "wma")
    {
        let file = fixture(extension, bytes);
        let mut source = FfmpegSource::open(file.path()).unwrap();
        let all = collect(&mut source);
        source.try_seek(Duration::from_millis(750)).unwrap();
        let mut after_seek = Vec::new();
        assert_eq!(
            source.append_interleaved_frames(&mut after_seek, 2, 1024),
            1024
        );
        let expected = &all[72_000..72_000 + after_seek.len()];
        let rms = (expected
            .iter()
            .zip(&after_seek)
            .map(|(left, right)| f64::from(left - right).powi(2))
            .sum::<f64>()
            / expected.len() as f64)
            .sqrt();
        assert!(
            rms < 0.003,
            "seek landed on different audio for {extension}: rms={rms}"
        );
        source.try_seek(Duration::ZERO).unwrap();
        assert_eq!(collect(&mut source).len(), all.len());
    }
}

#[test]
fn the_playback_converter_preserves_ffmpeg_pcm_in_bulk_mode() {
    let (_, bytes, _) = FIXTURES[2];
    let file = fixture("wv", bytes);
    let mut source = FfmpegSource::open(file.path()).unwrap();
    let expected = collect(&mut source);
    let source = FfmpegSource::open(file.path()).unwrap();
    let mut frames = FrameConverter::new(source, 2, 48_000, 2, 48_000);
    assert!(frames.can_bulk_append());
    let mut actual = Vec::new();
    while frames.append_frames(&mut actual, 512) > 0 {}
    assert_eq!(expected, actual);
}

#[test]
fn the_library_and_player_read_the_same_extended_metadata() {
    for &(extension, bytes, codec) in FIXTURES {
        let file = fixture(extension, bytes);
        let info = crate::metadata::extract_metadata_only(file.path()).unwrap();
        let (tag_info, tags, needs_cache) =
            crate::metadata::extract_track_tags(file.path(), true).unwrap();
        assert_eq!(info.codec, codec);
        assert_eq!(tag_info.codec, info.codec);
        assert_eq!(tag_info.sample_rate, info.sample_rate);
        assert_eq!(tag_info.duration_secs, info.duration_secs);
        assert!((1.9..2.1).contains(&info.duration_secs));
        assert_eq!(tags.title.as_deref(), Some("Codec fixture"));
        assert_eq!(tags.artist.as_deref(), Some("GMPlayer tests"));
        assert_eq!(tags.album.as_deref(), Some("Native codec tests"));
        assert_eq!(tags.track_no, Some(3));
        assert!(!needs_cache);
    }
}

#[test]
fn native_automix_can_analyze_extended_files_and_still_cancel() {
    let file = fixture("opus", FIXTURES[0].1);
    let result = crate::automix::analyze_audio_file(file.path(), false).unwrap();
    assert!(result.duration > 1.9);
    assert!(result.volume.rms > 0.05);
    let error = crate::automix::analyze_audio_file_until(file.path(), false, &|| true).unwrap_err();
    assert!(error.contains("cancelled"), "{error}");
}

#[test]
fn bounded_file_analysis_matches_the_full_pcm_reference_for_extended_codecs() {
    for &(extension, bytes, _) in FIXTURES {
        let file = fixture(extension, bytes);
        let mut source = FfmpegSource::open(file.path()).unwrap();
        let rate = source.sample_rate().get();
        let channels = source.channels().get() as usize;
        let interleaved = collect(&mut source);
        let mono: Vec<f32> = interleaved
            .chunks(channels)
            .map(|frame| frame.iter().copied().sum::<f32>() / frame.len() as f32)
            .collect();
        // File analysis reports the decoded length, not the container's hint.
        let duration = mono.len() as f32 / rate as f32;
        let expected = crate::automix::analyze_mono_samples(&mono, rate, duration, true);
        let actual = crate::automix::analyze_audio_file(file.path(), true).unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap(),
            "{extension}"
        );
    }
}

#[test]
fn bundled_ffmpeg_has_the_decoders_we_advertise_but_not_av3a() {
    use std::ffi::CString;
    for name in [
        "opus",
        "ape",
        "wmav2",
        "wavpack",
        "tta",
        "tak",
        "mpc7",
        "mpc8",
        "dsd_lsbf",
        "ac3",
        "eac3",
        "dca",
        "amrnb",
        "pcm_s16le",
    ] {
        let name = CString::new(name).unwrap();
        assert!(
            !unsafe { ffmpeg_audio::sys::avcodec_find_decoder_by_name(name.as_ptr()) }.is_null(),
            "missing decoder: {name:?}"
        );
    }
    assert!(!crate::AUDIO_EXTENSIONS.contains(&"av3a"));
}
