use std::path::{Path, PathBuf};

use tracing::warn;

use crate::decoder;
use crate::types::{
    AudioInfo, AudioQuality, AudioThreadEvent, AutoMixConfig, AutoMixNativeState, AutoMixStatus,
    CrossfadeCurve, DisplayAudioInfo, SongData,
};

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

    pub fn mark_failed(&mut self, error: String, current_index: usize) -> Vec<AudioThreadEvent> {
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
        next_song,
        config,
    } = request;

    let result = prepare_track_blocking(config, current_duration, next_song).map(|mut prepared| {
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
    let plan = current_duration.map(|duration| build_time_based_plan(&config, duration));
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
    let max_duration = (current_duration / 4.0).max(2.0);
    let duration = config.crossfade_duration.clamp(2.0, max_duration);
    let start_time = (current_duration - duration).max(0.0);

    CrossfadePlan {
        start_time,
        duration,
        incoming_gain_adjustment: 1.0,
        overlap_headroom_db: -1.2,
        curve: config.transition_style,
    }
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
