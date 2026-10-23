//! Native-owned FM recommendations. Network work reports through a channel;
//! the player remains the only writer of the reservoir and playback cursor.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tracing::{info, warn};

use crate::types::{AudioThreadEvent, NativePlaybackManifest, NativePlaybackMode, TrackIdentity};

use super::personal_fm::{self, PersonalFm};
use super::planner::PlannedTrack;
use super::planner_runtime::{PlannedStart, PlannerDirection};
use super::source_resolver::{self, ResolveError, ResolveErrorKind};
use super::{AudioPlayer, PlaybackIntent};

pub(super) enum FmResult {
    Batch {
        session_id: u64,
        generation: u64,
        result: Result<Vec<serde_json::Value>, ResolveError>,
    },
    Trash {
        session_id: u64,
        generation: u64,
        id: String,
        result: Result<(), ResolveError>,
    },
}

impl AudioPlayer {
    pub(super) async fn start_personal_fm(&mut self, seed: Option<serde_json::Value>) {
        if self.personal_fm.is_some() {
            self.publish_personal_fm().await;
            return;
        }
        self.retire_fm_audio().await;
        // Milliseconds stay exactly representable in JavaScript; monotonic within
        // this player even when two sessions begin in the same millisecond.
        self.fm_session_id = self.fm_session_id.saturating_add(1).max(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
        self.personal_fm = Some(PersonalFm::new(self.fm_session_id, seed, Instant::now()));
        self.source_cache.invalidate();
        self.playback_intent = PlaybackIntent::Playing;
        self.current_identity = None;
        self.pending_identity = None;
        self.rebuild_fm_manifest();
        self.publish_personal_fm().await;
        self.publish_now_playing().await;
        self.tick_personal_fm().await;
    }

    pub(super) async fn stop_personal_fm(&mut self, session_id: Option<u64>) {
        if !self
            .personal_fm
            .as_ref()
            .is_some_and(|fm| session_id.is_none_or(|id| fm.id == id))
        {
            return;
        }
        self.personal_fm = None;
        self.retire_fm_audio().await;
        self.playback_intent = PlaybackIntent::Paused;
        self.current_identity = None;
        self.pending_identity = None;
        self.source_cache.invalidate();
        self.manifest.clear(self.manifest.revision());
        self.planner = super::planner::Planner::new();
        self.publish_personal_fm().await;
        self.sync_ui().await;
    }

    pub(super) async fn set_personal_fm_playing(&mut self, playing: bool) {
        let Some(fm) = self.personal_fm.as_mut() else {
            return;
        };
        fm.set_playing(playing, Instant::now());
        let waiting = fm.waiting;
        self.playback_intent = if playing {
            PlaybackIntent::Playing
        } else {
            PlaybackIntent::Paused
        };
        if playing && !waiting && self.current_decoder_handle.is_some() {
            self.resume_audio_output().await;
        } else {
            self.output.writer().set_paused(true);
            if let Some(handle) = &self.current_decoder_handle {
                let _ = handle.set_paused(true);
            }
            if let Some(handle) = &self.secondary_decoder_handle {
                let _ = handle.set_paused(true);
            }
        }
        let audible = playing && !waiting && self.current_decoder_handle.is_some();
        self.publish_position_anchor(audible, self.clock_position())
            .await;
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::PlayStatus {
                is_playing: audible,
            })
            .await;
        self.publish_personal_fm().await;
        self.publish_now_playing().await;
        if playing {
            self.tick_personal_fm().await;
        }
    }

    pub(super) async fn advance_personal_fm(&mut self) {
        if self.personal_fm.is_none() {
            return;
        }
        self.retire_fm_audio().await;
        if let Some(fm) = &mut self.personal_fm {
            fm.waiting = true;
            fm.revision += 1;
            fm.request_refill(Instant::now());
        }
        self.publish_personal_fm().await;
        self.publish_now_playing().await;
        self.drive_personal_fm().await;
    }

    async fn retire_fm_audio(&mut self) {
        self.cancel_pending_output_refresh();
        self.automix_prepare_generation = self.automix_prepare_generation.wrapping_add(1);
        self.cancel_native_automix_runtime().await;
        if let Some(handle) = self.current_decoder_handle.take() {
            handle.stop();
        }
        self.decoder_playback_id = self.decoder_playback_id.wrapping_add(1);
        self.deck_mixer.clear_all();
        self.output.writer().set_paused(true);
        self.current_song = None;
        self.current_file_path = None;
        self.current_local_path = None;
        self.current_temp_file = None;
        self.playback_queue.set_playlist(Vec::new(), true);
        self.playlist.clear();
        self.load_in_flight = false;
        self.settle_announcement();
        self.begin_timeline();
        self.publish_position_anchor(false, 0.0).await;
        self.clear_session_track();
    }

    /// Append-only replenishment keeps the prepared source and its generation.
    /// Compaction/removal changes positional addresses and must invalidate them.
    pub(super) fn rebuild_fm_manifest(&mut self) {
        let Some(fm) = &self.personal_fm else { return };
        rebuild_manifest(
            fm,
            &mut self.manifest,
            &mut self.planner,
            &mut self.source_cache,
        );
        if let Some(identity) = &self.current_identity {
            if let Some(index) = self.manifest.position_of_key(&identity.key()) {
                self.current_play_index = index;
            }
        }
    }

    pub(super) fn fm_next_track(&mut self) -> Option<PlannedTrack> {
        let fm = self.personal_fm.as_ref()?;
        if !fm.desired_playing
            || fm.auth_failed
            || fm
                .source_deadline
                .is_some_and(|deadline| deadline > Instant::now())
        {
            return None;
        }
        if fm.current.is_none() {
            let entry = self.manifest.entry_at(0)?;
            return Some(PlannedTrack {
                identity: entry.identity.clone(),
                playlist_index: 0,
                position: 0,
            });
        }
        self.planner.peek_next(&mut self.manifest)
    }

    pub(super) async fn tick_personal_fm(&mut self) {
        let task_busy = self
            .fm_fetch_task
            .as_ref()
            .is_some_and(|task| !task.is_finished());
        if !task_busy && self.resolver_config.is_usable() {
            let request = self.personal_fm.as_mut().and_then(|fm| {
                let generation = fm.begin_fetch(Instant::now())?;
                Some((fm.id, generation))
            });
            if let Some((session_id, generation)) = request {
                let config = self.resolver_config.clone();
                let tx = self.fm_tx.clone();
                self.fm_fetch_task = Some(tokio::spawn(async move {
                    let result = tokio::task::spawn_blocking(move || {
                        source_resolver::fetch_personal_fm(&config)
                    })
                    .await
                    .unwrap_or_else(|err| Err(worker_error(err)));
                    let _ = tx.send(FmResult::Batch {
                        session_id,
                        generation,
                        result,
                    });
                }));
                self.publish_personal_fm().await;
            }
        }
        self.drive_personal_fm().await;
    }

    pub(super) async fn handle_fm_result(&mut self, result: FmResult) {
        match result {
            FmResult::Batch {
                session_id,
                generation,
                result,
            } => {
                let Some(fm) = self
                    .personal_fm
                    .as_mut()
                    .filter(|fm| fm.accepts(session_id, generation))
                else {
                    return;
                };
                match result {
                    Ok(rows) => {
                        let added = fm.ingest(rows);
                        fm.fetched(added, Instant::now());
                        info!(
                            "FM replenished: session={session_id} added={added} remaining={}",
                            fm.remaining()
                        );
                        if added > 0 {
                            self.rebuild_fm_manifest();
                        }
                    }
                    Err(err) => {
                        warn!(
                            "FM refill failed: {}",
                            source_resolver::redact(&err.message)
                        );
                        fm.defer(
                            source_resolver::redact(&err.message),
                            err.kind == ResolveErrorKind::Auth,
                            Instant::now(),
                        );
                    }
                }
                self.publish_personal_fm().await;
                self.publish_now_playing().await;
                self.drive_personal_fm().await;
            }
            FmResult::Trash {
                session_id,
                generation,
                id,
                result,
            } => {
                let Some(fm) = self
                    .personal_fm
                    .as_mut()
                    .filter(|fm| fm.accepts(session_id, generation))
                else {
                    return;
                };
                fm.trash_in_flight = None;
                let error = result
                    .as_ref()
                    .err()
                    .map(|err| source_resolver::redact(&err.message));
                let skip = result.is_ok()
                    && fm.current.as_ref().and_then(TrackIdentity::netease_id) == Some(id.as_str());
                if result.is_ok() {
                    fm.dislike(&id);
                    fm.request_refill(Instant::now());
                    self.remember_favourite(&id, false);
                    self.refresh_favourite_for_current_track();
                    self.rebuild_fm_manifest();
                }
                let _ = self
                    .emitter()
                    .emit(AudioThreadEvent::PersonalFmTrashResult {
                        session_id,
                        id,
                        error,
                    })
                    .await;
                self.publish_personal_fm().await;
                if skip {
                    self.advance_personal_fm().await;
                }
            }
        }
    }

    pub(super) async fn trash_personal_fm(&mut self, session_id: u64, id: String) {
        let Some(fm) = self.personal_fm.as_mut().filter(|fm| fm.id == session_id) else {
            return;
        };
        if fm.trash_in_flight.is_some()
            || self
                .fm_trash_task
                .as_ref()
                .is_some_and(|task| !task.is_finished())
        {
            return;
        }
        if !fm
            .tracks
            .iter()
            .any(|track| track.identity.netease_id() == Some(id.as_str()))
        {
            return;
        }
        if self
            .resolver_config
            .cookie
            .as_deref()
            .is_none_or(|cookie| cookie.trim().is_empty())
        {
            let _ = self
                .emitter()
                .emit(AudioThreadEvent::PersonalFmTrashResult {
                    session_id,
                    id,
                    error: Some("FM trash requires a signed-in account".into()),
                })
                .await;
            return;
        }
        fm.trash_in_flight = Some(id.clone());
        let generation = fm.generation;
        let config = self.resolver_config.clone();
        let tx = self.fm_tx.clone();
        self.fm_trash_task = Some(tokio::spawn(async move {
            let request_id = id.clone();
            let result = tokio::task::spawn_blocking(move || {
                source_resolver::trash_personal_fm(&request_id, &config)
            })
            .await
            .unwrap_or_else(|err| Err(worker_error(err)));
            let _ = tx.send(FmResult::Trash {
                session_id,
                generation,
                id,
                result,
            });
        }));
    }

    /// Unlike the ordinary planner fallback, FM waits for the asynchronous
    /// one-ahead resolve. Pause/stop/refill can still run while that request waits.
    async fn drive_personal_fm(&mut self) {
        let waiting = self
            .personal_fm
            .as_ref()
            .is_some_and(|fm| fm.waiting && fm.desired_playing);
        let Some(track) = self.fm_next_track() else {
            return;
        };
        if !waiting {
            self.prefetch_next_source();
            return;
        }
        let Some(source) = self.source_cache.take_for(track.position) else {
            self.prefetch_next_source();
            return;
        };
        if source.identity != track.identity {
            self.source_cache.invalidate();
            self.prefetch_next_source();
            return;
        }
        match self
            .start_planned_track(&track, source.uri, PlannerDirection::Next)
            .await
        {
            PlannedStart::Started => {}
            PlannedStart::OutputUnavailable => {
                if let Some(fm) = &mut self.personal_fm {
                    fm.defer_source("Audio output is unavailable".into(), false, Instant::now());
                }
                self.rebuild_fm_manifest();
                self.current_song = None;
                self.clear_session_track();
                self.publish_personal_fm().await;
                self.publish_now_playing().await;
            }
            PlannedStart::TrackFailed => {
                if let Some(fm) = &mut self.personal_fm {
                    fm.reject(&track.identity, Instant::now());
                    fm.source_deadline = Some(Instant::now() + Duration::from_secs(1));
                }
                self.current_song = None;
                self.clear_session_track();
                self.rebuild_fm_manifest();
                self.publish_personal_fm().await;
                self.publish_now_playing().await;
            }
        }
    }

    pub(super) async fn fm_prefetch_failed(&mut self, identity: TrackIdentity, err: ResolveError) {
        let Some(fm) = &mut self.personal_fm else {
            return;
        };
        if !err.is_retryable() && err.kind != ResolveErrorKind::Auth {
            fm.reject(&identity, Instant::now());
            fm.source_deadline = Some(Instant::now() + Duration::from_secs(1));
            self.rebuild_fm_manifest();
        } else {
            fm.defer_source(
                source_resolver::redact(&err.message),
                err.kind == ResolveErrorKind::Auth,
                Instant::now(),
            );
        }
        self.publish_personal_fm().await;
        self.publish_now_playing().await;
    }

    pub(super) async fn fm_source_ready(&mut self) {
        self.drive_personal_fm().await;
    }

    /// Called after decoder open, before LoadAudio/SyncStatus. The page must see
    /// the row before receiving a native adoption event naming that row.
    pub(super) async fn personal_fm_started(&mut self) {
        let Some(identity) = self.current_identity.clone() else {
            return;
        };
        let Some(fm) = &mut self.personal_fm else {
            return;
        };
        fm.started(&identity, Instant::now());
        self.rebuild_fm_manifest();
        self.publish_personal_fm().await;
    }

    pub(super) async fn publish_personal_fm(&self) {
        let snapshot = self.personal_fm.as_ref().map(PersonalFm::snapshot);
        {
            let mut session = self.session.lock();
            session.personal_fm = snapshot.clone();
            session.manifest_revision = self.manifest.revision();
        }
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::PersonalFmChanged { session: snapshot })
            .await;
    }
}

fn rebuild_manifest(
    fm: &PersonalFm,
    manifest: &mut super::manifest::ManifestStore,
    planner: &mut super::planner::Planner,
    cache: &mut super::source_cache::SourceCache,
) {
    let entries: Vec<_> = fm
        .tracks
        .iter()
        .enumerate()
        .map(|(index, track)| personal_fm::manifest_entry(track, index))
        .collect();
    let prefix_unchanged = manifest.entries().len() <= entries.len()
        && manifest
            .entries()
            .iter()
            .zip(&entries)
            .all(|(old, new)| old.identity == new.identity);
    let cursor = fm.current.clone();
    manifest.set(NativePlaybackManifest {
        schema_version: 1,
        revision: manifest.revision().saturating_add(1),
        entries,
        order: Vec::new(),
        cursor_identity: cursor.clone(),
        cursor_index: 0,
        mode: NativePlaybackMode::Normal,
        repeat_list: false,
        random_seed: None,
    });
    if !prefix_unchanged {
        cache.invalidate();
    }
    planner.set_enabled(true);
    planner.reset_for_new_manifest(manifest, cursor.as_ref().map(TrackIdentity::key).as_deref());
}

#[cfg(test)]
mod tests {
    use super::super::source_resolver::{ResolvedSource, SourceOrigin};
    use super::super::{
        manifest::ManifestStore,
        planner::Planner,
        source_cache::{PrefetchResult, SourceCache},
    };
    use super::*;
    use serde_json::json;

    #[test]
    fn replenishment_preserves_inflight_source_but_compaction_invalidates_it() {
        let now = Instant::now();
        let mut fm = PersonalFm::new(1, Some(json!({"id": 1})), now);
        fm.ingest(vec![json!({"id": 2})]);
        fm.started(&TrackIdentity::Netease { id: "1".into() }, now);
        let (mut manifest, mut planner, mut cache) =
            (ManifestStore::new(), Planner::new(), SourceCache::new());
        rebuild_manifest(&fm, &mut manifest, &mut planner, &mut cache);
        let generation = cache.begin_prefetch();
        let revision = manifest.revision();
        fm.ingest(vec![json!({"id": 3})]);
        rebuild_manifest(&fm, &mut manifest, &mut planner, &mut cache);
        assert!(cache.accept(PrefetchResult {
            generation,
            manifest_revision: revision,
            position: 1,
            outcome: Ok(ResolvedSource::remote(
                TrackIdentity::Netease { id: "2".into() },
                "https://example.test/2".into(),
                SourceOrigin::Ncm
            )),
        }));
        assert!(cache.has_fresh_for(1));
        fm.started(&TrackIdentity::Netease { id: "2".into() }, now);
        rebuild_manifest(&fm, &mut manifest, &mut planner, &mut cache);
        assert!(!cache.has_fresh_for(1));
        assert_eq!(
            planner.peek_next(&mut manifest).unwrap().identity.key(),
            "netease:3"
        );
    }

    #[test]
    fn exhausted_radio_does_not_wrap_and_appending_resumes_after_the_current_identity() {
        let now = Instant::now();
        let mut fm = PersonalFm::new(1, Some(json!({"id": 1})), now);
        fm.started(&TrackIdentity::Netease { id: "1".into() }, now);
        let (mut manifest, mut planner, mut cache) =
            (ManifestStore::new(), Planner::new(), SourceCache::new());
        rebuild_manifest(&fm, &mut manifest, &mut planner, &mut cache);
        assert!(!manifest.repeat_list());
        assert!(planner.peek_next(&mut manifest).is_none());
        fm.ingest(vec![json!({"id": 2})]);
        rebuild_manifest(&fm, &mut manifest, &mut planner, &mut cache);
        assert_eq!(
            planner.cursor(&manifest).unwrap().identity.key(),
            "netease:1"
        );
        assert_eq!(
            planner.peek_next(&mut manifest).unwrap().identity.key(),
            "netease:2"
        );
    }
}

fn worker_error(err: tokio::task::JoinError) -> ResolveError {
    ResolveError {
        kind: ResolveErrorKind::Transient,
        message: source_resolver::redact(&format!("FM worker failed: {err}")),
    }
}
