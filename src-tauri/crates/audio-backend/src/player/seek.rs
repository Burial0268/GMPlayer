//! Seek processing: request validation against the current track, in-place
//! decoder seeks with prebuffer gating, and deferred-seek handling while the
//! decoder is unavailable.

use std::time::Duration;

use tracing::warn;

use crate::analysis::AnalysisCommand;
use crate::types::SongData;

use super::api::SeekRequest;
use super::{AudioPlayer, PlaybackIntent};

impl AudioPlayer {
    fn seek_matches_current_song(&self, seek: &SeekRequest) -> bool {
        let Some(expected) = seek.expected_music_id.as_deref() else {
            return true;
        };
        self.current_song
            .as_ref()
            .is_some_and(|song| song.get_id() == expected)
    }

    async fn apply_pending_seek_if_ready(&mut self) -> anyhow::Result<bool> {
        let Some(seek) = self.pending_seek.clone() else {
            return Ok(false);
        };
        if !self.seek_matches_current_song(&seek) {
            self.pending_seek = None;
            warn!(
                "忽略已过期 seek 请求: {:.3}s, expected={:?}",
                seek.position, seek.expected_music_id
            );
            self.emit_seek_failed(seek, "seek 请求所属歌曲已不是当前歌曲")
                .await;
            self.sync_ui().await;
            return Ok(true);
        }
        let Some(handle) = self.current_decoder_handle.as_ref() else {
            return Ok(false);
        };

        let was_playing = self.playback_intent == PlaybackIntent::Playing;
        if was_playing {
            self.output.writer().set_paused(true);
        }

        let seek_ack = handle.seek(Duration::from_secs_f64(seek.position));
        let seek_result = match seek_ack {
            Ok(ack) => ack.wait().await,
            Err(err) => Err(err),
        };
        if let Err(err) = seek_result {
            if was_playing {
                self.output.writer().set_paused(false);
            }
            self.pending_seek = None;
            let error = err.to_string();
            warn!("decoder 原地 seek 失败: {:.3}s, {error}", seek.position);
            self.emit_seek_failed(seek, error).await;
            self.sync_ui().await;
            return Ok(true);
        }
        self.pending_seek = None;
        let _ = self.analysis_tx.send(AnalysisCommand::Clear);
        if was_playing {
            // Keep the deep Android queues for steady-state playback, while
            // using a much smaller first-audio watermark after an in-place seek.
            self.wait_for_seek_prebuffer().await;
            self.output.writer().set_paused(false);
        }
        self.publish_position_anchor(was_playing, seek.position)
            .await;
        self.reset_output_health();
        self.emit_seek_committed(seek).await;
        self.sync_ui().await;
        Ok(true)
    }

    pub(super) async fn process_seek_request(
        &mut self,
        request: SeekRequest,
    ) -> anyhow::Result<()> {
        let seek = request.normalized();
        if self.current_song.is_none() {
            self.pending_seek = None;
            warn!("没有当前歌曲, 忽略 seek 请求: {:.3}s", seek.position);
            self.emit_seek_failed(seek, "没有当前歌曲可 seek").await;
            return Ok(());
        }

        if !self.seek_matches_current_song(&seek) {
            warn!(
                "忽略非当前歌曲 seek 请求: {:.3}s, expected={:?}, current={:?}",
                seek.position,
                seek.expected_music_id,
                self.current_song.as_ref().map(SongData::get_id)
            );
            self.emit_seek_failed(seek, "seek 请求所属歌曲已不是当前歌曲")
                .await;
            self.sync_ui().await;
            return Ok(());
        }

        self.cancel_native_automix_runtime().await;
        self.automix_prepare_generation = self.automix_prepare_generation.wrapping_add(1);
        let events = self.automix.cancel(self.current_play_index);
        self.emit_many(events).await;

        self.pending_seek = Some(seek.clone());
        match self.apply_pending_seek_if_ready().await {
            Ok(true) => {}
            Ok(false) => {
                let is_playing = self.playback_intent == PlaybackIntent::Playing;
                self.publish_position_anchor(is_playing, seek.position)
                    .await;
                self.sync_ui().await;
                warn!("解码器暂不可用, 已延后 seek 到 {:.3}s", seek.position);
            }
            Err(err) => {
                self.pending_seek = None;
                let error = err.to_string();
                warn!("执行 seek 失败: {error}");
                self.emit_seek_failed(seek, error).await;
                self.sync_ui().await;
            }
        }
        Ok(())
    }
}
