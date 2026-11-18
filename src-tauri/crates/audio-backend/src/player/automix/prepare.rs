//! Blocking source acquisition, metadata and analysis preparation.

use super::beat_plan::EXIT_WINDOW;
use super::cache::analyze_cached;
use super::plan::build_analysis_backed_plan;
use super::PreparedTrack;
use crate::types::{
    AudioInfo, AudioQuality, AutoMixConfig, DisplayAudioInfo, SongData, TrackIdentity,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{info, info_span};

pub(in crate::player) struct AutoMixPrepareRequest {
    pub generation: u64,
    pub transition_id: Option<u64>,
    pub current_index: usize,
    pub next_index: usize,
    pub current_id: String,
    pub current_duration: Option<f64>,
    /// Backend clock position when the request was made: bounds how long analysis may wait for
    /// a download to land.
    pub current_position: Option<f64>,
    /// The outgoing deck's gain now (`active_norm_gain`): what B's gain is matched against.
    pub outgoing_gain: f32,
    pub current_source_path: Option<PathBuf>,
    pub next_song: SongData,
    pub next_identity: Option<TrackIdentity>,
    pub config: AutoMixConfig,
}

/// Tells a blocking prepare it was superseded. The task cannot be aborted, so
/// it polls this at its checkpoints and stops decoding instead of finishing a
/// track nobody will use.
#[derive(Clone)]
pub(in crate::player) struct PrepareCancel {
    live: Arc<AtomicU64>,
    generation: u64,
    /// Set when the runtime stopped waiting for this request (its 60 s bound). Nothing bumps the
    /// generation then, so without it the task would keep its download and the analysis gate.
    abandoned: Arc<AtomicBool>,
}

impl PrepareCancel {
    pub(in crate::player) fn new(live: Arc<AtomicU64>, generation: u64) -> Self {
        Self {
            live,
            generation,
            abandoned: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(in crate::player) fn abandon(&self) {
        self.abandoned.store(true, Ordering::Release);
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.abandoned.load(Ordering::Acquire)
            || self.live.load(Ordering::Acquire) != self.generation
    }
}

/// Longest a prepare waits for a download to land before analysing it.
const MAX_ANALYSIS_WAIT: f64 = 20.0;
/// Kept free between the end of that wait and where the fade could start.
const ANALYSIS_WAIT_MARGIN: f64 = 8.0;

/// Until when analysis may wait for a still-downloading source, or `None` for not at all.
///
/// A remote track is played while it downloads (`source::download_to_temp_path` returns on the
/// headers), and the prepare runs at the start of the track, so an analysis that only accepted a
/// whole file never saw the incoming track. Waiting is cheap — a download is usually seconds and
/// the fade minutes away — but must end before the fade could start: a beat-matched or
/// section-cued plan may start up to `EXIT_WINDOW` before the end, not just one crossfade length.
fn analysis_deadline(
    config: &AutoMixConfig,
    current_duration: Option<f64>,
    current_position: Option<f64>,
) -> Option<std::time::Instant> {
    let remaining = current_duration? - current_position?;
    let earliest_fade = if config.emotional_transition || (config.bpm_match && config.beat_align) {
        config.crossfade_duration.max(EXIT_WINDOW)
    } else {
        config.crossfade_duration
    };
    let budget = (remaining - earliest_fade - ANALYSIS_WAIT_MARGIN).min(MAX_ANALYSIS_WAIT);
    (budget > 0.0).then(|| std::time::Instant::now() + std::time::Duration::from_secs_f64(budget))
}

const SUPERSEDED: &str = "AutoMix prepare superseded";

/// Where a song's outro begins, for the UI's transition lead-in: the last section the section
/// analysis calls an outro, else the classifier's distinct outro section, else where the energy
/// last drops. Mirrored by `outroStartOf` in TransitionStateMachine.ts; keep the two in step.
pub(in crate::player) fn outro_start_of(analysis: &crate::automix::TrackAnalysis) -> f64 {
    let from_sections = analysis.sections.as_ref().and_then(|sections| {
        sections
            .sections
            .iter()
            .rev()
            .find(|section| matches!(section.section_type, crate::automix::SongSectionKind::Outro))
            .map(|section| section.start)
    });
    let from_classifier = analysis
        .outro
        .as_ref()
        .and_then(|outro| outro.outro_section_start);
    from_sections
        .or(from_classifier)
        .unwrap_or(analysis.duration - analysis.energy.outro_start_offset)
        .max(0.0) as f64
}

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
        current_position,
        outgoing_gain,
        current_source_path,
        next_song,
        next_identity,
        config,
    } = request;
    let wait_until = analysis_deadline(&config, current_duration, current_position);

    let result = prepare_track_blocking(
        config,
        current_duration,
        current_source_path,
        next_song,
        wait_until,
        outgoing_gain,
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
    wait_until: Option<std::time::Instant>,
    outgoing_gain: f32,
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
    let quality = AudioQuality::from(&audio_info);
    // Analyze the current + next tracks so the crossfade plan is driven by real
    // outro classification, mix-point selection, curve shaping, and loudness
    // matching instead of a blind "fade the last N seconds" plan.
    //
    // `run_prepare_request_blocking` runs off the audio callback. The runtime
    // bounds preparation to 60s, including source acquisition, waiting for a
    // download to land (`analysis_deadline`) and analysis. Any analysis failure
    // degrades gracefully back to the time-based plan.
    let analyze_current_bpm = config.bpm_match && config.beat_align;
    let current_analysis = current_source_path
        .as_deref()
        .and_then(|path| analyze_cached("current", path, analyze_current_bpm, wait_until, cancel));
    // Analyze with the same BPM policy as a "current" track so this result can be
    // reused straight from the cache when the next track later becomes current,
    // avoiding a second full decode of the same file.
    // The curve is always chosen from what overlaps, so B is always analysed.
    let next_analysis =
        analyze_cached("next", &local_path, analyze_current_bpm, wait_until, cancel);
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
            f64::from(outgoing_gain),
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
        outro_start: current_analysis.as_ref().map(outro_start_of),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_seconds(config: &AutoMixConfig, duration: f64, position: f64) -> Option<f64> {
        let now = std::time::Instant::now();
        analysis_deadline(config, Some(duration), Some(position))
            .map(|deadline| deadline.duration_since(now).as_secs_f64())
    }

    #[test]
    fn analysis_waits_only_while_the_fade_is_still_ahead() {
        let plain = AutoMixConfig {
            bpm_match: false,
            emotional_transition: false,
            ..AutoMixConfig::default()
        };
        // Track start: capped at the maximum wait.
        let at_start = wait_seconds(&plain, 240.0, 0.0).unwrap();
        assert!((at_start - MAX_ANALYSIS_WAIT).abs() < 0.1, "{at_start}");
        // Near the end: whatever is left before the fade and its margin.
        let late = wait_seconds(&plain, 240.0, 215.0).unwrap();
        assert!(
            (late - (25.0 - 8.0 - ANALYSIS_WAIT_MARGIN)).abs() < 0.1,
            "{late}"
        );
        // Past that: no wait at all, plan as before.
        assert_eq!(wait_seconds(&plain, 240.0, 228.0), None);
        // A beat-matched or section-cued plan may start up to EXIT_WINDOW early.
        assert_eq!(wait_seconds(&AutoMixConfig::default(), 240.0, 190.0), None);
        // Unknown length or position: no basis for a deadline.
        assert_eq!(analysis_deadline(&plain, None, Some(0.0)), None);
        assert_eq!(analysis_deadline(&plain, Some(240.0), None), None);
    }
}
