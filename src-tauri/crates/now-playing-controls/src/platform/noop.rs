use anyhow::Result;

use crate::{
    model::{MetadataPayload, PlayModePayload, PlayStatePayload, TimelinePayload},
    EventCallback, NowPlayingOptions,
};

pub struct NoOpImpl;

#[allow(clippy::unused_async)]
impl NoOpImpl {
    pub async fn new(_options: &NowPlayingOptions, _callback: EventCallback) -> Result<Self> {
        Ok(Self)
    }

    pub async fn enable(&self) -> Result<()> {
        Ok(())
    }

    pub async fn disable(&self) -> Result<()> {
        Ok(())
    }

    pub async fn update_metadata(&self, _payload: MetadataPayload) -> Result<()> {
        Ok(())
    }

    pub async fn update_play_state(&self, _payload: PlayStatePayload) -> Result<()> {
        Ok(())
    }

    pub async fn update_playback_rate(&self, _rate: f64) -> Result<()> {
        Ok(())
    }

    pub async fn update_volume(&self, _volume: f64) -> Result<()> {
        Ok(())
    }

    pub async fn update_timeline(&self, _payload: TimelinePayload) -> Result<()> {
        Ok(())
    }

    pub async fn update_play_mode(&self, _payload: PlayModePayload) -> Result<()> {
        Ok(())
    }

    #[allow(clippy::unused_self, clippy::missing_const_for_fn)]
    pub fn shutdown(&self) {}
}
