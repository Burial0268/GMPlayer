//! `AudioThreadMessage` dispatch: the single `process_message` entry point
//! that maps frontend/self commands onto playback, queue, output, DSP and
//! AutoMix operations.

use std::sync::atomic::Ordering;

use tracing::info;

use crate::analysis::AnalysisCommand;
use crate::output;
use crate::types::*;

use super::api::SeekRequest;
use super::{AudioPlayer, PlaybackIntent};

impl AudioPlayer {
    pub(super) async fn process_message(
        &mut self,
        msg: AudioThreadEventMessage<AudioThreadMessage>,
    ) -> anyhow::Result<()> {
        let emitter = self.emitter();
        if let Some(ref data) = msg.data {
            match data {
                AudioThreadMessage::ResumeAudio => {
                    self.playback_intent = PlaybackIntent::Playing;
                    self.resume_audio_output().await;
                    let current_pos = self.clock_position();
                    self.publish_position_anchor(true, current_pos).await;
                    let _ = emitter
                        .emit(AudioThreadEvent::PlayStatus { is_playing: true })
                        .await;
                }
                AudioThreadMessage::PauseAudio => {
                    self.playback_intent = PlaybackIntent::Paused;
                    let current_pos = self.clock_position();
                    self.output.writer().set_paused(true);
                    if let Some(handle) = &self.current_decoder_handle {
                        let _ = handle.set_paused(true);
                    }
                    if let Some(handle) = &self.secondary_decoder_handle {
                        let _ = handle.set_paused(true);
                    }
                    self.publish_position_anchor(false, current_pos).await;
                    let _ = emitter
                        .emit(AudioThreadEvent::PlayStatus { is_playing: false })
                        .await;
                }
                AudioThreadMessage::ResumeOrPauseAudio => {
                    let was_paused = self.playback_intent == PlaybackIntent::Paused;
                    let current_pos = self.clock_position();
                    if was_paused {
                        self.playback_intent = PlaybackIntent::Playing;
                        self.resume_audio_output().await;
                    } else {
                        self.playback_intent = PlaybackIntent::Paused;
                        self.output.writer().set_paused(true);
                        if let Some(handle) = &self.current_decoder_handle {
                            let _ = handle.set_paused(true);
                        }
                        if let Some(handle) = &self.secondary_decoder_handle {
                            let _ = handle.set_paused(true);
                        }
                    }
                    self.publish_position_anchor(was_paused, current_pos).await;
                    let _ = emitter
                        .emit(AudioThreadEvent::PlayStatus {
                            is_playing: was_paused,
                        })
                        .await;
                }
                AudioThreadMessage::SeekAudio {
                    position,
                    request_id,
                    expected_music_id,
                } => {
                    self.process_seek_request(SeekRequest::new(
                        *position,
                        *request_id,
                        expected_music_id.clone(),
                    ))
                    .await?;
                }
                AudioThreadMessage::SetVolume { volume } => {
                    self.volume = volume.clamp(0.0, 1.0);
                    self.output.writer().set_volume(self.volume as f32);
                    let _ = emitter
                        .emit(AudioThreadEvent::VolumeChanged {
                            volume: self.volume,
                        })
                        .await;
                }
                AudioThreadMessage::SetVolumeRelative { volume } => {
                    self.volume = (self.volume + volume).clamp(0.0, 1.0);
                    self.output.writer().set_volume(self.volume as f32);
                    let _ = emitter
                        .emit(AudioThreadEvent::VolumeChanged {
                            volume: self.volume,
                        })
                        .await;
                }
                AudioThreadMessage::NextSong => {
                    if self.playback_queue.next().is_none() || !self.sync_current_from_queue() {
                        return self.finish_message(msg).await;
                    }
                    self.start_playing_song(true, None).await?;
                }
                AudioThreadMessage::NextSongGapless => {
                    if self.playback_queue.next().is_none() || !self.sync_current_from_queue() {
                        return self.finish_message(msg).await;
                    }
                    self.start_playing_song(true, None).await?;
                }
                AudioThreadMessage::PrevSong => {
                    if self.playback_queue.prev().is_none() || !self.sync_current_from_queue() {
                        return self.finish_message(msg).await;
                    }
                    self.start_playing_song(true, None).await?;
                }
                AudioThreadMessage::JumpToSong { song_index } => {
                    if self.playback_queue.set_index(*song_index).is_some()
                        && self.sync_current_from_queue()
                    {
                        self.start_playing_song(true, None).await?;
                    }
                }
                AudioThreadMessage::JumpToSongAt {
                    song_index,
                    position,
                } => {
                    if self.playback_queue.set_index(*song_index).is_some()
                        && self.sync_current_from_queue()
                    {
                        self.start_playing_song(true, Some(*position)).await?;
                    }
                }
                AudioThreadMessage::SetPlaylist { songs, windowed } => {
                    let current_id = self.current_song.as_ref().map(SongData::get_id);
                    self.playback_queue.set_playlist(songs.clone(), *windowed);
                    if let Some(current_id) = current_id.as_deref() {
                        // Identity wins over the clamped positional index: prefill
                        // windows replace the playlist mid-track, and the window can
                        // contain an entry whose orig_order collides with the stale
                        // index (end-of-list wrap `[cur@5, next@0]`). Re-anchoring by
                        // id keeps `current_song` on what is audibly playing.
                        self.playback_queue.set_index_by_song_id(current_id);
                    }
                    self.playlist = self.playback_queue.playlist_cloned();
                    self.sync_current_from_queue();
                    self.playlist_inited = true;
                    let _ = emitter
                        .emit(AudioThreadEvent::PlayListChanged {
                            playlist: self.playlist.clone(),
                            current_play_index: self.current_play_index,
                        })
                        .await;
                }
                AudioThreadMessage::SetAnalysis { enabled } => {
                    self.analysis_enabled.store(*enabled, Ordering::Release);
                    let _ = self
                        .analysis_tx
                        .send(AnalysisCommand::SetEnabled { enabled: *enabled });
                }
                AudioThreadMessage::SetFFT { enabled } => {
                    let _ = self
                        .analysis_tx
                        .send(AnalysisCommand::SetFftEnabled { enabled: *enabled });
                }
                AudioThreadMessage::SetFFTRange { from_freq, to_freq } => {
                    let _ = self.analysis_tx.send(AnalysisCommand::SetFreqRange {
                        from: *from_freq,
                        to: *to_freq,
                    });
                }
                AudioThreadMessage::SetEqualizer { config } => {
                    self.dsp_config.equalizer = config.clone();
                    self.dsp_config.enabled = dsp_config_is_active(&self.dsp_config);
                    self.deck_mixer.set_dsp(self.dsp_config.clone());
                }
                AudioThreadMessage::SetDsp { config } => {
                    self.dsp_config = config.clone();
                    self.deck_mixer.set_dsp(self.dsp_config.clone());
                }
                AudioThreadMessage::SetAudioOutput { name } => {
                    let selector = output::OutputDeviceSelector::from_name(name);
                    if selector != self.output_selector {
                        self.cancel_pending_output_refresh();
                        self.output_selector = selector;
                    }
                    self.reset_output_refresh_backoff();
                    self.reset_output_poll_stride();
                    self.clear_output_error();
                    self.request_output_refresh(true, false);
                }
                AudioThreadMessage::SyncStatus => {
                    // Explicit snapshot request from the frontend — emit it here.
                    // `finish_message` no longer emits SyncStatus by default.
                    self.sync_ui().await;
                }
                AudioThreadMessage::Close => {
                    // Handled in run() loop before reaching here.
                }
                AudioThreadMessage::SetMediaControlsEnabled { .. } => {
                    // OS media controls require platform-specific glue (SMTC etc.) —
                    // not yet wired into this backend.
                }
                AudioThreadMessage::AutomixSetEnabled { enabled } => {
                    let events = self.automix.set_enabled(*enabled, self.current_play_index);
                    if !enabled {
                        self.automix_prepare_generation =
                            self.automix_prepare_generation.wrapping_add(1);
                        self.cancel_native_automix_runtime().await;
                    }
                    self.emit_many(events).await;
                }
                AudioThreadMessage::AutomixConfigure { config } => {
                    let events = self
                        .automix
                        .configure(config.clone(), self.current_play_index);
                    if !config.enabled {
                        self.automix_prepare_generation =
                            self.automix_prepare_generation.wrapping_add(1);
                        self.cancel_native_automix_runtime().await;
                    }
                    self.emit_many(events).await;
                }
                AudioThreadMessage::AutomixPrepareNext {
                    current_index,
                    next_index,
                    next_song,
                    transition_id,
                } => {
                    info!(
                        "AutoMix prepare requested: current_index={}, next_index={}, transition_id={:?}",
                        current_index, next_index, transition_id
                    );
                    self.automix_prepare_generation =
                        self.automix_prepare_generation.wrapping_add(1);
                    let generation = self.automix_prepare_generation;
                    self.cancel_native_automix_runtime().await;
                    let current_song = self.current_song.clone();
                    let current_source_path = self.current_local_path.clone();
                    let current_duration = Some(self.current_audio_info.read().await.duration)
                        .filter(|duration| *duration > 0.0);
                    let (events, request) = self.automix.begin_prepare_next(
                        generation,
                        *transition_id,
                        *current_index,
                        *next_index,
                        current_song,
                        current_duration,
                        current_source_path,
                        next_song.clone(),
                    );
                    if let Some(request) = request {
                        self.spawn_automix_prepare_task(request);
                    }
                    self.emit_many(events).await;
                }
                AudioThreadMessage::AutomixCancel => {
                    self.automix_prepare_generation =
                        self.automix_prepare_generation.wrapping_add(1);
                    self.cancel_native_automix_runtime().await;
                    let events = self.automix.cancel(self.current_play_index);
                    self.emit_many(events).await;
                }
                AudioThreadMessage::AutomixForceStart { generation } => {
                    if let Some(generation) = generation {
                        if *generation != self.native_crossfade_generation {
                            return self.finish_message(msg).await;
                        }
                    }
                    if self.native_crossfade_active {
                        return self.finish_message(msg).await;
                    }
                    if let Err(err) = self.start_native_automix_crossfade().await {
                        let events = self
                            .automix
                            .mark_failed(err.to_string(), self.current_play_index);
                        self.emit_many(events).await;
                    }
                }
                AudioThreadMessage::AutomixCompleteNative {
                    generation,
                    current_index,
                    position,
                } => {
                    if *generation != self.native_crossfade_generation {
                        return self.finish_message(msg).await;
                    }
                    self.complete_native_automix(*current_index, *position)
                        .await;
                }
            }
        }
        self.finish_message(msg).await
    }

    /// Ack the request (so the frontend's callback_id pairing resolves) and
    /// return. Specific state-change events (PlayStatus, PlayPosition,
    /// VolumeChanged, PlayListChanged) are emitted at the point of change.
    /// SyncStatus snapshots are only emitted on explicit `SyncStatus`
    /// requests or after `start_playing_song` — emitting one per command
    /// caused races where a snapshot could observe the old playback intent
    /// before a follow-on ResumeAudio had been processed.
    async fn finish_message(
        &self,
        msg: AudioThreadEventMessage<AudioThreadMessage>,
    ) -> anyhow::Result<()> {
        let _ = self.emitter().ret_none(msg).await;
        Ok(())
    }
}

fn dsp_config_is_active(config: &DspConfig) -> bool {
    if config.input_gain_db.abs() >= 0.001 || config.output_gain_db.abs() >= 0.001 {
        return true;
    }
    if config.limiter.enabled {
        return true;
    }
    config.equalizer.enabled
        && (config.equalizer.preamp_db.abs() >= 0.001
            || config
                .equalizer
                .bands
                .iter()
                .any(|band| band.enabled && band.gain_db.abs() >= 0.001))
}
