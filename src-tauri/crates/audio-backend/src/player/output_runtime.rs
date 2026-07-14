//! Output-device lifecycle: default-device polling (with adaptive stride),
//! stall/failure health handling, hot-swap refresh events, stream replacement
//! and full playback-chain rebuilds when the audio layout changes.

use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::analysis::AnalysisCommand;
use crate::decoder;
use crate::error::{AudioError, AudioResult};
use crate::output::{self, LowLatencyOutput};
use crate::types::{AudioInfo, AudioThreadEvent};

use super::mixer::{DeckId, DeckMixer};
use super::platform::{output_refresh_target, output_target_for_source, output_target_matches};
use super::{AudioPlayer, PlaybackIntent};

/// Adaptive default-device poll: each consecutive `Unchanged` probe stretches
/// the effective poll period by one base tick (1s → 2s → 3s), any change or
/// failure snaps it back to every tick. The probe enumerates devices (on
/// hosts without a platform default-device id it hashes the whole device
/// list, with per-device server roundtrips on PulseAudio), so idle steady
/// state should not pay that cost every second. Output FAILURES are still
/// caught by the 100ms health check independent of this stride.
const OUTPUT_POLL_MAX_STRIDE: u32 = 3;

pub(super) enum OutputRefreshEvent {
    Unchanged {
        generation: u64,
        output_epoch: u64,
    },
    Stale {
        generation: u64,
        output_epoch: u64,
    },
    Opened {
        generation: u64,
        output_epoch: u64,
        output: LowLatencyOutput,
        force_replace: bool,
        rebuild_chain: bool,
    },
    DecoderReady {
        generation: u64,
        output_epoch: u64,
        playback_id: u64,
        position: f64,
        output: LowLatencyOutput,
        deck_mixer: DeckMixer,
        result: AudioResult<decoder::DecoderHandle>,
    },
    Failed {
        generation: u64,
        output_epoch: u64,
        error: String,
    },
}

impl OutputRefreshEvent {
    fn generation(&self) -> u64 {
        match self {
            Self::Unchanged { generation, .. }
            | Self::Stale { generation, .. }
            | Self::Opened { generation, .. }
            | Self::DecoderReady { generation, .. }
            | Self::Failed { generation, .. } => *generation,
        }
    }

    fn output_epoch(&self) -> u64 {
        match self {
            Self::Unchanged { output_epoch, .. }
            | Self::Stale { output_epoch, .. }
            | Self::Opened { output_epoch, .. }
            | Self::DecoderReady { output_epoch, .. }
            | Self::Failed { output_epoch, .. } => *output_epoch,
        }
    }
}

impl AudioPlayer {
    pub(super) fn ensure_output_for_source(
        &mut self,
        audio_info: &AudioInfo,
    ) -> anyhow::Result<bool> {
        let target = output_target_for_source(audio_info);
        if self.output.selector() == &self.output_selector
            && output_target_matches(self.output.target(), target)
            && !self.output.has_failed()
        {
            return Ok(false);
        }

        self.cancel_pending_output_refresh();
        let output = output::open_output(self.output_selector.clone(), target)
            .map_err(AudioError::Output)?;
        let changed =
            output.config() != self.output.config() || output.device() != self.output.device();
        self.install_output(output);
        Ok(changed)
    }

    pub(super) fn cancel_pending_output_refresh(&mut self) {
        self.output_refresh_generation = self.output_refresh_generation.wrapping_add(1);
        self.output_refresh_pending = false;
        self.output_refresh_dirty = false;
        self.output_refresh_dirty_force = false;
        self.output_refresh_dirty_rebuild_chain = false;
    }

    fn mark_output_chain_committed(&mut self) {
        self.output_epoch = self.output_epoch.wrapping_add(1);
        self.reset_output_refresh_backoff();
    }

    fn install_output(&mut self, output: LowLatencyOutput) {
        output.writer().set_volume(self.volume as f32);
        output
            .writer()
            .set_paused(self.playback_intent == PlaybackIntent::Paused);
        self.output = output;
        self.mark_output_chain_committed();
        {
            let writer = self.output.writer();
            let config = self.output.config();
            self.clock.lock().set_render_clock(
                writer.render_clock(),
                config.sample_rate,
                config.channels,
            );
        }
        self.deck_mixer = DeckMixer::new(
            self.output.writer(),
            self.output.config().channels,
            self.output.config().sample_rate,
            &self.dsp_config,
        );
        self.active_deck = DeckId::Primary;
        self.secondary_playback_id = None;
        self.native_crossfade_generation = self.native_crossfade_generation.wrapping_add(1);
        self.native_crossfade_active = false;
        self.native_crossfade_transition_id = None;
        self.reset_output_health();
    }

    fn replace_output_stream(&mut self, output: LowLatencyOutput) -> Result<(), LowLatencyOutput> {
        output.writer().set_volume(0.0);
        output.writer().set_paused(true);
        let writer = output.writer();
        let config = output.config();
        if !self.deck_mixer.replace_output(writer.clone()) {
            return Err(output);
        }
        self.output = output;
        self.mark_output_chain_committed();
        self.clock.lock().set_render_clock(
            writer.render_clock(),
            config.sample_rate,
            config.channels,
        );
        self.output.writer().set_volume(self.volume as f32);
        self.output
            .writer()
            .set_paused(self.playback_intent == PlaybackIntent::Paused);
        self.reset_output_health();
        Ok(())
    }

    async fn abandon_current_mixer_for_output_rebuild(&mut self) {
        self.cancel_native_automix_runtime().await;
        self.automix_prepare_generation = self.automix_prepare_generation.wrapping_add(1);
        let events = self.automix.cancel(self.current_play_index);
        self.emit_many(events).await;

        if let Some(handle) = self.current_decoder_handle.take() {
            handle.stop();
        }
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

        self.deck_mixer = DeckMixer::new(
            self.output.writer(),
            self.output.config().channels,
            self.output.config().sample_rate,
            &self.dsp_config,
        );
        self.active_deck = DeckId::Primary;
        // The current track survives the output rebuild (it is re-decoded into the
        // fresh Primary deck), so carry its normalization gain across instead of
        // resetting to 1.0.
        self.deck_mixer
            .set_deck_gain(DeckId::Primary, self.active_norm_gain);
        self.deck_mixer.set_deck_gain(DeckId::Secondary, 0.0);
        self.secondary_norm_gain = 1.0;
        self.native_crossfade_generation = self.native_crossfade_generation.wrapping_add(1);
        self.native_crossfade_active = false;
        self.native_crossfade_transition_id = None;
        self.output_epoch = self.output_epoch.wrapping_add(1);
        self.reset_output_health();
    }

    pub(super) fn reset_output_health(&mut self) {
        self.output_health_last_samples = self.output.writer().render_clock().rendered_samples();
        self.output_health_stalled_ticks = 0;
    }

    pub(super) fn reset_output_refresh_backoff(&mut self) {
        self.output_refresh_failures = 0;
        self.output_refresh_backoff_until = None;
    }

    /// Snap the default-device poll back to every tick — any observed device
    /// change, probe failure, or user-driven output switch means the device
    /// landscape is in motion and deserves prompt re-checks again.
    pub(super) fn reset_output_poll_stride(&mut self) {
        self.output_poll_stride = 1;
        self.output_poll_ticks = 0;
    }

    fn record_output_refresh_failure(&mut self) {
        self.output_refresh_failures = self.output_refresh_failures.saturating_add(1).min(6);
        let exponent = self.output_refresh_failures.saturating_sub(1) as u32;
        let delay_ms = 250u64.saturating_mul(1u64 << exponent).min(4_000);
        self.output_refresh_backoff_until =
            Some(std::time::Instant::now() + Duration::from_millis(delay_ms));
    }

    pub(super) fn output_render_stalled(&mut self) -> bool {
        let writer = self.output.writer();
        let rendered_samples = writer.render_clock().rendered_samples();
        let queued_samples = self.deck_mixer.queued_samples();
        if self.playback_intent != PlaybackIntent::Playing
            || self.current_song.is_none()
            || self.current_decoder_handle.is_none()
        {
            self.output_health_last_samples = rendered_samples;
            self.output_health_stalled_ticks = 0;
            return false;
        }

        if rendered_samples != self.output_health_last_samples {
            self.output_health_last_samples = rendered_samples;
            self.output_health_stalled_ticks = 0;
            return false;
        }

        self.output_health_stalled_ticks = self.output_health_stalled_ticks.saturating_add(1);
        let stalled_tick_limit = if queued_samples == 0 { 30 } else { 15 };
        self.output_health_stalled_ticks >= stalled_tick_limit
    }

    pub(super) fn request_output_refresh(&mut self, force_replace: bool, rebuild_chain: bool) {
        if self.output_refresh_pending {
            self.output_refresh_dirty = true;
            self.output_refresh_dirty_force |= force_replace;
            self.output_refresh_dirty_rebuild_chain |= rebuild_chain;
            return;
        }
        if let Some(backoff_until) = self.output_refresh_backoff_until {
            if std::time::Instant::now() < backoff_until {
                return;
            }
            self.output_refresh_backoff_until = None;
        }
        self.output_refresh_pending = true;
        self.output_refresh_generation = self.output_refresh_generation.wrapping_add(1);

        let generation = self.output_refresh_generation;
        let output_epoch = self.output_epoch;
        let selector = self.output_selector.clone();
        let current_device = self.output.device().clone();
        let force_replace = force_replace || self.output.has_failed();
        let rebuild_chain = rebuild_chain && self.current_song.is_some();
        let target = output_refresh_target(self.output.config());
        let tx = self.output_refresh_tx.clone();
        tokio::task::spawn_blocking(move || {
            let opened_event =
                |output: LowLatencyOutput| match output::selected_output_device_key(&selector) {
                    Ok(selected_device) if selected_device == output.device().clone() => {
                        OutputRefreshEvent::Opened {
                            generation,
                            output_epoch,
                            output,
                            force_replace,
                            rebuild_chain,
                        }
                    }
                    Ok(_) => OutputRefreshEvent::Stale {
                        generation,
                        output_epoch,
                    },
                    Err(error) => OutputRefreshEvent::Failed {
                        generation,
                        output_epoch,
                        error,
                    },
                };

            let event = if force_replace || rebuild_chain {
                match output::open_output(selector.clone(), target) {
                    Ok(output) => opened_event(output),
                    Err(error) => OutputRefreshEvent::Failed {
                        generation,
                        output_epoch,
                        error,
                    },
                }
            } else {
                match output::selected_output_device_key(&selector) {
                    Ok(selected_device) if selected_device == current_device => {
                        OutputRefreshEvent::Unchanged {
                            generation,
                            output_epoch,
                        }
                    }
                    Ok(_) => match output::open_output(selector.clone(), target) {
                        Ok(output) => opened_event(output),
                        Err(error) => OutputRefreshEvent::Failed {
                            generation,
                            output_epoch,
                            error,
                        },
                    },
                    Err(error) => OutputRefreshEvent::Failed {
                        generation,
                        output_epoch,
                        error,
                    },
                }
            };
            let _ = tx.send(event);
        });
    }

    fn complete_output_refresh(&mut self) {
        self.output_refresh_pending = false;
        if self.output_refresh_dirty {
            let force_replace = self.output_refresh_dirty_force;
            let rebuild_chain = self.output_refresh_dirty_rebuild_chain;
            self.output_refresh_dirty = false;
            self.output_refresh_dirty_force = false;
            self.output_refresh_dirty_rebuild_chain = false;
            self.request_output_refresh(force_replace, rebuild_chain);
        }
    }

    async fn emit_output_changed(&self, output: &LowLatencyOutput) {
        let config = output.config();
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::AudioOutputChanged {
                device_name: output.device().name().to_string(),
                is_default: output.selector().is_default(),
                channels: config.channels,
                sample_rate: config.sample_rate,
                sample_format: format!("{:?}", config.sample_format),
            })
            .await;
    }

    async fn emit_output_error_once(&mut self, error: String) {
        if self.last_output_error.as_deref() == Some(error.as_str()) {
            return;
        }
        self.last_output_error = Some(error.clone());
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::AudioOutputError {
                error,
                recoverable: true,
            })
            .await;
    }

    pub(super) fn clear_output_error(&mut self) {
        self.last_output_error = None;
    }

    pub(super) fn poll_output_device_tick(&mut self) {
        self.output_poll_ticks = self.output_poll_ticks.saturating_add(1);
        if self.output_poll_ticks >= self.output_poll_stride {
            self.output_poll_ticks = 0;
            self.request_output_refresh(false, false);
        }
    }

    pub(super) async fn handle_output_refresh_event(&mut self, event: OutputRefreshEvent) {
        if event.generation() != self.output_refresh_generation
            || event.output_epoch() != self.output_epoch
        {
            return;
        }

        match event {
            OutputRefreshEvent::Unchanged { .. } => {
                self.output_poll_stride = (self.output_poll_stride + 1).min(OUTPUT_POLL_MAX_STRIDE);
                self.clear_output_error();
                self.reset_output_refresh_backoff();
                self.reset_output_health();
                self.complete_output_refresh();
            }
            OutputRefreshEvent::Stale { .. } => {
                self.reset_output_poll_stride();
                warn!("刷新音频输出设备结果已过期，将重试");
                self.output_refresh_dirty = true;
                self.complete_output_refresh();
            }
            OutputRefreshEvent::Failed { error, .. } => {
                self.reset_output_poll_stride();
                warn!("刷新音频输出设备失败：{error}");
                self.record_output_refresh_failure();
                self.complete_output_refresh();
                self.emit_output_error_once(error).await;
            }
            OutputRefreshEvent::Opened {
                generation,
                output_epoch,
                output,
                force_replace,
                rebuild_chain,
            } => {
                self.reset_output_poll_stride();
                if output.device() == self.output.device()
                    && output.config() == self.output.config()
                    && !force_replace
                    && !rebuild_chain
                {
                    self.complete_output_refresh();
                    return;
                }

                let old_device = self.output.device().name().to_string();
                let new_device = output.device().name().to_string();
                let position = self.output_rebuild_position().await;
                let is_playing = self.playback_intent == PlaybackIntent::Playing;
                let position = self.clock.lock().set_anchor(is_playing, position);
                self.clear_output_error();
                info!("音频输出设备变化：{old_device} -> {new_device}");
                if output_audio_layout_matches(self.output.config(), output.config())
                    && !rebuild_chain
                {
                    match self.replace_output_stream(output) {
                        Ok(()) => {
                            self.complete_output_refresh();
                            self.emit_output_changed(&self.output).await;
                            self.publish_position_anchor(is_playing, position).await;
                            if is_playing {
                                let _ = self
                                    .emitter()
                                    .emit(AudioThreadEvent::PlayStatus { is_playing: true })
                                    .await;
                            }
                            self.sync_ui().await;
                        }
                        Err(output) => {
                            warn!("替换音频输出 writer 超时，将丢弃当前 mixer 并重建播放链路");
                            self.abandon_current_mixer_for_output_rebuild().await;
                            if self.current_song.is_some() {
                                self.spawn_reconfigured_decoder(
                                    generation,
                                    self.output_epoch,
                                    output,
                                    position,
                                );
                            } else {
                                self.install_output(output);
                                self.complete_output_refresh();
                                self.emit_output_changed(&self.output).await;
                                self.sync_ui().await;
                            }
                        }
                    }
                    return;
                }

                let old_config = self.output.config();
                let new_config = output.config();
                if rebuild_chain {
                    warn!(
                        "音频输出渲染停滞，重建当前播放链路：config={:?}",
                        old_config
                    );
                } else {
                    warn!(
                        "音频输出 layout 变化，必须重建解码器：old={:?} new={:?}",
                        old_config, new_config
                    );
                }

                if self.current_song.is_some() {
                    self.cancel_native_automix_runtime().await;
                    self.automix_prepare_generation =
                        self.automix_prepare_generation.wrapping_add(1);
                    let events = self.automix.cancel(self.current_play_index);
                    self.emit_many(events).await;
                    self.secondary_local_path = None;
                    self.secondary_temp_file = None;
                    self.secondary_song = None;
                    self.secondary_duration = 0.0;
                    self.secondary_display_info = None;
                    self.secondary_quality = None;
                    self.secondary_playback_id = None;
                    self.spawn_reconfigured_decoder(generation, output_epoch, output, position);
                } else {
                    self.install_output(output);
                    self.complete_output_refresh();
                    self.emit_output_changed(&self.output).await;
                    self.sync_ui().await;
                }
            }
            OutputRefreshEvent::DecoderReady {
                playback_id,
                position,
                output,
                deck_mixer,
                result,
                ..
            } => match result {
                Ok(handle) => {
                    if playback_id != self.decoder_playback_id.wrapping_add(1) {
                        self.complete_output_refresh();
                        return;
                    }
                    let is_playing = self.playback_intent == PlaybackIntent::Playing;
                    let pending_seek = self.pending_seek.take();
                    let commit_position = if let Some(seek) = pending_seek.as_ref() {
                        seek.position
                    } else {
                        self.output_rebuild_position().await
                    };
                    let commit_position = self.clock.lock().set_anchor(is_playing, commit_position);
                    if (commit_position - position).abs() > 0.025 {
                        let seek_ack = handle.seek(Duration::from_secs_f64(commit_position));
                        let seek_result = match seek_ack {
                            Ok(ack) => ack.wait().await,
                            Err(err) => Err(err),
                        };
                        if let Err(err) = seek_result {
                            warn!(
                                "热重建提交前同步 seek 失败，将按最新位置重新准备解码器: {err:?}"
                            );
                            handle.stop();
                            self.pending_seek = pending_seek;
                            self.spawn_reconfigured_decoder(
                                self.output_refresh_generation,
                                self.output_epoch,
                                output,
                                commit_position,
                            );
                            return;
                        }
                    }
                    if let Some(handle) = self.current_decoder_handle.take() {
                        handle.stop();
                    }
                    if let Some(handle) = self.secondary_decoder_handle.take() {
                        handle.stop();
                    }
                    self.decoder_playback_id = playback_id;
                    let writer = output.writer();
                    let config = output.config();
                    self.output = output;
                    self.mark_output_chain_committed();
                    self.deck_mixer = deck_mixer;
                    self.deck_mixer.set_dsp(self.dsp_config.clone());
                    self.active_deck = DeckId::Primary;
                    self.secondary_playback_id = None;
                    self.native_crossfade_generation =
                        self.native_crossfade_generation.wrapping_add(1);
                    self.native_crossfade_active = false;
                    self.native_crossfade_transition_id = None;
                    self.clock.lock().set_render_clock(
                        writer.render_clock(),
                        config.sample_rate,
                        config.channels,
                    );
                    self.output.writer().set_volume(self.volume as f32);
                    if is_playing {
                        self.current_decoder_handle = Some(handle);
                        self.resume_audio_output().await;
                    } else {
                        self.output.writer().set_paused(true);
                        let _ = handle.set_paused(true);
                        self.current_decoder_handle = Some(handle);
                    }
                    let _ = self.analysis_tx.send(AnalysisCommand::Clear);
                    self.reset_output_health();
                    self.complete_output_refresh();
                    self.clear_output_error();
                    self.emit_output_changed(&self.output).await;
                    self.publish_position_anchor(is_playing, commit_position)
                        .await;
                    if let Some(seek) = pending_seek {
                        self.emit_seek_committed(seek).await;
                    }
                    if is_playing {
                        let _ = self
                            .emitter()
                            .emit(AudioThreadEvent::PlayStatus { is_playing: true })
                            .await;
                    }
                    self.sync_ui().await;
                }
                Err(err) => {
                    self.complete_output_refresh();
                    warn!("切换音频输出后准备新解码器失败，将保留当前播放链路并重试: {err:?}");
                    let is_playing = self.playback_intent == PlaybackIntent::Playing;
                    let position = self.output_rebuild_position().await;
                    self.publish_position_anchor(is_playing, position).await;
                    self.sync_ui().await;
                }
            },
        }
    }

    fn spawn_reconfigured_decoder(
        &mut self,
        generation: u64,
        output_epoch: u64,
        output: LowLatencyOutput,
        position: f64,
    ) {
        let Some(path) = self.current_local_path.clone() else {
            self.complete_output_refresh();
            return;
        };

        let playback_id = self.decoder_playback_id.wrapping_add(1);
        output.writer().set_volume(0.0);
        output.writer().set_paused(true);
        let output_config = output.config();
        let deck_mixer = DeckMixer::new(
            output.writer(),
            output_config.channels,
            output_config.sample_rate,
            &self.dsp_config,
        );
        let output_writer = deck_mixer.primary_writer();
        let analysis_tx = self.analysis_tx.clone();
        let analysis_enabled = Arc::clone(&self.analysis_enabled);
        let decoder_event_tx = self.decoder_event_tx.clone();
        let start_paused = true;
        let seek_position = position.max(0.0);
        let tx = self.output_refresh_tx.clone();

        tokio::task::spawn_blocking(move || {
            let result = decoder::spawn_playback_decoder(
                &path,
                (seek_position > 0.0).then_some(seek_position),
                output_writer,
                output_config.channels,
                output_config.sample_rate,
                analysis_tx,
                analysis_enabled,
                decoder_event_tx,
                playback_id,
                start_paused,
            );
            let _ = tx.send(OutputRefreshEvent::DecoderReady {
                generation,
                output_epoch,
                playback_id,
                position: seek_position,
                output,
                deck_mixer,
                result,
            });
        });
    }

    async fn output_rebuild_position(&self) -> f64 {
        if let Some(seek) = self.pending_seek.as_ref() {
            return seek.position;
        }
        self.clock_position()
    }
}

fn output_audio_layout_matches(a: output::OutputConfigKey, b: output::OutputConfigKey) -> bool {
    a.channels == b.channels && a.sample_rate == b.sample_rate
}
