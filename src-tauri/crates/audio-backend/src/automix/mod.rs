/// Native Rust audio analysis for AutoMix.
///
/// Performs volume, energy, multiband IIR (biquad), BPM detection, and spectral
/// fingerprinting in native code. Matches the JS `analysis-worker.ts` logic
/// so results are identical regardless of which path (native vs Worker) is used.
///
/// Invoked via `analyze_audio_native` tauri::command — receives raw mono PCM,
/// returns full TrackAnalysis.
use rodio::{Decoder, Source};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex};

// ─── Input ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomixAnalyzeRequest {
    /// Encoded audio file bytes.
    pub audio_data: Vec<u8>,
    /// Whether to run BPM detection
    pub analyze_bpm: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomixAnalyzeSourceRequest {
    /// Local file path or already-downloaded temp path.
    pub source: String,
    /// Whether to run BPM detection.
    pub analyze_bpm: Option<bool>,
}

// ─── Output types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeAnalysis {
    pub peak: f32,
    pub rms: f32,
    #[serde(rename = "estimatedLUFS")]
    pub estimated_lufs: f32,
    pub gain_adjustment: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnergyAnalysis {
    pub energy_per_second: Vec<f32>,
    pub outro_start_offset: f32,
    pub intro_end_offset: f32,
    pub average_energy: f32,
    pub trailing_silence: f32,
    pub is_fade_out: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BPMResult {
    pub bpm: f32,
    pub confidence: f32,
    pub beat_grid: Vec<f32>,
    pub analysis_offset: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Phrase {
    pub start: f32,
    pub end: f32,
    pub index: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhraseAnalysis {
    pub phrases: Vec<Phrase>,
    pub mix_out_phrase: Option<Phrase>,
    pub mix_in_phrase: Option<Phrase>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SongSectionKind {
    Start,
    Verse,
    Chorus,
    Bridge,
    Breakdown,
    Outro,
    Silence,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SongSection {
    pub section_type: SongSectionKind,
    pub start: f32,
    pub end: f32,
    pub index: u32,
    pub confidence: f32,
    pub energy: f32,
    pub vocal_risk: f32,
    pub mix_suitability: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionAnalysis {
    pub sections: Vec<SongSection>,
    pub confidence: f32,
    pub method: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VocalActivityAnalysis {
    pub window_duration: f32,
    pub risk: Vec<f32>,
    pub confidence: f32,
    pub method: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MixPointCandidate {
    pub time: f32,
    pub score: f32,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_type: Option<SongSectionKind>,
    pub vocal_risk: f32,
    pub energy: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MixPointAnalysis {
    pub candidates: Vec<MixPointCandidate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<MixPointCandidate>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpectralFingerprint {
    pub bands: Vec<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntroAnalysis {
    pub quiet_intro_duration: f32,
    pub energy_build_duration: f32,
    pub intro_energy_ratio: f32,
    pub multiband_energy: Option<MultibandEnergy>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultibandEnergy {
    pub low: Vec<f32>,
    pub mid: Vec<f32>,
    pub high: Vec<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutroAnalysis {
    pub outro_type: String,
    pub outro_confidence: f32,
    pub musical_end_offset: f32,
    pub suggested_crossfade_start: f32,
    pub multiband_energy: MultibandEnergy,
    pub spectral_flux: Vec<f32>,
    pub short_term_loudness: Vec<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deceleration_start: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sustain_onset: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outro_section_start: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loop_period: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackAnalysis {
    pub volume: VolumeAnalysis,
    pub energy: EnergyAnalysis,
    pub bpm: Option<BPMResult>,
    pub fingerprint: SpectralFingerprint,
    pub outro: Option<OutroAnalysis>,
    pub intro: Option<IntroAnalysis>,
    pub phrases: Option<PhraseAnalysis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sections: Option<SectionAnalysis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocal_activity: Option<VocalActivityAnalysis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mix_candidates: Option<MixPointAnalysis>,
    pub duration: f32,
}

// ─── Constants (matching JS worker) ──────────────────────────────────

const TARGET_LUFS: f32 = -14.0;
const REFERENCE_RMS: f32 = 0.707;
const SILENCE_THRESHOLD: f32 = 0.003;
const BPM_ANALYSIS_DURATION: usize = 30;
const BPM_ANALYSIS_RATE: u32 = 11025;
const MIN_BPM: f32 = 60.0;
const MAX_BPM: f32 = 200.0;
const OUTRO_WINDOW_MS: f32 = 250.0;
const OUTRO_ANALYSIS_SECONDS: f32 = 60.0;
const INTRO_SCAN_SECONDS: usize = 20;
const SECTION_PHRASE_BEATS: f32 = 16.0;
const SECTION_FALLBACK_SECONDS: f32 = 8.0;
const SECTION_MIN_SECONDS: f32 = 4.0;
const SECTION_MAX_SECONDS: f32 = 24.0;
const MAX_SONG_SECTIONS: usize = 48;
const VOCAL_WINDOW_SECONDS: f32 = OUTRO_WINDOW_MS / 1000.0;

mod analysis;
mod structure;
mod vocal;

use analysis::{analyze_energy, analyze_volume, compute_fingerprint, run_bpm_detection};
use structure::{
    analyze_intro, analyze_outro_multiband, analyze_song_sections, build_mix_point_analysis,
};
use vocal::analyze_vocal_activity;

// ─── Main Analysis Entry Point ─────────────────────────────────────

/// Cap for the duration-hint preallocation (in mono samples). On 64-bit
/// targets: 64M samples ≈ 256 MiB of f32, covering ~24 min @ 44.1 kHz or
/// ~11 min @ 96 kHz. On the 32-bit Android targets (armv7/i686) the cap is
/// 16M samples ≈ 64 MiB so a corrupt header can never turn into a blind
/// allocation that pressures a 32-bit address space. Tracks longer than the
/// cap simply fall back to doubling growth beyond it, which is never worse
/// than the old un-hinted behavior.
#[cfg(target_pointer_width = "64")]
const MONO_PREALLOC_MAX_SAMPLES: usize = 64 << 20;
#[cfg(not(target_pointer_width = "64"))]
const MONO_PREALLOC_MAX_SAMPLES: usize = 16 << 20;

/// Mono-sample capacity estimate from the container duration hint, with a
/// small headroom margin (~1.6% + 1024) so hints that undershoot slightly do
/// not push the buffer into a full doubling reallocation for the excess.
///
/// The estimate is clamped in f64 space BEFORE any usize arithmetic: corrupt
/// containers can report astronomical durations (symphonia maps the MP4 v0
/// unknown-duration sentinel to u64::MAX seconds), and the saturating float
/// cast plus margin addition must not overflow-panic on such hints.
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
struct IoErrorLatchReader<R> {
    inner: R,
    latch: Arc<Mutex<Option<String>>>,
}

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

fn decode_source_to_mono<R>(input: R) -> Result<(Vec<f32>, u32, f32), String>
where
    R: std::io::Read + std::io::Seek + Send + Sync + 'static,
{
    let decoder = Decoder::new(input).map_err(|e| format!("decode audio: {e}"))?;
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

pub fn analyze_mono_samples(
    samples: &[f32],
    sample_rate: u32,
    duration: f32,
    analyze_bpm: bool,
) -> TrackAnalysis {
    let volume = analyze_volume(samples);
    let energy = analyze_energy(samples, sample_rate, duration);
    let bpm = if analyze_bpm {
        run_bpm_detection(samples, sample_rate, duration)
    } else {
        None
    };
    let fingerprint = compute_fingerprint(samples, sample_rate);
    let outro = analyze_outro_multiband(samples, sample_rate, duration, energy.trailing_silence);
    let intro = analyze_intro(
        &energy.energy_per_second,
        energy.average_energy,
        samples,
        sample_rate,
        duration,
    );

    // Phrase analysis (lightweight)
    let phrases = bpm.as_ref().and_then(|b| {
        if b.confidence < 0.3 || b.beat_grid.len() < 32 {
            return None;
        }
        let beats_per_phrase = 16usize;
        let phrases: Vec<Phrase> = b
            .beat_grid
            .chunks(beats_per_phrase)
            .enumerate()
            .filter_map(|(i, chunk)| {
                if chunk.len() >= beats_per_phrase {
                    Some(Phrase {
                        start: chunk[0],
                        end: chunk[beats_per_phrase - 1],
                        index: i as u32,
                    })
                } else {
                    None
                }
            })
            .collect();

        if phrases.len() < 2 {
            return None;
        }

        let intro_end = intro
            .as_ref()
            .map(|i| i.energy_build_duration)
            .unwrap_or(0.0);

        let mix_in_phrase = phrases
            .iter()
            .find(|p| p.start >= intro_end)
            .cloned()
            .unwrap_or_else(|| phrases[0].clone());

        let mix_out_phrase = if phrases.len() >= 4 {
            Some(phrases[phrases.len() - 4].clone())
        } else if phrases.len() >= 2 {
            Some(phrases[phrases.len() - 2].clone())
        } else {
            None
        };

        Some(PhraseAnalysis {
            phrases,
            mix_out_phrase,
            mix_in_phrase: Some(mix_in_phrase),
        })
    });
    let vocal_activity = analyze_vocal_activity(samples, sample_rate, duration);
    let sections = analyze_song_sections(
        &energy,
        bpm.as_ref(),
        intro.as_ref(),
        outro.as_ref(),
        vocal_activity.as_ref(),
        duration,
    );
    let mix_candidates = build_mix_point_analysis(
        &energy,
        bpm.as_ref(),
        outro.as_ref(),
        phrases.as_ref(),
        sections.as_ref(),
        vocal_activity.as_ref(),
        duration,
    );

    TrackAnalysis {
        volume,
        energy,
        bpm,
        fingerprint,
        outro,
        intro,
        phrases,
        sections,
        vocal_activity,
        mix_candidates,
        duration,
    }
}

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

pub fn analyze_audio_file(
    path: impl AsRef<Path>,
    analyze_bpm: bool,
) -> Result<TrackAnalysis, String> {
    // Stream the compressed file from disk instead of fs::read-ing it whole:
    // the encoded bytes would otherwise stay resident (owned by the decoder's
    // Cursor) for the entire full-track decode, adding the whole file size on
    // top of the PCM buffer at the peak.
    //
    // Goes through `source::open` rather than `File::open` so an Android
    // `content://` track gets analysed — and therefore crossfaded — like any
    // other. This failure is invisible if it is missed: analysis runs on its
    // own thread and a miss only logs a warning, so AutoMix quietly degrades to
    // hard cuts on exactly one platform.
    let opened = crate::source::open(crate::source::SourceLocator::from_path(path.as_ref()))
        .map_err(|e| format!("read audio source: {e}"))?;
    let io_error = Arc::new(Mutex::new(None));
    let reader = IoErrorLatchReader {
        inner: std::io::BufReader::new(opened),
        latch: Arc::clone(&io_error),
    };
    let decoded = decode_source_to_mono(reader);
    // Surface underlying disk errors exactly like the old whole-file read did,
    // whether or not the decoder managed to limp past them with partial PCM.
    if let Some(err) = io_error.lock().ok().and_then(|mut latch| latch.take()) {
        return Err(format!("read audio source: {err}"));
    }
    let (samples, sample_rate, duration) = decoded?;
    Ok(analyze_mono_samples(
        &samples,
        sample_rate,
        duration,
        analyze_bpm,
    ))
}

pub fn analyze_audio_source(req: AutomixAnalyzeSourceRequest) -> Result<TrackAnalysis, String> {
    analyze_audio_file(req.source, req.analyze_bpm.unwrap_or(true))
}

#[cfg(test)]
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
        let _ = decode_source_to_mono(reader);
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
}
