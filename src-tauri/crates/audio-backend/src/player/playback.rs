//! Track loading and natural playback progression: source resolution
//! (download/local reuse), decoder spawning, prebuffer-gated output resume,
//! and the decoder-finished → gapless-advance hop.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tracing::warn;

use crate::analysis::AnalysisCommand;
use crate::decoder;
use crate::error::AudioResult;
use crate::types::{
    AudioInfo, AudioQuality, AudioThreadEvent, AudioThreadEventMessage, AudioThreadMessage,
    DisplayAudioInfo, SongData,
};

use super::mixer::DeckId;
use super::platform::{
    seek_prebuffer_samples, start_prebuffer_samples, SEEK_PREBUFFER_WAIT_MS,
    START_PREBUFFER_WAIT_MS,
};
use super::{AudioPlayer, PlaybackIntent};

impl AudioPlayer {
    pub(super) async fn handle_decoder_finished(&mut self, playback_id: u64) {
        if self.native_crossfade_active
            && (playback_id == self.decoder_playback_id
                || Some(playback_id) == self.secondary_playback_id)
        {
            return;
        }

        if playback_id != self.decoder_playback_id || self.current_song.is_none() {
            return;
        }

        let finished_id = self
            .current_song
            .as_ref()
            .map(|s| s.get_id())
            .unwrap_or_default();
        self.current_decoder_handle = None;
        self.publish_position_anchor(false, 0.0).await;
        self.current_song = None;
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::AudioPlayFinished {
                music_id: finished_id,
            })
            .await;

        if !self.playlist.is_empty() {
            let _ = self.self_msg_tx.send(AudioThreadEventMessage::new(
                "".into(),
                Some(AudioThreadMessage::NextSongGapless),
            ));
        }
    }

    pub(super) async fn resume_audio_output(&self) {
        if let Some(handle) = &self.current_decoder_handle {
            let _ = handle.set_paused(false);
        }
        if let Some(handle) = &self.secondary_decoder_handle {
            let _ = handle.set_paused(false);
        }

        self.wait_for_start_prebuffer().await;
        self.output.writer().set_paused(false);
    }

    async fn wait_for_start_prebuffer(&self) {
        self.wait_for_prebuffer(
            start_prebuffer_samples,
            START_PREBUFFER_WAIT_MS,
            "音频输出预缓冲不足",
        )
        .await;
    }

    pub(super) async fn wait_for_seek_prebuffer(&self) {
        self.wait_for_prebuffer(
            seek_prebuffer_samples,
            SEEK_PREBUFFER_WAIT_MS,
            "seek 快速预缓冲不足",
        )
        .await;
    }

    async fn wait_for_prebuffer(
        &self,
        target_samples_for: fn(usize, u32) -> usize,
        wait_ms: u64,
        warning: &str,
    ) {
        if self.current_decoder_handle.is_none() && self.secondary_decoder_handle.is_none() {
            return;
        }

        let writer = self.output.writer();
        let output_config = self.output.config();
        let channels = output_config.channels.max(1) as usize;
        let target_samples = target_samples_for(channels, output_config.sample_rate);
        for _ in 0..wait_ms {
            if writer.queued_samples() >= target_samples {
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let queued = writer.queued_samples();
        if queued < target_samples {
            warn!(
                "{}: queued_samples={} target_samples={}",
                warning, queued, target_samples
            );
        }
    }

    pub(super) async fn start_playing_song(
        &mut self,
        clear_sink: bool,
        initial_position: Option<f64>,
    ) -> anyhow::Result<()> {
        self.pending_seek = None;
        let song_data = self
            .current_song
            .clone()
            .ok_or_else(|| anyhow::anyhow!("没有当前歌曲可播放"))?;

        let file_path = match &song_data {
            SongData::Local { file_path, .. } => file_path.clone(),
            _ => return Err(anyhow::anyhow!("当前实现仅支持本地文件 / HTTP(S) 流")),
        };

        // Emit LoadingAudio so the frontend can show a spinner / await load.
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::LoadingAudio {
                music_id: song_data.get_id(),
                current_play_index: self.current_play_index,
            })
            .await;

        // Replaying or advancing back onto the file that is already resolved
        // locally (repeat-one wrap, same-track restart) must not re-download
        // the source: take the temp guard out before the sink clear below
        // drops it, and reuse the on-disk copy.
        let reused_source = if self.current_file_path.as_deref() == Some(file_path.as_str()) {
            self.current_local_path
                .clone()
                .filter(|path| path.exists())
                .map(|path| (path, self.current_temp_file.take()))
        } else {
            None
        };

        if clear_sink {
            self.cancel_pending_output_refresh();
            self.automix_prepare_generation = self.automix_prepare_generation.wrapping_add(1);
            if let Some(handle) = self.current_decoder_handle.take() {
                handle.stop();
            }
            if let Some(handle) = self.secondary_decoder_handle.take() {
                handle.stop();
            }
            self.deck_mixer.clear_all();

            let _ = self.analysis_tx.send(AnalysisCommand::Clear);

            // Drop the previous temp file (if any) by clearing the guard. This
            // must happen BEFORE we assign the new temp path so disk usage stays
            // bounded.
            self.current_local_path = None;
            self.current_temp_file = None;
            self.secondary_local_path = None;
            self.secondary_temp_file = None;
            self.secondary_song = None;
            self.secondary_duration = 0.0;
            self.secondary_display_info = None;
            self.secondary_quality = None;
            self.secondary_playback_id = None;
            self.active_deck = DeckId::Primary;
            self.deck_mixer.set_deck_gain(DeckId::Primary, 1.0);
            self.deck_mixer.set_deck_gain(DeckId::Secondary, 0.0);
            // Fresh (non-automix) load starts unnormalized at unity gain.
            self.active_norm_gain = 1.0;
            self.secondary_norm_gain = 1.0;
            self.native_crossfade_generation = self.native_crossfade_generation.wrapping_add(1);
            self.native_crossfade_active = false;
        }

        self.current_file_path = Some(file_path.clone());

        // Resolve URL → local path (download to temp) so rodio's File-based
        // decoder can read it.
        let (local_path, temp_file) = if let Some(reused) = reused_source {
            reused
        } else {
            let resolve_path = file_path.clone();
            let resolve_result = tokio::task::spawn_blocking(
                move || -> AudioResult<(PathBuf, Option<tempfile::TempPath>)> {
                    if decoder::is_http_url(&resolve_path) {
                        let temp = decoder::download_to_temp_path(&resolve_path)?;
                        let path = temp.to_path_buf();
                        Ok((path, Some(temp)))
                    } else {
                        Ok((PathBuf::from(&resolve_path), None))
                    }
                },
            )
            .await?;

            match resolve_result {
                Ok(t) => t,
                Err(e) => {
                    warn!("解析音频源失败: {e:?}");
                    let _ = self
                        .emitter()
                        .emit(AudioThreadEvent::LoadError {
                            error: e.to_string(),
                        })
                        .await;
                    return Err(e.into());
                }
            }
        };

        self.current_local_path = Some(local_path.clone());
        self.current_temp_file = temp_file;

        // Read metadata before opening output so the CPAL stream can match the
        // source's channel count and sample rate where the device supports it.
        let path_for_info = local_path.clone();
        let info_result = tokio::task::spawn_blocking(move || {
            decoder::symphonia::extract_metadata_only(&path_for_info)
        })
        .await?;

        let audio_info = match info_result {
            Ok(info) => info,
            Err(e) => {
                warn!("读取音频元数据失败: {e:?}");
                let _ = self
                    .emitter()
                    .emit(AudioThreadEvent::LoadError {
                        error: e.to_string(),
                    })
                    .await;
                return Err(e.into());
            }
        };

        let _output_reopened = self.ensure_output_for_source(&audio_info)?;
        self.output.writer().set_volume(self.volume as f32);
        // Keep the device callback silent until the decoder/mixer has filled
        // the output queue; otherwise Android can underrun before the first PCM block arrives.
        self.output.writer().set_paused(true);

        // `initial_position` is applied inside the decoder worker before it
        // starts pushing PCM, avoiding a separate post-load seek round trip.
        let analysis_tx_for_open = self.analysis_tx.clone();
        let path_for_open = local_path.clone();
        let output_writer = match self.active_deck {
            DeckId::Primary => self.deck_mixer.primary_writer(),
            DeckId::Secondary => self.deck_mixer.secondary_writer(),
        };
        let output_config = self.output.config();
        self.decoder_playback_id = self.decoder_playback_id.wrapping_add(1);
        let playback_id = self.decoder_playback_id;
        let decoder_event_tx = self.decoder_event_tx.clone();
        let start_paused = self.playback_intent == PlaybackIntent::Paused;
        let seek_into_open = initial_position.filter(|p| *p > 0.0);
        let analysis_enabled_for_open = Arc::clone(&self.analysis_enabled);

        let open_result = tokio::task::spawn_blocking(move || {
            decoder::spawn_playback_decoder(
                &path_for_open,
                seek_into_open,
                output_writer,
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

        let handle = match open_result {
            Ok(handle) => handle,
            Err(e) => {
                warn!("打开音频源失败: {e:?}");
                let _ = self
                    .emitter()
                    .emit(AudioThreadEvent::LoadError {
                        error: e.to_string(),
                    })
                    .await;
                return Err(e.into());
            }
        };

        self.current_decoder_handle = Some(handle);

        // The starting position the position task should anchor at.
        let anchor_pos = seek_into_open.unwrap_or(0.0);

        // Use the symphonia-extracted duration; rodio's `total_duration()` is
        // unreliable for MP3 without VBR headers and similar.
        let display_info = DisplayAudioInfo {
            name: extract_title_from_metadata(&audio_info).unwrap_or_else(|| {
                Path::new(&file_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unknown")
                    .to_string()
            }),
            artist: extract_tag(&audio_info, &["artist", "TPE1"]).unwrap_or_default(),
            album: extract_tag(&audio_info, &["album", "TALB"]).unwrap_or_default(),
            duration: audio_info.duration_secs,
            // Carry the initial position so the frontend's `_state.position` is
            // seeded correctly from the very first `LoadAudio` / `SyncStatus`.
            position: anchor_pos,
            ..Default::default()
        };

        let quality = AudioQuality {
            sample_rate: audio_info.sample_rate,
            channels: audio_info.channels,
            bitrate: audio_info.bitrate_bps.map(|b| b as u32).unwrap_or_default(),
        };

        *self.current_audio_info.write().await = display_info.clone();
        *self.current_audio_quality.write().await = quality.clone();
        self.clock.lock().set_duration(audio_info.duration_secs);

        let is_now_playing = self.playback_intent == PlaybackIntent::Playing;
        self.publish_position_anchor(is_now_playing, anchor_pos)
            .await;

        let _ = self
            .emitter()
            .emit(AudioThreadEvent::LoadAudio {
                music_id: song_data.get_id(),
                music_info: display_info,
                quality,
                current_play_index: self.current_play_index,
            })
            .await;
        if is_now_playing {
            self.resume_audio_output().await;
            let _ = self
                .emitter()
                .emit(AudioThreadEvent::PlayStatus { is_playing: true })
                .await;
        }

        self.sync_ui().await;
        Ok(())
    }
}

// ── Metadata helpers ─────────────────────────────────────────────

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
