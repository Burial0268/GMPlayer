//! Native AutoMix runtime: prepare-task orchestration, secondary-deck
//! preloading, crossfade start/complete scheduling and runtime teardown.
//! The AutoMix *state machine* itself lives in `super::automix`.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::decoder::{self, PlaybackSink};
use crate::types::{
    AudioThreadEvent, AudioThreadEventMessage, AudioThreadMessage, AutoMixNativeState, SongData,
};

use super::automix;
use super::mixer::{CrossfadeParams, DeckId};
use super::{AudioPlayer, PlaybackIntent};

impl AudioPlayer {
    pub(super) fn spawn_automix_prepare_task(&self, request: automix::AutoMixPrepareRequest) {
        let tx = self.automix_prepare_tx.clone();
        tokio::spawn(async move {
            let generation = request.generation;
            let transition_id = request.transition_id;
            let current_index = request.current_index;
            let current_id = request.current_id.clone();
            let task =
                tokio::task::spawn_blocking(move || automix::run_prepare_request_blocking(request));
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
        if result.generation != self.automix_prepare_generation {
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
            self.schedule_native_automix_trigger(start_time);
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
        self.native_crossfade_generation = self.native_crossfade_generation.wrapping_add(1);
        self.native_crossfade_active = false;
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
    }

    fn schedule_native_automix_trigger(&mut self, start_time: f64) {
        self.native_crossfade_generation = self.native_crossfade_generation.wrapping_add(1);
        let generation = self.native_crossfade_generation;
        let clock = Arc::clone(&self.clock);
        let tx = self.self_msg_tx.clone();
        tokio::spawn(async move {
            loop {
                let position = clock.lock().position();
                if position + 0.025 >= start_time {
                    let _ = tx.send(AudioThreadEventMessage::new(
                        String::new(),
                        Some(AudioThreadMessage::AutomixForceStart {
                            generation: Some(generation),
                        }),
                    ));
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });
    }

    fn schedule_native_automix_complete(
        &self,
        generation: u64,
        current_index: usize,
        position: f64,
    ) {
        let tx = self.self_msg_tx.clone();
        let delay = Duration::from_secs_f64(position.max(0.05));
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = tx.send(AudioThreadEventMessage::new(
                String::new(),
                Some(AudioThreadMessage::AutomixCompleteNative {
                    generation,
                    current_index,
                    position,
                }),
            ));
        });
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

    pub(super) async fn start_native_automix_crossfade(&mut self) -> anyhow::Result<()> {
        let prepared = self
            .automix
            .take_prepared_for_start()
            .ok_or_else(|| anyhow::anyhow!("AutoMix has no prepared next track"))?;

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
            "AutoMix crossfade starting: next_index={}, preloaded={}, duration={:.3}",
            prepared.next_index, preloaded, duration
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
            })
            .unwrap_or_else(|| CrossfadeParams {
                outgoing_gain: self.active_norm_gain,
                ..Default::default()
            });
        // Remember the incoming track's normalization gain so it can be promoted
        // to `active_norm_gain` on completion — keeping the level continuous into
        // steady-state playback and the next crossfade's outgoing side.
        self.secondary_norm_gain = crossfade_params.incoming_gain;

        self.deck_mixer.start_crossfade(
            self.active_deck,
            incoming_deck,
            duration,
            output_config.sample_rate,
            output_config.channels,
            crossfade_params,
        );

        let _ = self
            .emitter()
            .emit(AudioThreadEvent::AutomixCrossfadeStarted {
                from_id: self
                    .current_song
                    .as_ref()
                    .map(SongData::get_id)
                    .unwrap_or_default(),
                to_id: prepared.song.get_id(),
                duration,
                transition_id: prepared.transition_id,
            })
            .await;

        let finish_index = prepared.next_index;
        self.schedule_native_automix_complete(
            self.native_crossfade_generation,
            finish_index,
            duration,
        );

        Ok(())
    }

    pub(super) async fn complete_native_automix(&mut self, current_index: usize, position: f64) {
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
        if let Some(playback_id) = self.secondary_playback_id.take() {
            self.decoder_playback_id = playback_id;
        }
        self.current_play_index = current_index;
        if let Some(song) = promoted_song {
            self.playback_queue
                .replace_or_set_current(current_index, song);
            self.playlist = self.playback_queue.playlist_cloned();
            self.sync_current_from_queue();
        }
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

        let mut display_info = self.secondary_display_info.take().unwrap_or_default();
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
