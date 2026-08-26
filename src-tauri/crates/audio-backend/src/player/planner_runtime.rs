//! Planner runtime: the `AudioPlayer` half of manifest-driven advancement.
//!
//! Responsibilities:
//! - own the `ManifestStore` / `Planner` / `SourceCache` triple
//! - prefetch the one-ahead source off the player loop (`spawn_blocking`)
//! - advance to the prepared source when a track finishes
//! - publish authoritative `NativePlannerStatus` for frontend reconciliation
//!
//! Everything here runs on the player's control path. Resolution and download
//! happen on blocking workers; no HTTP, JSON or disk I/O touches an audio
//! callback or the mixer hot loop.

use tracing::{info, warn};

use crate::types::{
    AudioThreadEvent, NativeManifestEntry, NativePlannerPlaybackState, NativePlannerStatus,
    NativePlaybackManifest, NativeResolverConfig, SongData,
};

use super::planner::PlannedTrack;
use super::source_cache::PrefetchResult;
use super::source_resolver::{self, ResolveError, ResolveErrorKind};
use super::{AudioPlayer, PlaybackIntent};

/// Whether a failure means "this track is gone" rather than "the network was
/// briefly unavailable". Only permanent failures blacklist a position; a
/// transient one must stay retryable or a passing network blip would silently
/// remove tracks from the rotation.

/// Which way a planner-driven transition walks the traversal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlannerDirection {
    Next,
    Prev,
}

impl AudioPlayer {
    // ── Manifest lifecycle ───────────────────────────────────────

    pub(super) async fn apply_native_manifest(&mut self, manifest: NativePlaybackManifest) {
        let revision = manifest.revision;
        if !self.manifest.set(manifest) {
            info!(
                "ignoring stale native manifest: incoming={} stored={}",
                revision,
                self.manifest.revision()
            );
            // Always report the authoritative revision back. A frontend whose
            // counter fell behind (WebView reload, cleared storage) would
            // otherwise keep publishing rejected manifests forever with no
            // signal that anything was wrong.
            self.emit_planner_status().await;
            return;
        }

        self.source_cache.invalidate();
        // What is *loaded* outranks what the manifest declares, in both the
        // planner cursor and `current_identity`.
        //
        // The declared cursor is a frontend store snapshot, and on Android that
        // snapshot is routinely older than the backend's own advances: the
        // WebView is frozen while the planner keeps hopping, then thaws and
        // publishes a manifest whose cursor still names a track that finished
        // minutes ago. Adopting it unconditionally repointed `current_identity`
        // at a neighbouring song — and the media-session projection resolves
        // its title, artist and artwork through exactly that identity, so the
        // lock screen showed one track while another played, updating cleanly
        // to the *wrong* metadata on every republish. It also dragged the
        // planner cursor backwards, replaying the tracks in between.
        //
        // `current_identity` is `None` precisely when nothing authoritative
        // knows what is loaded — a frontend-driven load clears it through
        // `invalidate_planner_anchor` for this very handshake — and that is the
        // one case where the frontend's declaration wins.
        let playing_key = self.current_planner_key();
        self.planner.reset_for_new_manifest(&self.manifest, playing_key.as_deref());

        if self.current_identity.is_none() {
            if let Some(track) = self.planner.cursor(&self.manifest) {
                self.current_identity = Some(track.identity);
            }
        }

        info!(
            "native manifest applied: revision={} entries={} mode={:?}",
            self.manifest.revision(),
            self.manifest.len(),
            self.manifest.mode()
        );

        self.emit_planner_status().await;
        // The manifest is the media session's metadata source, so a republish
        // can change what the notification should say about the *current* track
        // — a title that arrived late, cover art the frontend only just had.
        // Free when it does not: the bridge dedups on `same_metadata`, and this
        // runs once per publish, not per position update.
        if self.current_song.is_some() {
            self.publish_now_playing().await;
        }
        self.prefetch_next_source();
    }

    pub(super) async fn clear_native_manifest(&mut self, revision: u64) {
        if !self.manifest.clear(revision) {
            return;
        }
        self.source_cache.invalidate();
        self.planner = super::planner::Planner::new();
        self.emit_planner_status().await;
    }

    pub(super) fn set_native_resolver_config(&mut self, config: NativeResolverConfig) {
        let usable = config.is_usable();
        let credentials_changed = self.resolver_config.cookie != config.cookie
            || self.resolver_config.user_id != config.user_id;
        self.resolver_config = config;
        // Credentials changed — anything prepared under the old ones may be
        // wrong quality or outright unplayable.
        self.source_cache.invalidate();
        info!("native resolver config updated: usable={usable}");
        if credentials_changed {
            // Login, logout or account switch: the like list belongs to the
            // account, so the old one is not just stale but wrong. Refetching
            // here is what lets the notification's heart be correct with no page
            // alive — the frontend's push is a fast path, not the only source.
            self.refresh_likelist();
            self.refresh_favourite_for_current_track();
        }
        if usable {
            self.prefetch_next_source();
        }
    }

    pub(super) async fn set_native_planner_enabled(&mut self, enabled: bool) {
        if self.planner.is_enabled() == enabled {
            return;
        }
        self.planner.set_enabled(enabled);
        if !enabled {
            self.source_cache.invalidate();
        } else {
            self.prefetch_next_source();
        }
        self.emit_planner_status().await;
    }

    /// Whether the planner is in a position to drive advancement right now.
    pub(super) fn planner_can_advance(&self) -> bool {
        self.planner.is_enabled()
            && !self.planner.is_exhausted()
            && self.manifest.is_loaded()
            && !self.manifest.is_empty()
    }

    /// Planner key of whatever is currently loaded, resolved through the
    /// source identity rather than the transport `SongData`.
    fn current_planner_key(&self) -> Option<String> {
        self.current_identity.as_ref().map(|id| id.key())
    }

    // ── Prefetch ─────────────────────────────────────────────────

    /// Kick off a one-ahead resolve for the planner's next track, unless one
    /// is already prepared or in flight.
    pub(super) fn prefetch_next_source(&mut self) {
        if !self.planner_can_advance() || !self.resolver_config.is_usable() {
            return;
        }
        let Some(next) = self.planner.peek_next(&mut self.manifest) else {
            return;
        };
        if self.source_cache.has_fresh_for(next.position) || self.source_cache.is_in_flight() {
            return;
        }
        let Some(entry) = self.manifest.entry_at(next.position).cloned() else {
            return;
        };

        let generation = self.source_cache.begin_prefetch();
        let manifest_revision = self.manifest.revision();
        let config = self.resolver_config.clone();
        let position = next.position;
        let tx = self.prefetch_tx.clone();

        self.tasks.push(tokio::task::spawn(async move {
            let outcome = tokio::task::spawn_blocking(move || {
                source_resolver::resolve_blocking(&entry, &config)
            })
            .await
            .unwrap_or_else(|err| {
                Err(ResolveError {
                    kind: ResolveErrorKind::Transient,
                    message: source_resolver::redact(&format!("resolver task failed: {err}")),
                })
            });

            let _ = tx.send(PrefetchResult {
                generation,
                manifest_revision,
                position,
                outcome,
            });
        }));
    }

    pub(super) async fn handle_prefetch_result(&mut self, result: PrefetchResult) {
        let position = result.position;
        let failure = match &result.outcome {
            Ok(source) => {
                info!(
                    "prepared one-ahead source: position={} origin={:?}",
                    position, source.origin
                );
                None
            }
            Err(err) => Some(err.clone()),
        };

        if !self.source_cache.accept(result) {
            // Superseded by a newer generation — nothing to record.
            return;
        }

        if let Some(err) = failure {
            warn!(
                "one-ahead resolve failed: position={} {}",
                position,
                source_resolver::redact(&err.message)
            );
            let exhausted = self
                .planner
                .mark_failed(position, &self.manifest, !err.is_retryable());
            if exhausted {
                self.emit_planner_exhausted(&err).await;
                self.emit_planner_status().await;
                return;
            }
            // Step over the dead track and try the one after it.
            self.prefetch_next_source();
        }

        self.emit_planner_status().await;
    }

    // ── Advancement ──────────────────────────────────────────────

    /// Advance to the planner's next track. Returns `true` when the backend
    /// took ownership of the transition, meaning the legacy queue-driven
    /// `NextSongGapless` path must not also run.
    pub(super) async fn advance_via_planner(&mut self) -> bool {
        self.advance_via_planner_in(PlannerDirection::Next).await
    }

    /// Same, in either direction. `Prev` exists for transport commands the
    /// bounded queue cannot serve: while the planner drives playback the queue
    /// holds exactly the playing track, so its own `prev()` has nothing to
    /// return — and on Android the media-notification button is pressed
    /// precisely when there is no JS runtime to compute an answer.
    pub(super) async fn advance_via_planner_in(&mut self, direction: PlannerDirection) -> bool {
        let advanced = self.try_advance_via_planner(direction).await;
        if !advanced {
            // Nothing took the announcement's place, and no load is running.
            // Drop both and republish, or the session is left showing a track
            // that is never going to play, spinning forever.
            self.load_in_flight = false;
            if self.announced_track.is_some() {
                self.settle_announcement();
                self.publish_now_playing().await;
            }
        }
        advanced
    }

    async fn try_advance_via_planner(&mut self, direction: PlannerDirection) -> bool {
        if !self.planner_can_advance() {
            return false;
        }
        // Iterative rather than recursive: a run of dead tracks would otherwise
        // grow the stack, and the bound belongs next to the loop it protects.
        // `MAX_ADVANCE_ATTEMPTS` is a backstop — `mark_failed`'s exhaustion
        // check is the real terminator.
        const MAX_ADVANCE_ATTEMPTS: usize = 8;

        for _ in 0..MAX_ADVANCE_ATTEMPTS {
            let candidate = match direction {
                PlannerDirection::Next => self.planner.peek_next(&mut self.manifest),
                PlannerDirection::Prev => self.planner.peek_prev(&self.manifest),
            };
            let Some(next) = candidate else {
                return false;
            };

            // Swap the OS media session onto the track we are heading for
            // before the resolve/download window, not after it. Without this
            // the notification keeps showing the previous track — with a
            // play/pause button that looks broken — for however long the
            // network takes. Settled by `start_playing_song` once the real
            // projection exists.
            if let Some(entry) = self.manifest.entry_at(next.position) {
                let identity = entry.identity.clone();
                let display = crate::types::TrackDisplay {
                    title: entry.title.clone(),
                    artist: entry.artist.clone(),
                    album: entry.album.clone(),
                    artwork_url: entry.artwork_url.clone(),
                };
                self.announce_track_quietly(identity, display).await;
            }

            // Use the prepared source when it is still fresh, otherwise resolve
            // now — an expired CDN link must never reach the decoder.
            let source = match self.source_cache.take_for(next.position) {
                Some(source) => source,
                None => {
                    let Some(entry) = self.manifest.entry_at(next.position).cloned() else {
                        return false;
                    };
                    // Tell the frontend the backend owns this transition BEFORE
                    // the (possibly slow) network resolve. Otherwise its 2.5s
                    // adoption fallback fires, the JS path starts its own track,
                    // and the late backend load overrides what the user hears.
                    self.load_in_flight = true;
                    let _ = self
                        .emitter()
                        .emit(AudioThreadEvent::LoadingAudio {
                            music_id: entry.identity.key(),
                            current_play_index: next.playlist_index,
                            load_request_id: None,
                        })
                        .await;

                    match self.resolve_now(&entry).await {
                        Ok(source) => source,
                        Err(err) => {
                            warn!(
                                "planner advance resolve failed: {}",
                                source_resolver::redact(&err.message)
                            );
                            self.load_in_flight = false;
                            let exhausted = self.planner.mark_failed(
                                next.position,
                                &self.manifest,
                                !err.is_retryable(),
                            );
                            if exhausted {
                                self.emit_planner_exhausted(&err).await;
                                self.emit_planner_status().await;
                                return false;
                            }
                            // Step over the broken track and keep walking in the
                            // direction that was asked for. A backwards step
                            // consumes its history entry too, so the dead track
                            // is not offered again on the next press.
                            self.commit_planned(&next, direction);
                            continue;
                        }
                    }
                }
            };

            if self.start_planned_track(&next, source.uri, direction).await {
                return true;
            }
            // The track resolved but would not start (decoder error, dead CDN
            // link). `start_planned_track` has already recorded the failure;
            // keep walking so a frozen frontend still gets a playing track.
            if self.planner.is_exhausted() {
                return false;
            }
        }

        false
    }

    /// Move the planner cursor onto a track it just committed to playing.
    ///
    /// The direction decides what happens to the play history: forward hops
    /// extend it, backward hops consume it.
    fn commit_planned(&mut self, track: &PlannedTrack, direction: PlannerDirection) {
        match direction {
            PlannerDirection::Next => self.planner.commit(track),
            PlannerDirection::Prev => self.planner.commit_back(track),
        }
    }

    /// Load `track` from `uri` through the existing bounded-queue machinery,
    /// then re-anchor planner state and prepare the following track.
    async fn start_planned_track(
        &mut self,
        track: &PlannedTrack,
        uri: String,
        direction: PlannerDirection,
    ) -> bool {
        // Carry the manifest's own metadata into the load. This is the
        // WebView-is-dead path — nothing else is going to tell us what this
        // track is called, and without it the media session would fall back to
        // the temp file the URI gets downloaded into.
        let display = self
            .manifest
            .entry_at(track.position)
            .map(|entry| crate::types::TrackDisplay {
                title: entry.title.clone(),
                artist: entry.artist.clone(),
                album: entry.album.clone(),
                artwork_url: entry.artwork_url.clone(),
            });
        let song = SongData::Local {
            file_path: uri,
            orig_order: track.playlist_index,
            display,
        };
        let music_id = song.get_id();

        // Keep the bounded queue to exactly the track being played: the
        // planner, not the queue, owns what comes next.
        self.playback_queue
            .set_playlist(vec![song.clone()], /* windowed */ true);
        self.playback_queue.set_index(track.playlist_index);
        self.playlist = self.playback_queue.playlist_cloned();
        self.current_song = Some(song);
        self.current_play_index = track.playlist_index;
        self.pending_identity = Some(track.identity.clone());

        self.commit_planned(track, direction);

        match self.start_playing_song(true, None, None).await {
            Ok(()) => {
                self.planner.mark_started(track.position);
                let _ = self
                    .emitter()
                    .emit(AudioThreadEvent::NativePlannerAdvanced {
                        manifest_revision: self.manifest.revision(),
                        identity: track.identity.clone(),
                        playlist_index: track.playlist_index,
                        music_id,
                    })
                    .await;
                self.emit_planner_status().await;
                // Immediately begin preparing the following track so the next
                // hand-off has a warm source.
                self.prefetch_next_source();
                true
            }
            Err(err) => {
                warn!("planner advance failed to start playback: {err:?}");
                let resolve_err = ResolveError {
                    kind: ResolveErrorKind::Unavailable,
                    message: source_resolver::redact(&err.to_string()),
                };
                // A source that resolved but will not decode is track-specific,
                // so blacklist it rather than just counting the streak.
                let exhausted = self
                    .planner
                    .mark_failed(track.position, &self.manifest, true);
                if exhausted {
                    self.emit_planner_exhausted(&resolve_err).await;
                }
                self.emit_planner_status().await;
                false
            }
        }
    }

    async fn resolve_now(
        &mut self,
        entry: &NativeManifestEntry,
    ) -> Result<super::source_resolver::ResolvedSource, ResolveError> {
        let entry = entry.clone();
        let config = self.resolver_config.clone();
        tokio::task::spawn_blocking(move || source_resolver::resolve_blocking(&entry, &config))
            .await
            .unwrap_or_else(|err| {
                Err(ResolveError {
                    kind: ResolveErrorKind::Transient,
                    message: source_resolver::redact(&format!("resolver task failed: {err}")),
                })
            })
    }

    /// Move the planner cursor onto the manifest entry whose UI index is
    /// `playlist_index`, after some other subsystem (native AutoMix) advanced
    /// playback on its own. No-op when the manifest has no such entry.
    pub(super) fn advance_planner_cursor_to_playlist_index(&mut self, playlist_index: usize) {
        if !self.manifest.is_loaded() {
            return;
        }
        let identity = self
            .manifest
            .entries()
            .iter()
            .find(|entry| entry.playlist_index == playlist_index)
            .map(|entry| entry.identity.clone());
        let Some(identity) = identity else { return };
        self.current_identity = Some(identity.clone());
        if self.planner.anchor_to_key(&self.manifest, &identity.key()) {
            self.source_cache.invalidate();
            self.prefetch_next_source();
        }
    }

    /// Invalidate the planner's notion of "currently playing" after a
    /// frontend-driven load.
    ///
    /// The transport `SongData` carries only a CDN URL and a queue-local
    /// `orig_order` — `NativeRustSound.load()` always sends a one-entry queue
    /// with `origOrder: 0`, so `current_play_index` is NOT a manifest index and
    /// must never be used to look one up (doing so anchored every manual
    /// selection to `playlists[0]`).
    ///
    /// The frontend always follows a load with `publishNativeManifest()`, whose
    /// `cursorIdentity` is authoritative. So the correct action here is simply
    /// to drop the stale identity and let that manifest re-anchor the cursor.
    pub(super) fn invalidate_planner_anchor(&mut self) {
        self.current_identity = None;
        self.planner.invalidate_plan();
        self.source_cache.invalidate();
    }

    // ── Status ───────────────────────────────────────────────────

    pub(super) fn planner_status(&self) -> NativePlannerStatus {
        let cursor = self.planner.cursor(&self.manifest);
        NativePlannerStatus {
            manifest_revision: self.manifest.revision(),
            cursor_identity: cursor.as_ref().map(|track| track.identity.clone()),
            cursor_index: cursor.as_ref().map(|track| track.playlist_index),
            playback_state: self.planner_playback_state(),
            prepared_identity: self.source_cache.prepared_identity().cloned(),
            failure_count: self.planner.failure_count(),
            exhausted: self.planner.is_exhausted(),
            enabled: self.planner.is_enabled(),
        }
    }

    fn planner_playback_state(&self) -> NativePlannerPlaybackState {
        if self.current_song.is_none() {
            return NativePlannerPlaybackState::Stopped;
        }
        match self.playback_intent {
            PlaybackIntent::Playing => NativePlannerPlaybackState::Playing,
            PlaybackIntent::Paused => NativePlannerPlaybackState::Paused,
        }
    }

    pub(super) async fn emit_planner_status(&self) {
        let status = self.planner_status();
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::NativePlannerStatusChanged { status })
            .await;
    }

    async fn emit_planner_exhausted(&self, err: &ResolveError) {
        warn!(
            "native planner exhausted after {} failures",
            self.planner.failure_count()
        );
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::NativePlannerExhausted {
                manifest_revision: self.manifest.revision(),
                attempted: self.planner.failure_count(),
                reason: source_resolver::redact(&err.message),
            })
            .await;
    }
}
