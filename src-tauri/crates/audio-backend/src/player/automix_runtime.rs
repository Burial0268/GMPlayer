//! Native AutoMix runtime: prepare-task orchestration, secondary-deck
//! preloading, crossfade start/complete scheduling and runtime teardown.
//! The AutoMix *state machine* itself lives in `super::automix`.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::decoder::{self, PlaybackSink};
use crate::types::{AudioThreadEvent, AutoMixNativeState, SongData, TrackIdentity};

use super::automix;
use super::mixer::{CrossfadeParams, DeckId};
use super::{AudioPlayer, PlaybackIntent};

pub(super) struct NativeMix {
    generation: u64,
    next_index: usize,
    from_id: String,
    to_id: String,
    duration: f64,
    pub(super) started: bool,
    pub(super) outgoing_finished: bool,
    playback_id: u64,
    incoming_identity: Option<TrackIdentity>,
}

impl AudioPlayer {
    pub(super) fn spawn_automix_prepare_task(&self, request: automix::AutoMixPrepareRequest) {
        let tx = self.automix_prepare_tx.clone();
        let live = Arc::clone(&self.automix_prepare_generation);
        tokio::spawn(async move {
            let generation = request.generation;
            let transition_id = request.transition_id;
            let current_index = request.current_index;
            let current_id = request.current_id.clone();
            let cancel = automix::PrepareCancel::new(live, generation);
            let task = tokio::task::spawn_blocking(move || {
                automix::run_prepare_request_blocking(request, &cancel)
            });
            let result = match tokio::time::timeout(Duration::from_secs(60), task).await {
                Ok(Ok(result)) => result,
                Ok(Err(err)) => automix::AutoMixPrepareResult {
                    generation,
                    transition_id,
                    current_index,
                    current_id,
                    result: Err(format!("AutoMix prepare task failed: {err}")),
                },
                Err(_) => automix::AutoMixPrepareResult {
                    generation,
                    transition_id,
                    current_index,
                    current_id,
                    result: Err("AutoMix prepare timed out".to_string()),
                },
            };
            let _ = tx.send(result);
        });
    }

    pub(super) async fn handle_automix_prepare_result(
        &mut self,
        result: automix::AutoMixPrepareResult,
    ) {
        if result.generation != self.automix_prepare_generation.load(Ordering::Acquire) {
            return;
        }

        let status_index = result.current_index;
        let events = self.automix.finish_prepare(result, status_index);
        let status = self.automix.status(status_index);
        self.emit_many(events).await;

        if status.state != AutoMixNativeState::Waiting {
            return;
        }

        if let Err(err) = self.preload_native_automix_deck().await {
            warn!("AutoMix preload failed: {err:?}");
            self.cancel_native_automix_runtime().await;
            let events = self
                .automix
                .mark_failed(format!("AutoMix preload failed: {err}"), status_index);
            self.emit_many(events).await;
            return;
        }

        if let Some(start_time) = status.crossfade_start {
            info!(
                "AutoMix prepared: next_index={:?}, start_time={:.3}, duration={:?}",
                status.next_index, start_time, status.crossfade_duration
            );
            if let Err(error) = self.start_native_automix_crossfade(true).await {
                self.cancel_native_automix_runtime().await;
                let events = self.automix.mark_failed(error.to_string(), status_index);
                self.emit_many(events).await;
            }
        } else {
            warn!(
                "AutoMix prepared without crossfade_start; frontend force-start fallback is required"
            );
        }
    }

    fn inactive_deck(&self) -> DeckId {
        match self.active_deck {
            DeckId::Primary => DeckId::Secondary,
            DeckId::Secondary => DeckId::Primary,
        }
    }

    pub(super) async fn cancel_native_automix_runtime(&mut self) {
        if let Some((index, position)) = self.rendered_native_mix_completion() {
            let identity = self.native_mix.take().and_then(|mix| mix.incoming_identity);
            self.complete_native_automix(index, position, identity)
                .await;
        }
        let outgoing_finished = self
            .native_mix
            .take()
            .is_some_and(|mix| mix.outgoing_finished);
        self.bump_native_crossfade_gen();
        self.native_crossfade_active = false;
        self.native_mix = None;
        if let Some(handle) = &self.current_decoder_handle {
            handle.cancel_tempo();
        }
        self.native_crossfade_transition_id = None;
        self.secondary_playback_id = None;

        if let Some(handle) = self.secondary_decoder_handle.take() {
            handle.stop();
        }
        self.secondary_local_path = None;
        self.secondary_temp_file = None;
        self.secondary_song = None;
        self.secondary_duration = 0.0;
        self.secondary_display_info = None;
        self.secondary_quality = None;

        self.deck_mixer.clear_deck(self.inactive_deck());
        // The current track keeps playing — only the incoming automix deck is
        // torn down — so restore the active deck to its persistent normalization
        // gain rather than snapping it back to 1.0.
        self.deck_mixer
            .set_deck_gain(self.active_deck, self.active_norm_gain);
        self.deck_mixer.set_deck_gain(self.inactive_deck(), 0.0);
        self.secondary_norm_gain = 1.0;
        if outgoing_finished {
            let _ = self.decoder_event_tx.send(decoder::DecoderEvent::Finished {
                playback_id: self.decoder_playback_id,
            });
        }
    }

    pub(super) fn native_prepare_matches_current(&self, expected: Option<&TrackIdentity>) -> bool {
        let actual = self
            .current_identity
            .clone()
            .or_else(|| self.current_song.as_ref().and_then(local_identity));
        match (expected, actual.as_ref()) {
            (Some(expected), Some(actual)) => expected == actual,
            _ => true,
        }
    }

    pub(super) async fn resume_native_mix(
        &mut self,
        next_index: usize,
        identity: Option<&TrackIdentity>,
        transition_id: Option<u64>,
    ) -> bool {
        let generation = self.native_crossfade_gen();
        let Some(identity) = identity else {
            return false;
        };
        let Some(mix) = self.native_mix.as_mut() else {
            return false;
        };
        if mix.generation != generation
            || mix.playback_id != self.decoder_playback_id
            || mix.incoming_identity.as_ref() != Some(identity)
        {
            return false;
        }
        mix.next_index = next_index;
        let started = mix
            .started
            .then(|| AudioThreadEvent::AutomixCrossfadeStarted {
                from_id: mix.from_id.clone(),
                to_id: mix.to_id.clone(),
                duration: mix.duration,
                transition_id,
            });
        self.native_crossfade_transition_id = transition_id;
        self.automix.rebind_prepared(next_index, transition_id);
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::AutomixStatus {
                status: self.automix.status(self.current_play_index),
            })
            .await;
        if let Some(event) = started {
            let _ = self.emitter().emit(event).await;
        }
        info!("AutoMix attached to active native pair: next_index={next_index}, transition_id={transition_id:?}");
        true
    }

    fn rendered_native_mix_completion(&self) -> Option<(usize, f64)> {
        let mix = self.native_mix.as_ref()?;
        if mix.generation != self.native_crossfade_gen() {
            return None;
        }
        let clock = self.output.writer().render_clock();
        let anchor = clock.anchor()?;
        let rendered = clock.rendered_samples();
        if anchor.anchor.transition != mix.generation
            || anchor.anchor.phase != 2
            || rendered < anchor.rendered_at
        {
            return None;
        }
        let config = self.output.config();
        let position = anchor.anchor.source.seconds
            + (rendered - anchor.rendered_at) as f64
                / (config.sample_rate as f64 * config.channels as f64)
                * anchor.anchor.source.rate;
        let next_index = mix
            .incoming_identity
            .as_ref()
            .and_then(|identity| self.manifest.position_of_key(&identity.key()))
            .and_then(|position| self.manifest.entry_at(position))
            .map_or(mix.next_index, |entry| entry.playlist_index);
        Some((next_index, position))
    }

    pub(super) async fn tick_native_mix(&mut self) {
        let Some(mix) = self.native_mix.as_ref() else {
            return;
        };
        if mix.generation != self.native_crossfade_gen() {
            self.native_mix = None;
            return;
        }
        let render_clock = self.output.writer().render_clock();
        let Some(anchor) = render_clock.anchor() else {
            return;
        };
        if anchor.rendered_at > render_clock.rendered_samples()
            || anchor.anchor.transition != mix.generation
            || anchor.anchor.phase == 0
        {
            return;
        }
        if anchor.anchor.phase == 3 {
            self.cancel_native_automix_runtime().await;
            let events = self.automix.mark_failed(
                "AutoMix incoming audio missed its scheduled beat".into(),
                self.current_play_index,
            );
            self.emit_many(events).await;
            return;
        }
        let mix = self.native_mix.as_mut().unwrap();
        if !mix.started {
            mix.started = true;
            info!(
                transition_id = ?self.native_crossfade_transition_id,
                generation = mix.generation,
                next_index = mix.next_index,
                phase = anchor.anchor.phase,
                source_position = anchor.anchor.source.seconds,
                rendered_at = anchor.rendered_at,
                "AutoMix crossfade reached output"
            );
            let event = AudioThreadEvent::AutomixCrossfadeStarted {
                from_id: mix.from_id.clone(),
                to_id: mix.to_id.clone(),
                duration: mix.duration,
                transition_id: self.native_crossfade_transition_id,
            };
            self.automix.mark_started();
            let _ = self.emitter().emit(event).await;
        }
        if let Some((index, position)) = self.rendered_native_mix_completion() {
            let identity = self.native_mix.take().and_then(|mix| mix.incoming_identity);
            self.complete_native_automix(index, position, identity)
                .await;
        }
    }

    fn secondary_matches_prepared_automix(&self, song: &SongData, local_path: &Path) -> bool {
        let song_id = song.get_id();
        self.secondary_decoder_handle.is_some()
            && self.secondary_playback_id.is_some()
            && self
                .secondary_local_path
                .as_ref()
                .is_some_and(|path| path == local_path)
            && self
                .secondary_song
                .as_ref()
                .is_some_and(|secondary_song| secondary_song.get_id() == song_id)
    }

    async fn preload_native_automix_deck(&mut self) -> anyhow::Result<()> {
        if self.native_crossfade_active {
            return Ok(());
        }

        let prepared = self
            .automix
            .take_prepared_for_preload()
            .ok_or_else(|| anyhow::anyhow!("AutoMix has no prepared next track to preload"))?;

        if let Some(handle) = self.secondary_decoder_handle.take() {
            handle.stop();
        }
        self.secondary_local_path = None;
        self.secondary_temp_file = None;
        self.secondary_song = None;
        self.secondary_duration = 0.0;
        self.secondary_display_info = None;
        self.secondary_quality = None;
        self.secondary_playback_id = None;

        let incoming_deck = self.inactive_deck();
        self.deck_mixer.clear_deck(incoming_deck);
        let incoming_writer = match incoming_deck {
            DeckId::Primary => self.deck_mixer.primary_writer(),
            DeckId::Secondary => self.deck_mixer.secondary_writer(),
        };
        let start_paused = self.playback_intent == PlaybackIntent::Paused;
        incoming_writer.set_paused(start_paused);
        self.deck_mixer.set_deck_gain(incoming_deck, 0.0);

        let analysis_tx_for_open = self.analysis_tx.clone();
        let path_for_open = prepared.local_path.clone();
        let output_config = self.output.config();
        let playback_id = self.decoder_playback_id.wrapping_add(1);
        let decoder_event_tx = self.decoder_event_tx.clone();
        let analysis_enabled_for_open = Arc::clone(&self.analysis_enabled);

        let open_result = tokio::task::spawn_blocking(move || {
            decoder::spawn_playback_decoder(
                &path_for_open,
                Some(0.0),
                incoming_writer,
                output_config.channels,
                output_config.sample_rate,
                analysis_tx_for_open,
                analysis_enabled_for_open,
                decoder_event_tx,
                playback_id,
                start_paused,
            )
        })
        .await?;

        let incoming_handle = open_result.map_err(|e| anyhow::anyhow!(e.to_string()))?;

        self.secondary_decoder_handle = Some(incoming_handle);
        self.secondary_local_path = Some(prepared.local_path);
        self.secondary_temp_file = prepared._temp_file;
        self.secondary_song = Some(prepared.song);
        self.secondary_duration = prepared.analysis_duration;
        self.secondary_display_info = Some(prepared.display_info);
        self.secondary_quality = Some(prepared.quality);
        self.secondary_playback_id = Some(playback_id);

        info!(
            "AutoMix preloaded secondary deck: next_index={}, playback_id={}",
            prepared.next_index, playback_id
        );

        Ok(())
    }

    pub(super) async fn start_native_automix_crossfade(
        &mut self,
        scheduled: bool,
    ) -> anyhow::Result<()> {
        let mut prepared = self
            .automix
            .take_prepared_for_start()
            .ok_or_else(|| anyhow::anyhow!("AutoMix has no prepared next track"))?;
        if prepared.analysis_duration > 0.0 && prepared.analysis_duration <= 0.1 {
            anyhow::bail!("AutoMix incoming track is too short for an overlap");
        }

        let incoming_deck = self.inactive_deck();
        let incoming_writer = match incoming_deck {
            DeckId::Primary => self.deck_mixer.primary_writer(),
            DeckId::Secondary => self.deck_mixer.secondary_writer(),
        };
        let start_paused = self.playback_intent == PlaybackIntent::Paused;
        incoming_writer.set_paused(start_paused);
        self.deck_mixer.set_deck_gain(incoming_deck, 0.0);
        let output_config = self.output.config();
        let preloaded =
            self.secondary_matches_prepared_automix(&prepared.song, &prepared.local_path);
        let playback_id = if preloaded {
            if let Some(handle) = &self.secondary_decoder_handle {
                let _ = handle.set_paused(start_paused);
            }
            self.secondary_playback_id
                .unwrap_or_else(|| self.decoder_playback_id.wrapping_add(1))
        } else {
            if let Some(handle) = self.secondary_decoder_handle.take() {
                handle.stop();
            }
            self.secondary_local_path = None;
            self.secondary_temp_file = None;
            self.secondary_song = None;
            self.secondary_duration = 0.0;
            self.secondary_display_info = None;
            self.secondary_quality = None;
            self.secondary_playback_id = None;
            self.deck_mixer.clear_deck(incoming_deck);

            let analysis_tx_for_open = self.analysis_tx.clone();
            let path_for_open = prepared.local_path.clone();
            let playback_id = self.decoder_playback_id.wrapping_add(1);
            let decoder_event_tx = self.decoder_event_tx.clone();
            let analysis_enabled_for_open = Arc::clone(&self.analysis_enabled);

            let open_result = tokio::task::spawn_blocking(move || {
                decoder::spawn_playback_decoder(
                    &path_for_open,
                    Some(0.0),
                    incoming_writer,
                    output_config.channels,
                    output_config.sample_rate,
                    analysis_tx_for_open,
                    analysis_enabled_for_open,
                    decoder_event_tx,
                    playback_id,
                    start_paused,
                )
            })
            .await?;

            let incoming_handle = open_result.map_err(|e| anyhow::anyhow!(e.to_string()))?;
            self.secondary_decoder_handle = Some(incoming_handle);
            self.secondary_temp_file = prepared._temp_file;
            playback_id
        };
        let now = self.clock_position();
        let start_position = prepared
            .plan
            .as_ref()
            .filter(|plan| scheduled && plan.start_time > now)
            .map(|plan| plan.start_time);
        if let Some(plan) = prepared.plan.as_mut() {
            if prepared.analysis_duration > 0.0 && plan.duration + 0.1 >= prepared.analysis_duration
            {
                plan.duration = (prepared.analysis_duration - 0.1).max(0.05);
                plan.outgoing_rate = 1.0;
                plan.beat_synced = false;
            }
            if start_position.is_none() || plan.start_time <= now + 2.0 || start_paused {
                plan.outgoing_rate = 1.0;
                plan.beat_synced = false;
            }
            if (plan.outgoing_rate - 1.0).abs() > 1e-6 {
                let channels = output_config.channels;
                let sample_rate = output_config.sample_rate;
                let rate = plan.outgoing_rate.clamp(0.96, 1.04);
                let processor = tokio::task::spawn_blocking(move || {
                    decoder::tempo::AutoMixStretch::new(channels, sample_rate, rate)
                })
                .await?;
                let result = match self.current_decoder_handle.as_ref() {
                    Some(handle) => match handle.arm_tempo(plan.start_time, processor) {
                        Ok(ack) => ack.wait().await,
                        Err(error) => Err(error),
                    },
                    None => Err(crate::error::AudioError::NotInitialized),
                };
                if let Err(error) = result {
                    warn!("AutoMix tempo preparation missed its lead time, using ordinary crossfade: {error}");
                    if let Some(handle) = &self.current_decoder_handle {
                        handle.cancel_tempo();
                    }
                    plan.outgoing_rate = 1.0;
                    plan.beat_synced = false;
                }
            }
        }
        let duration = prepared.plan.as_ref().map(|p| p.duration).unwrap_or(2.0);

        self.secondary_local_path = Some(prepared.local_path.clone());
        self.secondary_song = Some(prepared.song.clone());
        self.secondary_duration = prepared.analysis_duration;
        self.secondary_display_info = Some(prepared.display_info.clone());
        self.secondary_quality = Some(prepared.quality.clone());
        self.secondary_playback_id = Some(playback_id);
        self.native_crossfade_active = true;
        self.native_crossfade_transition_id = prepared.transition_id;
        info!(
            "AutoMix crossfade armed: transition_id={:?}, next_index={}, preloaded={}, position={:.3}, cue={:?}, duration={:.3}, rate={:.5}, beat_synced={}",
            prepared.transition_id, prepared.next_index, preloaded, now, start_position, duration,
            prepared.plan.as_ref().map_or(1.0, |plan| plan.outgoing_rate),
            prepared.plan.as_ref().is_some_and(|plan| plan.beat_synced),
        );

        let crossfade_params = prepared
            .plan
            .as_ref()
            .map(|plan| CrossfadeParams {
                curve: plan.curve,
                incoming_gain: plan.incoming_gain_adjustment as f32,
                // The outgoing deck is already playing at its own persistent
                // normalization gain — feed that in as the outgoing target so the
                // crossfade starts from the current level instead of snapping the
                // outgoing track up to 1.0.
                outgoing_gain: self.active_norm_gain,
                overlap_headroom_db: plan.overlap_headroom_db as f32,
                in_shape: plan.in_shape as f32,
                out_shape: plan.out_shape as f32,
            })
            .unwrap_or_else(|| CrossfadeParams {
                outgoing_gain: self.active_norm_gain,
                ..Default::default()
            });
        // Remember the incoming track's normalization gain so it can be promoted
        // to `active_norm_gain` on completion — keeping the level continuous into
        // steady-state playback and the next crossfade's outgoing side.
        self.secondary_norm_gain = crossfade_params.incoming_gain;

        let generation = self.bump_native_crossfade_gen();
        self.native_mix = Some(NativeMix {
            generation,
            next_index: prepared.next_index,
            from_id: self
                .current_song
                .as_ref()
                .map(SongData::get_id)
                .unwrap_or_default(),
            to_id: prepared.song.get_id(),
            duration,
            started: false,
            outgoing_finished: false,
            playback_id: self.decoder_playback_id,
            incoming_identity: prepared
                .next_identity
                .clone()
                .or_else(|| local_identity(&prepared.song)),
        });
        self.deck_mixer.schedule_crossfade(
            self.active_deck,
            incoming_deck,
            duration,
            output_config.sample_rate,
            output_config.channels,
            crossfade_params,
            start_position,
            generation,
        );

        Ok(())
    }

    pub(super) async fn complete_native_automix(
        &mut self,
        current_index: usize,
        position: f64,
        incoming_identity: Option<TrackIdentity>,
    ) {
        if !self.native_crossfade_active || self.secondary_decoder_handle.is_none() {
            return;
        }

        if let Some(handle) = self.current_decoder_handle.take() {
            handle.stop();
        }

        self.current_decoder_handle = self.secondary_decoder_handle.take();
        self.current_local_path = self.secondary_local_path.take();
        self.current_temp_file = self.secondary_temp_file.take();
        let promoted_song = self.secondary_song.take();
        self.current_song = promoted_song.clone();
        self.current_file_path = self
            .current_song
            .as_ref()
            .and_then(SongData::file_path)
            .map(str::to_string);
        self.current_identity =
            incoming_identity.or_else(|| self.current_song.as_ref().and_then(local_identity));
        if let Some(playback_id) = self.secondary_playback_id.take() {
            self.decoder_playback_id = playback_id;
            self.clock.lock().set_playback_id(playback_id);
        }
        self.current_play_index = current_index;
        if let Some(mut song) = promoted_song {
            // A queue edit can move B after its decoder was prepared.
            match &mut song {
                SongData::Local { orig_order, .. } | SongData::Custom { orig_order, .. } => {
                    *orig_order = current_index
                }
            }
            self.playback_queue
                .replace_or_set_current(current_index, song);
            self.playlist = self.playback_queue.playlist_cloned();
            self.sync_current_from_queue();
        }
        // AutoMix advanced the track without going through the planner. Move
        // the planner cursor with it, otherwise the next natural advance would
        // plan from the track that just finished and replay the incoming one.
        if let Some(identity) = self.current_identity.as_ref() {
            if let Some(entry) = self
                .manifest
                .position_of_key(&identity.key())
                .and_then(|position| self.manifest.entry_at(position))
            {
                self.advance_planner_cursor_to_playlist_index(entry.playlist_index);
            } else {
                self.planner.invalidate_plan();
            }
        } else {
            self.advance_planner_cursor_to_playlist_index(current_index);
        }
        self.refresh_favourite_for_current_track();
        let incoming_duration = self.secondary_duration;
        self.secondary_duration = 0.0;
        self.native_crossfade_active = false;
        let transition_id = self.native_crossfade_transition_id.take();
        self.active_deck = match self.active_deck {
            DeckId::Primary => DeckId::Secondary,
            DeckId::Secondary => DeckId::Primary,
        };
        // The incoming track (now active) is already playing at its normalization
        // gain — the mixer left the deck there at crossfade end. Adopt it as the
        // persistent active gain so cancels/rebuilds restore the right level.
        self.active_norm_gain = self.secondary_norm_gain;
        self.secondary_norm_gain = 1.0;

        // Never publish a *default* `DisplayAudioInfo`. `secondary_display_info`
        // is `None` whenever the incoming deck was torn down between arming the
        // crossfade and completing it (a manual skip runs `start_playing_song`
        // with `clear_sink`, which clears it), and a default carries
        // `duration: 0` plus an empty name — which `sync_ui` then broadcasts as
        // the *current* track. The frontend reads that duration in preference to
        // its own, so the UI went to 0:00 while the position kept running.
        // Keeping what is already published is always closer to the truth.
        let mut display_info = match self.secondary_display_info.take() {
            Some(info) => info,
            None => self.current_audio_info.read().await.clone(),
        };
        display_info.duration = if display_info.duration > 0.0 {
            display_info.duration
        } else {
            incoming_duration
        };
        display_info.position = position;
        let duration = display_info.duration;
        *self.current_audio_info.write().await = display_info;
        if let Some(quality) = self.secondary_quality.take() {
            *self.current_audio_quality.write().await = quality;
        }

        let is_playing = self.playback_intent == PlaybackIntent::Playing;
        self.clock.lock().set_duration(incoming_duration);
        // The incoming deck is a different source with its own timeline, even
        // though no `LoadAudio` announced it — the crossfade started it.
        self.begin_timeline();
        self.publish_position_anchor(is_playing, position).await;

        let music_id = self
            .current_song
            .as_ref()
            .map(SongData::get_id)
            .unwrap_or_default();

        let _ = self
            .emitter()
            .emit(AudioThreadEvent::AutomixCrossfadeComplete {
                current_index: self.current_play_index,
                music_id,
                position,
                duration,
                transition_id,
            })
            .await;
        let events = self.automix.complete(self.current_play_index);
        self.emit_many(events).await;
        self.sync_ui().await;
    }
}

pub(super) fn local_identity(song: &SongData) -> Option<TrackIdentity> {
    song.file_path()
        .filter(|path| !crate::source::is_http_url(path))
        .map(|path| TrackIdentity::Local {
            path: path.to_string(),
        })
}

#[cfg(all(test, target_os = "linux"))]
#[path = "automix_runtime/tests.rs"]
mod tests;
