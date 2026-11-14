//! Blocking source acquisition, metadata and analysis preparation.

use super::cache::analyze_cached;
use super::plan::build_analysis_backed_plan;
use super::PreparedTrack;
use crate::types::{
    AudioInfo, AudioQuality, AutoMixConfig, DisplayAudioInfo, SongData, TrackIdentity,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{info, info_span};

pub(in crate::player) struct AutoMixPrepareRequest {
    pub generation: u64,
    pub transition_id: Option<u64>,
    pub current_index: usize,
    pub next_index: usize,
    pub current_id: String,
    pub current_duration: Option<f64>,
    pub current_source_path: Option<PathBuf>,
    pub next_song: SongData,
    pub next_identity: Option<TrackIdentity>,
    pub config: AutoMixConfig,
}

/// Tells a blocking prepare it was superseded. The task cannot be aborted, so
/// it polls this at its checkpoints and stops decoding instead of finishing a
/// track nobody will use.
pub(in crate::player) struct PrepareCancel {
    live: Arc<AtomicU64>,
    generation: u64,
}

impl PrepareCancel {
    pub(in crate::player) fn new(live: Arc<AtomicU64>, generation: u64) -> Self {
        Self { live, generation }
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.live.load(Ordering::Acquire) != self.generation
    }
}

const SUPERSEDED: &str = "AutoMix prepare superseded";

pub(in crate::player) struct AutoMixPrepareResult {
    pub generation: u64,
    pub transition_id: Option<u64>,
    pub current_index: usize,
    pub current_id: String,
    pub result: Result<PreparedTrack, String>,
}

pub(in crate::player) fn run_prepare_request_blocking(
    request: AutoMixPrepareRequest,
    cancel: &PrepareCancel,
) -> AutoMixPrepareResult {
    let started = std::time::Instant::now();
    let span = info_span!("automix_prepare",
        generation = request.generation,
        transition_id = ?request.transition_id,
        current_index = request.current_index,
        next_index = request.next_index,
        debug_build = cfg!(debug_assertions));
    let _entered = span.enter();
    let AutoMixPrepareRequest {
        generation,
        transition_id,
        current_index,
        next_index,
        current_id,
        current_duration,
        current_source_path,
        next_song,
        next_identity,
        config,
    } = request;

    let result = prepare_track_blocking(
        config,
        current_duration,
        current_source_path,
        next_song,
        cancel,
    )
    .map(|mut prepared| {
        prepared.next_index = next_index;
        prepared.transition_id = transition_id;
        prepared.next_identity = next_identity;
        prepared
    });

    info!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        success = result.is_ok(),
        "AutoMix prepare finished"
    );
    AutoMixPrepareResult {
        generation,
        transition_id,
        current_index,
        current_id,
        result,
    }
}

fn prepare_track_blocking(
    config: AutoMixConfig,
    current_duration: Option<f64>,
    current_source_path: Option<PathBuf>,
    next_song: SongData,
    cancel: &PrepareCancel,
) -> Result<PreparedTrack, String> {
    let source = next_song
        .file_path()
        .ok_or_else(|| "AutoMix only supports local/url SongData for now".to_string())?
        .to_string();

    let (local_path, temp_file) = resolve_source_to_local_path(&source)?;
    if cancel.is_cancelled() {
        return Err(SUPERSEDED.into());
    }
    let audio_info = crate::metadata::extract_metadata_only(&local_path)
        .map_err(|e| format!("read next track metadata: {e}"))?;
    let display_info = build_display_info(&audio_info, &source);
    let quality = AudioQuality {
        sample_rate: audio_info.sample_rate,
        channels: audio_info.channels,
        bitrate: audio_info.bitrate_bps.map(|b| b as u32).unwrap_or_default(),
    };
    // Analyze the current + next tracks so the crossfade plan is driven by real
    // outro classification, mix-point selection, curve shaping, and loudness
    // matching instead of a blind "fade the last N seconds" plan.
    //
    // `run_prepare_request_blocking` runs off the audio callback. The runtime
    // bounds preparation to 60s, including source acquisition and analysis.
    // Any analysis failure degrades gracefully back to the time-based plan.
    let analyze_current_bpm = config.bpm_match && config.beat_align;
    let current_analysis = current_source_path
        .as_deref()
        .and_then(|path| analyze_cached("current", path, analyze_current_bpm, cancel));
    // Analyze with the same BPM policy as a "current" track so this result can be
    // reused straight from the cache when the next track later becomes current,
    // avoiding a second full decode of the same file.
    let next_analysis = if config.beat_align
        || config.volume_norm
        || config.smart_curve
        || config.emotional_transition
    {
        analyze_cached("next", &local_path, analyze_current_bpm, cancel)
    } else {
        None
    };
    if cancel.is_cancelled() {
        return Err(SUPERSEDED.into());
    }
    // The decoded length outranks the container's claim: metadata probed from
    // a stream can be short or absent, and a short one starts the mix early.
    let planning_duration = current_analysis
        .as_ref()
        .map(|analysis| analysis.duration as f64)
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .or(current_duration);
    let plan = planning_duration.map(|duration| {
        build_analysis_backed_plan(
            &config,
            duration,
            current_analysis.as_ref(),
            next_analysis.as_ref(),
        )
    });
    if let Some(plan) = plan.as_ref() {
        info!(
            playback_duration = ?current_duration,
            analyzed_duration = ?current_analysis.as_ref().map(|a| a.duration),
            trailing_silence = ?current_analysis.as_ref().map(|a| a.energy.trailing_silence),
            next_analyzed_duration = ?next_analysis.as_ref().map(|a| a.duration),
            tail_pulse = current_analysis.as_ref().and_then(|a| a.bpm.as_ref()).is_some_and(|b| b.tail.is_some()),
            head_pulse = next_analysis.as_ref().and_then(|a| a.bpm.as_ref()).is_some_and(|b| b.head.is_some()),
            plan = ?plan,
            "AutoMix plan computed"
        );
    }
    let duration = if audio_info.duration_secs > 0.0 {
        audio_info.duration_secs
    } else {
        next_analysis
            .as_ref()
            .map(|analysis| analysis.duration as f64)
            .filter(|duration| *duration > 0.0)
            .unwrap_or(display_info.duration)
    };

    Ok(PreparedTrack {
        song: next_song,
        current_id: None,
        local_path,
        _temp_file: temp_file,
        duration,
        plan,
        display_info,
        quality,
        next_index: 0,
        transition_id: None,
        next_identity: None,
    })
}

fn resolve_source_to_local_path(
    source: &str,
) -> Result<(PathBuf, Option<crate::source::TempGuard>), String> {
    if crate::source::is_http_url(source) {
        let temp = crate::source::download_to_temp_path(source).map_err(|e| e.to_string())?;
        let path = temp.to_path_buf();
        Ok((path, Some(temp)))
    } else {
        Ok((PathBuf::from(source), None))
    }
}

fn build_display_info(info: &AudioInfo, source: &str) -> DisplayAudioInfo {
    DisplayAudioInfo {
        name: extract_title_from_metadata(info).unwrap_or_else(|| {
            Path::new(source)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string()
        }),
        artist: extract_tag(info, &["artist", "TPE1"]).unwrap_or_default(),
        album: extract_tag(info, &["album", "TALB"]).unwrap_or_default(),
        duration: info.duration_secs,
        ..Default::default()
    }
}

fn extract_tag(info: &AudioInfo, keys: &[&str]) -> Option<String> {
    for (k, v) in &info.metadata_tags {
        for key in keys {
            if k.eq_ignore_ascii_case(key) {
                return Some(v.clone());
            }
        }
    }
    None
}

fn extract_title_from_metadata(info: &AudioInfo) -> Option<String> {
    extract_tag(info, &["title", "TIT2"])
}
