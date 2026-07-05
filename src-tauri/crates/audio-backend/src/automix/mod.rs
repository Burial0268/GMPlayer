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
use std::io::Cursor;
use std::path::Path;

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

fn decode_audio_to_mono(audio_data: Vec<u8>) -> Result<(Vec<f32>, u32, f32), String> {
    let cursor = Cursor::new(audio_data);
    let decoder = Decoder::new(cursor).map_err(|e| format!("decode audio: {e}"))?;
    let channels = decoder.channels().max(1) as usize;
    let sample_rate = decoder.sample_rate().max(1);
    let duration_hint = decoder.total_duration().map(|d| d.as_secs_f32());

    let mut mono = Vec::new();
    let mut frame_sum = 0.0f32;
    let mut frame_channel = 0usize;

    for sample in decoder.convert_samples::<f32>() {
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
    let audio_data = std::fs::read(path.as_ref()).map_err(|e| format!("read audio source: {e}"))?;
    let (samples, sample_rate, duration) = decode_audio_to_mono(audio_data)?;
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
