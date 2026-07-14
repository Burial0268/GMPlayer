//! Event emission and status reporting: the `EventEmitter` handle plus the
//! position-anchor / seek-ack / SyncStatus publishing helpers shared by every
//! other `AudioPlayer` concern.

use tokio::sync::mpsc;

use crate::types::{AudioThreadEvent, AudioThreadEventMessage, AudioThreadMessage};

use super::api::SeekRequest;
use super::AudioPlayer;

impl AudioPlayer {
    pub(super) fn emitter(&self) -> EventEmitter {
        EventEmitter::new(self.evt_sender.clone())
    }

    pub(super) async fn emit_many(&self, events: Vec<AudioThreadEvent>) {
        let emitter = self.emitter();
        for event in events {
            let _ = emitter.emit(event).await;
        }
    }

    pub(super) fn clock_position(&self) -> f64 {
        self.clock.lock().position()
    }

    pub(super) async fn publish_position_anchor(&self, is_playing: bool, position: f64) {
        let position = {
            let mut clock = self.clock.lock();
            clock.set_anchor(is_playing, position)
        };
        *self.current_position.write().await = position;
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::PlayPosition { position })
            .await;
    }

    pub(super) async fn emit_seek_committed(&self, seek: SeekRequest) {
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::SeekCommitted {
                request_id: seek.request_id,
                position: seek.position,
            })
            .await;
    }

    pub(super) async fn emit_seek_failed(&self, seek: SeekRequest, error: impl Into<String>) {
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::SeekFailed {
                request_id: seek.request_id,
                position: seek.position,
                error: error.into(),
            })
            .await;
    }

    pub(super) async fn sync_ui(&self) {
        let audio_info = self.current_audio_info.read().await.clone();
        let (position, is_playing) = {
            let clock = self.clock.lock();
            (clock.position(), clock.is_playing())
        };
        *self.current_position.write().await = position;
        let quality = self.current_audio_quality.read().await.clone();
        let duration = audio_info.duration;

        let status_event = AudioThreadEvent::SyncStatus {
            music_id: self
                .current_song
                .as_ref()
                .map(|s| s.get_id())
                .unwrap_or_default(),
            music_info: audio_info,
            is_playing,
            duration,
            position,
            volume: self.volume,
            load_position: 0.0,
            playlist_inited: self.playlist_inited,
            playlist: self.playlist.clone(),
            current_play_index: self.current_play_index,
            quality,
        };
        let _ = self.emitter().emit(status_event).await;
    }
}

// ── EventEmitter helper (mirrors AMLL's AudioPlayerEventEmitter) ──

#[derive(Debug, Clone)]
pub(super) struct EventEmitter {
    evt_sender: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
}

impl EventEmitter {
    pub(super) fn new(
        evt_sender: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
    ) -> Self {
        Self { evt_sender }
    }

    pub(super) async fn emit(&self, event: AudioThreadEvent) -> anyhow::Result<()> {
        self.evt_sender
            .send(AudioThreadEventMessage::new("".into(), Some(event)))
            .map_err(|_| anyhow::anyhow!("event channel closed"))?;
        Ok(())
    }

    pub(super) async fn ret_none(
        &self,
        req: AudioThreadEventMessage<AudioThreadMessage>,
    ) -> anyhow::Result<()> {
        self.evt_sender
            .send(req.to_none::<AudioThreadEvent>())
            .map_err(|_| anyhow::anyhow!("event channel closed"))?;
        Ok(())
    }
}
