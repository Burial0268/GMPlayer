//! Encoded source adaptation for the pure AutoMix analyzer.

use rodio::{Decoder, Source};
use serde::Deserialize;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::spool::PcmSpool;
use super::{analyze_mono_samples, TrackAnalysis};
use crate::decoder::OpenedDecoder;
use crate::error::AudioError;

// ─── Input ───────────────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomixAnalyzeRequest {
    /// Encoded audio file bytes.
    pub audio_data: Vec<u8>,
    /// Whether to run BPM detection
    pub analyze_bpm: Option<bool>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomixAnalyzeSourceRequest {
    /// Local file path or already-downloaded temp path.
    pub source: String,
    /// Whether to run BPM detection.
    pub analyze_bpm: Option<bool>,
}

/// Cap for the duration-hint preallocation (in mono samples). On 64-bit
/// targets: 64M samples ≈ 256 MiB of f32, covering ~24 min @ 44.1 kHz or
/// ~11 min @ 96 kHz. On the 32-bit Android targets (armv7/i686) the cap is
/// 16M samples ≈ 64 MiB so a corrupt header can never turn into a blind
/// allocation that pressures a 32-bit address space. Tracks longer than the
/// cap simply fall back to doubling growth beyond it, which is never worse
/// than the old un-hinted behavior.
#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
const MONO_PREALLOC_MAX_SAMPLES: usize = 64 << 20;
#[cfg(all(not(target_arch = "wasm32"), not(target_pointer_width = "64")))]
const MONO_PREALLOC_MAX_SAMPLES: usize = 16 << 20;

/// Mono-sample capacity estimate from the container duration hint, with a
/// small headroom margin (~1.6% + 1024) so hints that undershoot slightly do
/// not push the buffer into a full doubling reallocation for the excess.
///
/// The estimate is clamped in f64 space BEFORE any usize arithmetic: corrupt
/// containers can report astronomical durations (symphonia maps the MP4 v0
/// unknown-duration sentinel to u64::MAX seconds), and the saturating float
/// cast plus margin addition must not overflow-panic on such hints.
#[cfg(not(target_arch = "wasm32"))]
fn mono_capacity_hint(duration_hint: Option<f32>, sample_rate: u32) -> usize {
    let Some(duration) = duration_hint.filter(|d| d.is_finite() && *d > 0.0) else {
        return 0;
    };
    let frames = (duration as f64 * sample_rate as f64).ceil();
    if !frames.is_finite() || frames <= 0.0 {
        return 0;
    }
    let frames = frames.min(MONO_PREALLOC_MAX_SAMPLES as f64) as usize;
    frames
        .saturating_add(frames / 64)
        .saturating_add(1024)
        .min(MONO_PREALLOC_MAX_SAMPLES)
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_audio_to_mono(audio_data: Vec<u8>) -> Result<(Vec<f32>, u32, f32), String> {
    decode_source_to_mono(Cursor::new(audio_data))
}

/// Records the first genuine I/O error seen by the streamed decode source.
///
/// rodio/symphonia deliberately conflate mid-stream I/O errors with normal
/// end-of-stream (symphonia signals EOF as `IoError(UnexpectedEof)`), so a
/// dying disk or vanished network mount would otherwise silently truncate the
/// decoded PCM and produce a "successful" analysis over partial audio. The
/// old whole-file `fs::read` path surfaced every read error before decoding;
/// latching restores exactly that property for the streamed path.
/// `ErrorKind::Interrupted` is not latched to match `read_to_end`, which
/// transparently retries it.
#[cfg(not(target_arch = "wasm32"))]
struct IoErrorLatchReader<R> {
    inner: R,
    latch: Arc<Mutex<Option<String>>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl<R> IoErrorLatchReader<R> {
    fn record(&self, err: &std::io::Error) {
        if err.kind() == std::io::ErrorKind::Interrupted {
            return;
        }
        if let Ok(mut latch) = self.latch.lock() {
            if latch.is_none() {
                *latch = Some(err.to_string());
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<R: Read> Read for IoErrorLatchReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.inner.read(buf) {
            Ok(n) => Ok(n),
            Err(err) => {
                self.record(&err);
                Err(err)
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<R: Seek> Seek for IoErrorLatchReader<R> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        match self.inner.seek(pos) {
            Ok(offset) => Ok(offset),
            Err(err) => {
                self.record(&err);
                Err(err)
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<R: ::symphonia::core::io::MediaSource> ::symphonia::core::io::MediaSource
    for IoErrorLatchReader<R>
{
    fn is_seekable(&self) -> bool {
        self.inner.is_seekable()
    }
    fn byte_len(&self) -> Option<u64> {
        self.inner.byte_len()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_source_to_mono<R>(input: R) -> Result<(Vec<f32>, u32, f32), String>
where
    R: std::io::Read + std::io::Seek + Send + Sync + 'static,
{
    decode_source_to_mono_until(input, &|| false)
}

/// How often the decode loop asks whether it was superseded: ~1.5 s of audio at
/// 44.1 kHz, so a skipped prepare stops within milliseconds and the check costs
/// nothing per sample. Must be a power of two.
#[cfg(not(target_arch = "wasm32"))]
const CANCEL_POLL_FRAMES: usize = 1 << 16;

#[cfg(not(target_arch = "wasm32"))]
fn decode_source_to_mono_until<R>(
    input: R,
    cancelled: &dyn Fn() -> bool,
) -> Result<(Vec<f32>, u32, f32), String>
where
    R: std::io::Read + std::io::Seek + Send + Sync + 'static,
{
    let mut decoder = Decoder::new(input).map_err(|e| format!("decode audio: {e}"))?;
    collect_mono_until(&mut decoder, cancelled)
}

#[cfg(not(target_arch = "wasm32"))]
fn collect_mono_until<D: Source<Item = f32>>(
    decoder: &mut D,
    cancelled: &dyn Fn() -> bool,
) -> Result<(Vec<f32>, u32, f32), String> {
    let channels = decoder.channels().get() as usize;
    let sample_rate = decoder.sample_rate().get();
    let duration_hint = decoder.total_duration().map(|d| d.as_secs_f32());

    // Preallocate from the duration hint: growing a multi-minute track by
    // push-doubling briefly holds the old and new buffers at every doubling
    // (up to ~3× the final size at the last one), which alone accounted for
    // most of the transient RES spike during AutoMix prepare. This supersedes
    // master's time-capped `reserve_exact` variant: the cap here is an
    // absolute sample count (so hi-res rates cannot inflate it), computed in
    // f64 with saturating margin arithmetic, and sized per pointer width.
    let mut mono = Vec::with_capacity(mono_capacity_hint(duration_hint, sample_rate));
    let mut frame_sum = 0.0f32;
    let mut frame_channel = 0usize;

    for sample in decoder {
        frame_sum += sample;
        frame_channel += 1;

        if frame_channel == channels {
            mono.push(frame_sum / channels as f32);
            frame_sum = 0.0;
            frame_channel = 0;

            if mono.len() & (CANCEL_POLL_FRAMES - 1) == 0 && cancelled() {
                return Err("decode audio: cancelled".into());
            }
        }
    }

    if frame_channel > 0 {
        mono.push(frame_sum / frame_channel as f32);
    }

    if mono.is_empty() {
        return Err("decode audio: no samples".into());
    }

    let decoded_duration = mono.len() as f32 / sample_rate as f32;
    let duration = duration_hint
        .filter(|duration| *duration > 0.0)
        .unwrap_or(decoded_duration);

    Ok((mono, sample_rate, duration))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn analyze_audio_bytes(req: AutomixAnalyzeRequest) -> Result<TrackAnalysis, String> {
    let analyze_bpm = req.analyze_bpm.unwrap_or(true);
    let (samples, sample_rate, duration) = decode_audio_to_mono(req.audio_data)?;
    Ok(analyze_mono_samples(
        &samples,
        sample_rate,
        duration,
        analyze_bpm,
    ))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn analyze_audio_file(
    path: impl AsRef<Path>,
    analyze_bpm: bool,
) -> Result<TrackAnalysis, String> {
    analyze_audio_file_until(path, analyze_bpm, &|| false)
}

/// `analyze_audio_file` that gives up once `cancelled` turns true. A blocking
/// task cannot be aborted, so without this a superseded AutoMix prepare keeps
/// decoding a whole track nobody will use.
#[cfg(not(target_arch = "wasm32"))]
pub fn analyze_audio_file_until(
    path: impl AsRef<Path>,
    analyze_bpm: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<TrackAnalysis, String> {
    // Analysis opens an independent reader: publishing its read-ahead cursor
    // would make the playback stream gate believe bytes were already consumed.
    let io_error = Arc::new(Mutex::new(None::<String>));
    let opened = OpenedDecoder::open_with(|| {
        if cancelled() {
            return Err(AudioError::Decode("analysis cancelled".into()));
        }
        if let Some(error) = io_error.lock().ok().and_then(|latch| latch.clone()) {
            return Err(AudioError::Io(std::io::Error::other(error)));
        }
        let input = crate::source::open(crate::source::SourceLocator::from_path(path.as_ref()))?;
        let extension = input.extension().map(str::to_string);
        Ok((
            IoErrorLatchReader {
                inner: input,
                latch: Arc::clone(&io_error),
            },
            extension,
        ))
    });
    let decoded = match opened {
        Ok(OpenedDecoder::Symphonia(mut source)) => PcmSpool::decode(&mut source, cancelled),
        Ok(OpenedDecoder::Ffmpeg(mut source)) => {
            let decoded = PcmSpool::decode(&mut source, cancelled);
            match source.take_error() {
                Some(error) => Err(format!("decode audio: FFmpeg: {error}")),
                None => decoded,
            }
        }
        Err(error @ AudioError::Io(_)) => Err(format!("read audio source: {error}")),
        Err(error) => Err(format!("decode audio: {error}")),
    };
    if let Some(error) = io_error.lock().ok().and_then(|mut latch| latch.take()) {
        return Err(format!("read audio source: {error}"));
    }
    decoded?.analyze(analyze_bpm)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn analyze_audio_source(req: AutomixAnalyzeSourceRequest) -> Result<TrackAnalysis, String> {
    analyze_audio_file(req.source, req.analyze_bpm.unwrap_or(true))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::io::Write;

    /// Minimal 16-bit PCM WAV with a 220 Hz sine, `frames` frames per channel.
    fn wav_bytes(frames: usize, channels: u16, sample_rate: u32) -> Vec<u8> {
        let data_len = frames * channels as usize * 2;
        let mut out = Vec::with_capacity(44 + data_len);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&(sample_rate * channels as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data_len as u32).to_le_bytes());
        for i in 0..frames {
            let t = i as f32 / sample_rate as f32;
            let v = ((std::f32::consts::TAU * 220.0 * t).sin() * 20_000.0) as i16;
            for _ in 0..channels {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }

    #[test]
    fn file_and_bytes_decode_paths_are_identical() {
        let bytes = wav_bytes(44_100, 2, 44_100);

        let (mono_bytes, rate_bytes, duration_bytes) =
            decode_audio_to_mono(bytes.clone()).expect("bytes decode");

        let mut tmp = tempfile::NamedTempFile::new().expect("temp file");
        tmp.write_all(&bytes).expect("write wav");
        tmp.flush().expect("flush wav");
        let file = std::fs::File::open(tmp.path()).expect("open wav");
        let (mono_file, rate_file, duration_file) =
            decode_source_to_mono(std::io::BufReader::new(file)).expect("file decode");

        assert_eq!(rate_bytes, rate_file);
        assert_eq!(duration_bytes, duration_file);
        assert_eq!(mono_bytes, mono_file);
    }

    #[test]
    fn file_and_bytes_analysis_are_identical() {
        let bytes = wav_bytes(44_100, 2, 44_100);
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&bytes).unwrap();
        file.flush().unwrap();
        let from_file = analyze_audio_file(file.path(), false).unwrap();
        let from_bytes = analyze_audio_bytes(AutomixAnalyzeRequest {
            audio_data: bytes,
            analyze_bpm: Some(false),
        })
        .unwrap();
        assert_eq!(
            serde_json::to_value(from_file).unwrap(),
            serde_json::to_value(from_bytes).unwrap()
        );
    }

    #[test]
    fn file_analysis_never_publishes_the_playback_cursor_for_either_codec() {
        use crate::source::{growing, TempGuard};
        for bytes in [
            wav_bytes(44_100, 2, 44_100),
            include_bytes!("../../tests/fixtures/codecs/tone.opus").to_vec(),
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            file.write_all(&bytes).unwrap();
            file.flush().unwrap();
            let total = bytes.len() as u64;
            let progress = growing::Progress::new(total);
            let writer = progress.begin_writer(0);
            progress.advance(writer, total);
            progress.end_writer(writer, Ok(()));
            let guard = TempGuard::streaming(file.into_temp_path(), Arc::clone(&progress));
            growing::register(&guard, Arc::clone(&progress));

            let analysis = analyze_audio_file(&guard, false).unwrap();
            assert!(analysis.volume.peak > 0.0);
            assert_eq!(
                progress.lead(),
                total,
                "analysis must not consume the playback reader's lead"
            );
        }
    }

    #[test]
    fn cancelled_decode_stops_instead_of_finishing_the_track() {
        let bytes = wav_bytes(CANCEL_POLL_FRAMES * 4, 2, 44_100);

        let err = decode_source_to_mono_until(Cursor::new(bytes), &|| true)
            .expect_err("a cancelled decode must not return PCM");

        assert!(err.contains("cancelled"), "unexpected error: {err}");
    }

    #[test]
    fn mono_buffer_is_preallocated_from_duration_hint() {
        let frames = 2 * 44_100;
        let (mono, rate, _) = decode_audio_to_mono(wav_bytes(frames, 2, 44_100)).expect("decode");

        assert_eq!(rate, 44_100);
        assert_eq!(mono.len(), frames);
        assert!(mono.capacity() >= mono.len());
        // With the hint-based preallocation, capacity must track the frame
        // count closely; the old push-doubling growth would land on the next
        // power of two (131072) and fail this bound.
        assert!(
            mono.capacity() <= mono.len() + mono.len() / 32 + 4096,
            "capacity {} too far above len {}",
            mono.capacity(),
            mono.len()
        );
    }

    #[test]
    fn mono_capacity_hint_is_bounded_and_defensive() {
        assert_eq!(mono_capacity_hint(None, 44_100), 0);
        assert_eq!(mono_capacity_hint(Some(0.0), 44_100), 0);
        assert_eq!(mono_capacity_hint(Some(f32::NAN), 44_100), 0);
        assert_eq!(mono_capacity_hint(Some(-3.0), 44_100), 0);

        let two_seconds = mono_capacity_hint(Some(2.0), 44_100);
        assert!(two_seconds >= 2 * 44_100);
        assert!(two_seconds <= 2 * 44_100 + (2 * 44_100) / 32 + 4096);

        // A corrupt header claiming hours must not turn into a giant blind
        // allocation.
        assert_eq!(
            mono_capacity_hint(Some(36_000.0), 192_000),
            MONO_PREALLOC_MAX_SAMPLES
        );

        // Astronomical hints (symphonia maps the MP4 v0 unknown-duration
        // sentinel to u64::MAX seconds) must clamp without overflow-panicking
        // in dev/test builds, on 32-bit and 64-bit targets alike.
        assert_eq!(
            mono_capacity_hint(Some(f32::MAX), u32::MAX),
            MONO_PREALLOC_MAX_SAMPLES
        );
        assert_eq!(
            mono_capacity_hint(Some(u64::MAX as f32), 44_100),
            MONO_PREALLOC_MAX_SAMPLES
        );
    }

    /// Read+Seek source that fails with a genuine I/O error after a byte budget,
    /// simulating a dying disk / vanished network mount mid-decode.
    struct FailAfter {
        inner: Cursor<Vec<u8>>,
        remaining: usize,
    }

    impl Read for FailAfter {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.remaining == 0 {
                return Err(std::io::Error::other("injected io failure"));
            }
            let limit = buf.len().min(self.remaining);
            let read = self.inner.read(&mut buf[..limit])?;
            self.remaining -= read;
            Ok(read)
        }
    }

    impl Seek for FailAfter {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(pos)
        }
    }

    impl ::symphonia::core::io::MediaSource for FailAfter {
        fn is_seekable(&self) -> bool {
            true
        }
        fn byte_len(&self) -> Option<u64> {
            Some(self.inner.get_ref().len() as u64)
        }
    }

    #[test]
    fn mid_stream_io_errors_are_latched() {
        let bytes = wav_bytes(44_100, 2, 44_100);
        let budget = bytes.len() / 2;
        let latch = Arc::new(Mutex::new(None));
        let reader = IoErrorLatchReader {
            inner: FailAfter {
                inner: Cursor::new(bytes),
                remaining: budget,
            },
            latch: Arc::clone(&latch),
        };

        // rodio/symphonia conflate the mid-stream failure with end-of-stream,
        // so the decode itself may "succeed" with truncated PCM — the latch is
        // what lets analyze_audio_file surface it as a read error.
        let mut input = Some(reader);
        let opened = OpenedDecoder::open_with(|| Ok((input.take().expect("WAV opens once"), None)))
            .expect("open native analysis decoder");
        let OpenedDecoder::Symphonia(mut decoder) = opened else {
            panic!("WAV must use Symphonia");
        };
        let _ = PcmSpool::decode(&mut decoder, &|| false);
        let recorded = latch
            .lock()
            .unwrap()
            .take()
            .expect("io error must be latched");
        assert!(
            recorded.contains("injected io failure"),
            "unexpected latched error: {recorded}"
        );
    }

    #[test]
    fn analyze_audio_file_surfaces_unreadable_source_as_read_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let err = analyze_audio_file(dir.path(), false).expect_err("directory must not analyze");
        assert!(
            err.starts_with("read audio source:"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn scratch_analysis_remains_cancellable_after_decode_finishes() {
        let mut decoder = Decoder::new(Cursor::new(wav_bytes(44_100, 2, 44_100))).unwrap();
        let flag = std::cell::Cell::new(false);
        let cancelled = || flag.get();
        let spool = PcmSpool::decode(&mut decoder, &cancelled).unwrap();
        flag.set(true);
        let error = spool.analyze(false).unwrap_err();
        assert!(error.contains("cancelled"), "{error}");
    }

    #[test]
    fn cancelled_file_decode_does_not_finish_preparation() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&wav_bytes(44_100 * 4, 2, 44_100)).unwrap();
        file.flush().unwrap();
        let polls = std::cell::Cell::new(0);
        let error = analyze_audio_file_until(file.path(), true, &|| {
            polls.set(polls.get() + 1);
            polls.get() >= 3
        })
        .unwrap_err();
        assert!(error.contains("cancelled"), "{error}");
    }

    #[test]
    #[ignore = "set AUTOMIX_PROFILE_FILE; run the test executable directly to measure RSS"]
    fn profile_file_analysis() {
        let path = std::env::var("AUTOMIX_PROFILE_FILE").expect("set AUTOMIX_PROFILE_FILE");
        if let Ok(cache) = std::env::var("AUTOMIX_PROFILE_CACHE") {
            crate::source::set_stream_cache_dir(cache.into());
        }
        let start = std::time::Instant::now();
        let analysis = analyze_audio_file(path, true).expect("analyze profile input");
        println!("analysis elapsed: {:?}", start.elapsed());
        if let Ok(output) = std::env::var("AUTOMIX_PROFILE_JSON") {
            std::fs::write(output, serde_json::to_vec(&analysis).unwrap()).unwrap();
        }
    }

    /// Prints the sections found in a real track, for checking the heuristic
    /// by ear: `AUTOMIX_SECTIONS_FILE=<path> cargo test -p
    /// gmplayer-audio-backend print_sections_of_a_real_track -- --ignored --nocapture`
    #[test]
    #[ignore = "needs AUTOMIX_SECTIONS_FILE pointing at a real track"]
    fn print_sections_of_a_real_track() {
        let path = std::env::var("AUTOMIX_SECTIONS_FILE").expect("set AUTOMIX_SECTIONS_FILE");
        let analysis = analyze_audio_file(&path, true).expect("analyze");
        let Some(sections) = analysis.sections else {
            println!("no sections ({:.1}s)", analysis.duration);
            return;
        };
        println!(
            "{} sections, confidence {:.2}, bpm {:?}",
            sections.sections.len(),
            sections.confidence,
            analysis.bpm.map(|b| (b.bpm, b.confidence))
        );
        for s in sections.sections {
            println!(
                "{:>6.1}-{:>6.1}s  {:<9} energy {:.2}  vocal {:.2}  confidence {:.2}",
                s.start,
                s.end,
                format!("{:?}", s.section_type),
                s.energy,
                s.vocal_risk,
                s.confidence
            );
        }
    }
}
