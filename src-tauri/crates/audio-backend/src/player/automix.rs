//! AutoMix state and prepared resources. Planning and blocking I/O live in child modules.

use crate::types::{
    AudioQuality, AudioThreadEvent, AutoMixConfig, AutoMixNativeState, AutoMixStatus,
    DisplayAudioInfo, SongData, TrackIdentity,
};
use std::path::PathBuf;
use tracing::warn;

mod beat_plan;
mod cache;
mod plan;
mod prepare;
#[cfg(test)]
mod tests;

pub use plan::CrossfadePlan;
pub(super) use prepare::{
    run_prepare_request_blocking, AutoMixPrepareRequest, AutoMixPrepareResult, PrepareCancel,
};

pub(super) struct PreparedTrack {
    song: SongData,
    current_id: Option<String>,
    transition_id: Option<u64>,
    #[allow(dead_code)]
    local_path: PathBuf,
    _temp_file: Option<crate::source::TempGuard>,
    duration: f64,
    plan: Option<CrossfadePlan>,
    display_info: DisplayAudioInfo,
    quality: AudioQuality,
    next_index: usize,
    next_identity: Option<TrackIdentity>,
}

pub struct PreparedAutoMixStart {
    pub song: SongData,
    pub transition_id: Option<u64>,
    pub local_path: PathBuf,
    pub _temp_file: Option<crate::source::TempGuard>,
    pub plan: Option<CrossfadePlan>,
    pub analysis_duration: f64,
    pub display_info: DisplayAudioInfo,
    pub quality: AudioQuality,
    pub next_index: usize,
    pub next_identity: Option<TrackIdentity>,
}

pub struct AutoMixManager {
    enabled: bool,
    config: AutoMixConfig,
    state: AutoMixNativeState,
    prepared: Option<PreparedTrack>,
    active_transition_id: Option<u64>,
    last_error: Option<String>,
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
            self.clear_plan();
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
        if !self.enabled || self.state == AutoMixNativeState::Failed {
            self.clear_plan();
        }
        vec![AudioThreadEvent::AutomixStatus {
            status: self.status(current_index),
        }]
    }

    pub(super) fn clear_plan(&mut self) {
        self.prepared = None;
        self.active_transition_id = None;
        self.state = AutoMixNativeState::Idle;
        self.last_error = None;
    }

    pub fn cancel(&mut self, current_index: usize) -> Vec<AudioThreadEvent> {
        let transition_id = self.status(current_index).transition_id;
        self.clear_plan();
        let mut status = self.status(current_index);
        // The cancellation retires this attempt; subsequent idle snapshots have no id.
        status.transition_id = transition_id;
        vec![AudioThreadEvent::AutomixStatus { status }]
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
        // The mixer is armed before playback reaches the cue; keep Waiting metadata.
        self.take_prepared_for_preload()
    }

    pub(super) fn mark_started(&mut self) {
        self.state = AutoMixNativeState::Crossfading;
    }

    pub(super) fn rebind_prepared(&mut self, next_index: usize, transition_id: Option<u64>) {
        self.active_transition_id = transition_id;
        if let Some(prepared) = self.prepared.as_mut() {
            prepared.next_index = next_index;
            prepared.transition_id = transition_id;
        }
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
            next_identity: prepared.next_identity.clone(),
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
        next_identity: Option<TrackIdentity>,
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
            next_identity,
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
