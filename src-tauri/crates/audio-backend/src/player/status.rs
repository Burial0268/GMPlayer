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

    /// Retire the current timeline and start a new one.
    ///
    /// Call this immediately before anchoring a track that is starting from its
    /// own source — never for a seek, a pause, or an output-device rebuild,
    /// which all continue the timeline they are already on.
    pub(super) fn begin_timeline(&self) -> u64 {
        self.clock.lock().begin_epoch()
    }

    pub(super) async fn publish_position_anchor(&self, is_playing: bool, position: f64) {
        let (position, timeline_epoch) = {
            let mut clock = self.clock.lock();
            (clock.set_anchor(is_playing, position), clock.epoch())
        };
        *self.current_position.write().await = position;
        {
            let mut session = self.session.lock();
            session.position = position;
            session.is_playing = is_playing;
        }
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::PlayPosition {
                position,
                timeline_epoch,
            })
            .await;
    }

    /// Mark the session as carrying no track. Called when playback finishes and
    /// nothing has taken over yet, so a frontend booting in that window falls
    /// back to its own startup path instead of adopting a retired track.
    pub(super) fn clear_session_track(&self) {
        let mut session = self.session.lock();
        session.has_track = false;
        session.music_id.clear();
        session.identity = None;
        session.is_playing = false;
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
        let (position, is_playing, timeline_epoch) = {
            let clock = self.clock.lock();
            (clock.position(), clock.is_playing(), clock.epoch())
        };
        *self.current_position.write().await = position;
        let quality = self.current_audio_quality.read().await.clone();
        let duration = audio_info.duration;
        let music_id = self
            .current_song
            .as_ref()
            .map(|s| s.get_id())
            .unwrap_or_default();

        // Refresh the synchronously-readable snapshot from the same values the
        // event carries, so `audio_get_session` and `SyncStatus` can never
        // disagree about what is playing.
        {
            let mut session = self.session.lock();
            session.has_track = self.current_song.is_some();
            session.music_id = music_id.clone();
            session.identity = self.current_identity.clone();
            session.playlist_index = self.current_play_index;
            session.position = position;
            session.duration = duration;
            session.is_playing = is_playing;
            session.volume = self.volume;
            session.manifest_revision = self.manifest.revision();
            session.planner_active = self.planner_can_advance();
        }

        // Resolved display metadata for the in-process media-session bridge.
        // Built before `audio_info` is moved into the status event; emitted
        // from here so it tracks every path that can change what is playing
        // (track load, AutoMix completion, output rebuild, seek). Consumers
        // dedup on `same_metadata`.
        let now_playing = self.now_playing_info(&audio_info, position, is_playing);

        let status_event = AudioThreadEvent::SyncStatus {
            music_id,
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
            identity: self.current_identity.clone(),
            timeline_epoch,
        };
        let _ = self.emitter().emit(status_event).await;
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::NowPlayingChanged { info: now_playing })
            .await;
    }

    /// Publish only the OS media-session projection, with no `SyncStatus`.
    ///
    /// For changes that exist purely for the session — announcing the track a
    /// backend-driven advance is heading for, and retracting it if that
    /// advance fails. Both happen in the window where `current_song` is
    /// already `None`, so a full `sync_ui` would push an empty transport
    /// snapshot at a frontend that has no use for it.
    pub(super) async fn publish_now_playing(&self) {
        let audio_info = self.current_audio_info.read().await.clone();
        let (position, is_playing) = {
            let clock = self.clock.lock();
            (clock.position(), clock.is_playing())
        };
        let info = self.now_playing_info(&audio_info, position, is_playing);
        let _ = self
            .emitter()
            .emit(AudioThreadEvent::NowPlayingChanged { info })
            .await;
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
