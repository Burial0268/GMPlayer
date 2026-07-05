use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use tracing::warn;

use crate::automix as automix_analysis;
use crate::decoder;
use crate::types::{
    AudioInfo, AudioQuality, AudioThreadEvent, AutoMixConfig, AutoMixNativeState, AutoMixStatus,
    CrossfadeCurve, DisplayAudioInfo, SongData,
};

const MIN_CROSSFADE_DURATION: f64 = 2.0;
const DEFAULT_OVERLAP_HEADROOM_DB: f64 = -1.2;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CrossfadePlan {
    pub start_time: f64,
    pub duration: f64,
    pub incoming_gain_adjustment: f64,
    pub overlap_headroom_db: f64,
    pub curve: CrossfadeCurve,
}

pub(super) struct PreparedTrack {
    song: SongData,
    current_id: Option<String>,
    transition_id: Option<u64>,
    #[allow(dead_code)]
    local_path: PathBuf,
    _temp_file: Option<tempfile::TempPath>,
    duration: f64,
    plan: Option<CrossfadePlan>,
    display_info: DisplayAudioInfo,
    quality: AudioQuality,
    next_index: usize,
}

pub struct PreparedAutoMixStart {
    pub song: SongData,
    pub transition_id: Option<u64>,
    pub local_path: PathBuf,
    pub _temp_file: Option<tempfile::TempPath>,
    pub plan: Option<CrossfadePlan>,
    pub analysis_duration: f64,
    pub display_info: DisplayAudioInfo,
    pub quality: AudioQuality,
    pub next_index: usize,
}

pub struct AutoMixManager {
    enabled: bool,
    config: AutoMixConfig,
    state: AutoMixNativeState,
    prepared: Option<PreparedTrack>,
    active_transition_id: Option<u64>,
    last_error: Option<String>,
}

pub(super) struct AutoMixPrepareRequest {
    pub generation: u64,
    pub transition_id: Option<u64>,
    pub current_index: usize,
    pub next_index: usize,
    pub current_id: String,
    pub current_duration: Option<f64>,
    pub current_source_path: Option<PathBuf>,
    pub next_song: SongData,
    pub config: AutoMixConfig,
}

pub(super) struct AutoMixPrepareResult {
    pub generation: u64,
    pub transition_id: Option<u64>,
    pub current_index: usize,
    pub current_id: String,
    pub result: Result<PreparedTrack, String>,
}

impl AutoMixManager {
    pub fn new() -> Self {
        let config = AutoMixConfig::default();
        Self {
            enabled: config.enabled,
            config,
            state: AutoMixNativeState::Idle,
            prepared: None,
            active_transition_id: None,
            last_error: None,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool, current_index: usize) -> Vec<AudioThreadEvent> {
        self.enabled = enabled;
        self.config.enabled = enabled;
        if !enabled {
            self.prepared = None;
            self.active_transition_id = None;
            self.state = AutoMixNativeState::Idle;
        }
        vec![AudioThreadEvent::AutomixStatus {
            status: self.status(current_index),
        }]
    }

    pub fn configure(
        &mut self,
        config: AutoMixConfig,
        current_index: usize,
    ) -> Vec<AudioThreadEvent> {
        self.enabled = config.enabled;
        self.config = config;
        if !self.enabled {
            self.prepared = None;
            self.active_transition_id = None;
            self.state = AutoMixNativeState::Idle;
        }
        vec![AudioThreadEvent::AutomixStatus {
            status: self.status(current_index),
        }]
    }

    pub fn cancel(&mut self, current_index: usize) -> Vec<AudioThreadEvent> {
        self.prepared = None;
        self.active_transition_id = None;
        self.state = AutoMixNativeState::Idle;
        self.last_error = None;
        vec![AudioThreadEvent::AutomixStatus {
            status: self.status(current_index),
        }]
    }

    #[allow(dead_code)]
    pub fn force_start(&mut self, current_index: usize) -> Vec<AudioThreadEvent> {
        let Some(prepared) = self.prepared.as_ref() else {
            self.state = AutoMixNativeState::Failed;
            self.last_error = Some("AutoMix has no prepared next track".into());
            return vec![
                AudioThreadEvent::AutomixError {
                    error: self.last_error.clone().unwrap_or_default(),
                    recoverable: true,
                },
                AudioThreadEvent::AutomixStatus {
                    status: self.status(current_index),
                },
            ];
        };

        let duration = prepared.plan.as_ref().map(|p| p.duration).unwrap_or(0.0);
        self.state = AutoMixNativeState::Crossfading;
        vec![
            AudioThreadEvent::AutomixCrossfadeStarted {
                from_id: String::new(),
                to_id: prepared.song.get_id(),
                duration,
                transition_id: prepared.transition_id,
            },
            AudioThreadEvent::AutomixStatus {
                status: self.status(current_index),
            },
        ]
    }

    pub fn take_prepared_for_start(&mut self) -> Option<PreparedAutoMixStart> {
        let prepared = self.prepared.take()?;
        self.state = AutoMixNativeState::Crossfading;
        Some(PreparedAutoMixStart {
            song: prepared.song,
            transition_id: prepared.transition_id,
            local_path: prepared.local_path,
            _temp_file: prepared._temp_file,
            plan: prepared.plan,
            analysis_duration: prepared.duration,
            display_info: prepared.display_info,
            quality: prepared.quality,
            next_index: prepared.next_index,
        })
    }

    pub fn take_prepared_for_preload(&mut self) -> Option<PreparedAutoMixStart> {
        let prepared = self.prepared.as_mut()?;
        Some(PreparedAutoMixStart {
            song: prepared.song.clone(),
            transition_id: prepared.transition_id,
            local_path: prepared.local_path.clone(),
            _temp_file: prepared._temp_file.take(),
            plan: prepared.plan.clone(),
            analysis_duration: prepared.duration,
            display_info: prepared.display_info.clone(),
            quality: prepared.quality.clone(),
            next_index: prepared.next_index,
        })
    }

    pub fn mark_failed(&mut self, error: String, current_index: usize) -> Vec<AudioThreadEvent> {
        self.prepared = None;
        self.state = AutoMixNativeState::Failed;
        self.active_transition_id = None;
        self.last_error = Some(error.clone());
        vec![
            AudioThreadEvent::AutomixError {
                error,
                recoverable: true,
            },
            AudioThreadEvent::AutomixStatus {
                status: self.status(current_index),
            },
        ]
    }

    pub fn complete(&mut self, current_index: usize) -> Vec<AudioThreadEvent> {
        self.prepared = None;
        self.active_transition_id = None;
        self.state = AutoMixNativeState::Idle;
        self.last_error = None;
        vec![AudioThreadEvent::AutomixStatus {
            status: self.status(current_index),
        }]
    }

    pub fn begin_prepare_next(
        &mut self,
        generation: u64,
        transition_id: Option<u64>,
        current_index: usize,
        next_index: usize,
        current_song: Option<SongData>,
        current_duration: Option<f64>,
        current_source_path: Option<PathBuf>,
        next_song: SongData,
    ) -> (Vec<AudioThreadEvent>, Option<AutoMixPrepareRequest>) {
        if !self.enabled {
            self.state = AutoMixNativeState::Idle;
            self.active_transition_id = None;
            return (
                vec![AudioThreadEvent::AutomixStatus {
                    status: self.status(current_index),
                }],
                None,
            );
        }

        self.state = AutoMixNativeState::Preparing;
        self.active_transition_id = transition_id;
        self.last_error = None;
        let current_id = current_song
            .as_ref()
            .map(SongData::get_id)
            .unwrap_or_default();
        let request = AutoMixPrepareRequest {
            generation,
            transition_id,
            current_index,
            next_index,
            current_id,
            current_duration,
            current_source_path,
            next_song,
            config: self.config.clone(),
        };

        (
            vec![AudioThreadEvent::AutomixStatus {
                status: self.status(current_index),
            }],
            Some(request),
        )
    }

    pub fn finish_prepare(
        &mut self,
        prepare_result: AutoMixPrepareResult,
        current_index: usize,
    ) -> Vec<AudioThreadEvent> {
        let current_id = prepare_result.current_id.clone();
        let transition_id = prepare_result.transition_id;
        let prepared = match prepare_result.result {
            Ok(mut prepared) => {
                prepared.current_id = if current_id.is_empty() {
                    None
                } else {
                    Some(current_id.clone())
                };
                prepared.transition_id = transition_id;
                prepared
            }
            Err(err) => {
                warn!("AutoMix prepare failed: {err}");
                self.prepared = None;
                self.state = AutoMixNativeState::Failed;
                self.active_transition_id = None;
                self.last_error = Some(err.clone());
                return vec![
                    AudioThreadEvent::AutomixError {
                        error: err,
                        recoverable: true,
                    },
                    AudioThreadEvent::AutomixStatus {
                        status: self.status(current_index),
                    },
                ];
            }
        };

        let next_id = prepared.song.get_id();
        self.prepared = Some(prepared);
        self.active_transition_id = transition_id;
        self.state = AutoMixNativeState::Waiting;

        vec![
            AudioThreadEvent::AutomixAnalysisReady {
                current_id,
                next_id,
                transition_id,
            },
            AudioThreadEvent::AutomixStatus {
                status: self.status(current_index),
            },
        ]
    }

    pub fn status(&self, current_index: usize) -> AutoMixStatus {
        let prepared = self.prepared.as_ref();
        AutoMixStatus {
            state: self.state,
            enabled: self.enabled,
            transition_id: prepared
                .and_then(|p| p.transition_id)
                .or(self.active_transition_id),
            current_index,
            next_index: prepared.map(|p| p.next_index),
            current_id: prepared.and_then(|p| p.current_id.clone()),
            next_id: prepared.map(|p| p.song.get_id()),
            crossfade_start: prepared.and_then(|p| p.plan.as_ref().map(|plan| plan.start_time)),
            crossfade_duration: prepared.and_then(|p| p.plan.as_ref().map(|plan| plan.duration)),
            error: self.last_error.clone(),
        }
    }
}

pub(super) fn run_prepare_request_blocking(request: AutoMixPrepareRequest) -> AutoMixPrepareResult {
    let AutoMixPrepareRequest {
        generation,
        transition_id,
        current_index,
        next_index,
        current_id,
        current_duration,
        current_source_path,
        next_song,
        config,
    } = request;

    let result = prepare_track_blocking(config, current_duration, current_source_path, next_song)
        .map(|mut prepared| {
            prepared.next_index = next_index;
            prepared.transition_id = transition_id;
            prepared
        });

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
) -> Result<PreparedTrack, String> {
    let source = next_song
        .file_path()
        .ok_or_else(|| "AutoMix only supports local/url SongData for now".to_string())?
        .to_string();

    let (local_path, temp_file) = resolve_source_to_local_path(&source)?;
    let audio_info = decoder::symphonia::extract_metadata_only(&local_path)
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
    // This is safe to run here: `run_prepare_request_blocking` executes on a
    // `spawn_blocking` thread (never the audio callback) under a 60s prepare
    // timeout, and prepare is triggered with a large lead time before the
    // crossfade window. Two full-track analyses complete well within that budget.
    // Any analysis failure degrades gracefully back to the time-based plan.
    let analyze_current_bpm = config.beat_align;
    let current_analysis = current_source_path
        .as_deref()
        .and_then(|path| analyze_cached("current", path, analyze_current_bpm));
    // Analyze with the same BPM policy as a "current" track so this result can be
    // reused straight from the cache when the next track later becomes current,
    // avoiding a second full decode of the same file.
    let next_analysis = if config.volume_norm || config.smart_curve {
        analyze_cached("next", &local_path, analyze_current_bpm)
    } else {
        None
    };
    let plan = current_duration.map(|duration| {
        build_analysis_backed_plan(
            &config,
            duration,
            current_analysis.as_ref(),
            next_analysis.as_ref(),
        )
    });
    let duration = if audio_info.duration_secs > 0.0 {
        audio_info.duration_secs
    } else {
        display_info.duration
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
    })
}

fn build_time_based_plan(config: &AutoMixConfig, current_duration: f64) -> CrossfadePlan {
    let max_duration = (current_duration / 4.0).max(MIN_CROSSFADE_DURATION);
    let duration = config
        .crossfade_duration
        .clamp(MIN_CROSSFADE_DURATION, max_duration);
    let start_time = (current_duration - duration).max(0.0);

    CrossfadePlan {
        start_time,
        duration,
        incoming_gain_adjustment: 1.0,
        overlap_headroom_db: DEFAULT_OVERLAP_HEADROOM_DB,
        curve: config.transition_style,
    }
}

fn analyze_optional(
    label: &str,
    path: &Path,
    analyze_bpm: bool,
) -> Option<automix_analysis::TrackAnalysis> {
    match automix_analysis::analyze_audio_file(path, analyze_bpm) {
        Ok(analysis) => Some(analysis),
        Err(err) => {
            warn!("AutoMix {label} analysis failed: {err}");
            None
        }
    }
}

// ─── Analysis cache ────────────────────────────────────────────────
//
// A track is analyzed when it is the "next" candidate and again once it becomes
// "current" on the following prepare — a guaranteed double-decode in steady-state
// playback. Repeated prepares (playlist edits, "add to play next") compound it.
// Cache the *analysis* (a few KB of Vecs; the ~40MB PCM is already discarded) so
// each distinct file is decoded at most once. Keyed by path + size + mtime, so a
// replaced file invalidates automatically.

const ANALYSIS_CACHE_CAP: usize = 8;

struct CachedTrackAnalysis {
    key: String,
    analysis: automix_analysis::TrackAnalysis,
}

fn analysis_cache() -> &'static Mutex<Vec<CachedTrackAnalysis>> {
    static CACHE: OnceLock<Mutex<Vec<CachedTrackAnalysis>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

fn analysis_cache_key(path: &Path) -> String {
    let meta = std::fs::metadata(path).ok();
    let len = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let mtime = meta
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}|{len}|{mtime}", path.display())
}

/// Cache-backed wrapper around `analyze_optional`. A cached entry only satisfies
/// a request that does not need BPM, or one whose cached analysis already carries
/// BPM — otherwise it is recomputed and replaces the stale entry.
fn analyze_cached(
    label: &str,
    path: &Path,
    analyze_bpm: bool,
) -> Option<automix_analysis::TrackAnalysis> {
    let key = analysis_cache_key(path);

    if let Ok(cache) = analysis_cache().lock() {
        if let Some(entry) = cache.iter().find(|entry| entry.key == key) {
            if !analyze_bpm || entry.analysis.bpm.is_some() {
                return Some(entry.analysis.clone());
            }
        }
    }

    let analysis = analyze_optional(label, path, analyze_bpm)?;

    if let Ok(mut cache) = analysis_cache().lock() {
        cache.retain(|entry| entry.key != key);
        cache.push(CachedTrackAnalysis {
            key,
            analysis: analysis.clone(),
        });
        let overflow = cache.len().saturating_sub(ANALYSIS_CACHE_CAP);
        if overflow > 0 {
            cache.drain(0..overflow);
        }
    }

    Some(analysis)
}

fn build_analysis_backed_plan(
    config: &AutoMixConfig,
    current_duration: f64,
    current_analysis: Option<&automix_analysis::TrackAnalysis>,
    next_analysis: Option<&automix_analysis::TrackAnalysis>,
) -> CrossfadePlan {
    let mut plan = build_time_based_plan(config, current_duration);

    if config.volume_norm {
        if let Some(next) = next_analysis {
            plan.incoming_gain_adjustment = next.volume.gain_adjustment as f64;
        }
    }

    let Some(current) = current_analysis else {
        return plan;
    };

    let trailing_silence = current.energy.trailing_silence as f64;
    let effective_end = (current_duration - trailing_silence)
        .max(0.0)
        .min(current_duration);
    if effective_end <= MIN_CROSSFADE_DURATION {
        return plan;
    }

    plan.duration = choose_analysis_duration(config, current, effective_end);
    plan.curve = choose_analysis_curve(config, current);
    plan.overlap_headroom_db = choose_overlap_headroom(current_analysis, next_analysis);

    let baseline_start =
        analysis_baseline_start(current, current_duration, effective_end, plan.duration)
            .unwrap_or(plan.start_time);
    let candidate_start = selected_mix_candidate_start(config, current);
    let start = if let Some(candidate_start) = candidate_start {
        let max_shift = plan.duration.max(8.0).min(20.0);
        if (candidate_start - baseline_start).abs() <= max_shift {
            candidate_start
        } else {
            baseline_start
        }
    } else {
        baseline_start
    };

    plan.start_time = start;
    if config.beat_align && !skip_native_beat_align(current) {
        if let Some(bpm) = current.bpm.as_ref() {
            plan.start_time = align_to_nearest_beat(plan.start_time, bpm);
        }
    }

    clamp_plan_to_content(plan, effective_end)
}

fn choose_analysis_duration(
    config: &AutoMixConfig,
    current: &automix_analysis::TrackAnalysis,
    effective_end: f64,
) -> f64 {
    let max_duration = (effective_end / 4.0).max(MIN_CROSSFADE_DURATION);
    let configured = config
        .crossfade_duration
        .clamp(MIN_CROSSFADE_DURATION, max_duration);

    if !config.smart_curve {
        return configured;
    }

    let Some(outro) = current.outro.as_ref() else {
        return configured;
    };

    match outro.outro_type.as_str() {
        "hard" if outro.outro_confidence >= 0.6 => configured.min(3.0).max(MIN_CROSSFADE_DURATION),
        "silence" if outro.outro_confidence >= 0.6 => {
            configured.min(4.0).max(MIN_CROSSFADE_DURATION)
        }
        "reverbTail" if outro.outro_confidence >= 0.6 => configured.min(5.0),
        "sustained" if outro.outro_confidence >= 0.6 => configured.min(6.0),
        "musicalOutro" if outro.outro_confidence >= 0.6 => configured.min(7.0),
        _ => configured,
    }
}

fn selected_mix_candidate_start(
    config: &AutoMixConfig,
    current: &automix_analysis::TrackAnalysis,
) -> Option<f64> {
    let mix = current.mix_candidates.as_ref()?;
    let selected = mix.candidates.iter().max_by(|a, b| {
        adjusted_mix_score(config, a)
            .partial_cmp(&adjusted_mix_score(config, b))
            .unwrap_or(std::cmp::Ordering::Equal)
    })?;

    if adjusted_mix_score(config, selected) >= 0.35 {
        Some(selected.time as f64)
    } else {
        None
    }
}

fn adjusted_mix_score(
    config: &AutoMixConfig,
    candidate: &automix_analysis::MixPointCandidate,
) -> f32 {
    if config.vocal_guard {
        candidate.score
    } else {
        (candidate.score + candidate.vocal_risk * 0.38).clamp(0.0, 1.0)
    }
}

fn choose_analysis_curve(
    config: &AutoMixConfig,
    current: &automix_analysis::TrackAnalysis,
) -> CrossfadeCurve {
    if !config.smart_curve {
        return config.transition_style;
    }

    let Some(outro) = current.outro.as_ref() else {
        return config.transition_style;
    };
    if outro.outro_confidence < 0.6 {
        return config.transition_style;
    }

    match outro.outro_type.as_str() {
        "slowDown" | "sustained" | "reverbTail" => CrossfadeCurve::SCurve,
        _ => CrossfadeCurve::EqualPower,
    }
}

fn choose_overlap_headroom(
    current_analysis: Option<&automix_analysis::TrackAnalysis>,
    next_analysis: Option<&automix_analysis::TrackAnalysis>,
) -> f64 {
    let Some(current) = current_analysis else {
        return DEFAULT_OVERLAP_HEADROOM_DB;
    };
    let Some(next) = next_analysis else {
        return DEFAULT_OVERLAP_HEADROOM_DB;
    };

    let similarity = spectral_similarity(&current.fingerprint.bands, &next.fingerprint.bands);
    if similarity < 0.45 {
        -2.0
    } else if similarity < 0.65 {
        -1.6
    } else {
        DEFAULT_OVERLAP_HEADROOM_DB
    }
}

fn spectral_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom <= 0.000001 {
        0.0
    } else {
        (dot / denom).clamp(0.0, 1.0)
    }
}

fn analysis_baseline_start(
    current: &automix_analysis::TrackAnalysis,
    current_duration: f64,
    effective_end: f64,
    duration: f64,
) -> Option<f64> {
    if let Some(outro) = current.outro.as_ref() {
        if outro.outro_confidence >= 0.55 {
            return Some(outro.suggested_crossfade_start as f64);
        }
    }

    let energy = &current.energy;
    if energy.is_fade_out {
        let outro_content_duration = energy.outro_start_offset - energy.trailing_silence;
        let fade_in_point = (outro_content_duration.max(0.0) * 0.5) as f64;
        return Some((current_duration - energy.trailing_silence as f64 - fade_in_point).max(0.0));
    }

    if energy.outro_start_offset > 0.0 {
        return Some((current_duration - energy.outro_start_offset as f64).max(0.0));
    }

    Some((effective_end - duration).max(0.0))
}

fn skip_native_beat_align(current: &automix_analysis::TrackAnalysis) -> bool {
    let Some(outro) = current.outro.as_ref() else {
        return false;
    };

    matches!(
        outro.outro_type.as_str(),
        "fadeOut" | "reverbTail" | "sustained" | "loopFade"
    )
}

fn align_to_nearest_beat(start_time: f64, bpm: &automix_analysis::BPMResult) -> f64 {
    if bpm.beat_grid.is_empty() || bpm.bpm <= 0.0 {
        return start_time;
    }

    let beat_interval = (60.0 / bpm.bpm).max(0.1) as f64;
    let max_shift = (beat_interval * 0.5).min(0.6);
    let mut best = start_time;
    let mut best_diff = f64::INFINITY;

    for &beat in &bpm.beat_grid {
        let time = (beat + bpm.analysis_offset) as f64;
        let diff = (time - start_time).abs();
        if diff < best_diff {
            best_diff = diff;
            best = time;
        }
    }

    if best_diff <= max_shift {
        best
    } else {
        start_time
    }
}

fn clamp_plan_to_content(mut plan: CrossfadePlan, effective_end: f64) -> CrossfadePlan {
    if effective_end <= 0.0 {
        plan.start_time = 0.0;
        plan.duration = MIN_CROSSFADE_DURATION;
        return plan;
    }

    let latest_start = (effective_end - MIN_CROSSFADE_DURATION).max(0.0);
    plan.start_time = plan.start_time.clamp(0.0, latest_start);
    let remaining = (effective_end - plan.start_time).max(0.0);
    if remaining < plan.duration {
        plan.duration = remaining.max(0.5);
    }
    plan.duration = plan.duration.max(0.5);
    plan
}

fn resolve_source_to_local_path(
    source: &str,
) -> Result<(PathBuf, Option<tempfile::TempPath>), String> {
    if decoder::is_http_url(source) {
        let temp = decoder::download_to_temp_path(source).map_err(|e| e.to_string())?;
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
