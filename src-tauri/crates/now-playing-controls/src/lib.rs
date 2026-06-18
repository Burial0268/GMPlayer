//! 用于将播放信息同步到系统的媒体控件和/或 Discord RPC 的 Rust crate
//!
//! 目前支持 Windows、Linux 和 MacOS 的媒体控件交互

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc;

mod coordinator;
mod discord;
pub mod model;
mod platform;

use crate::{
    coordinator::Command,
    model::{
        DiscordConfigPayload, MetadataPayload, NowPlayingOptions, PlayModePayload,
        PlayStatePayload, SystemMediaEvent, TimelinePayload,
    },
};

pub type EventCallback = Arc<dyn Fn(SystemMediaEvent) + Send + Sync>;

#[derive(Clone)]
pub struct NowPlayingSession {
    tx: mpsc::UnboundedSender<Command>,
}

impl NowPlayingSession {
    pub fn new(options: NowPlayingOptions, callback: EventCallback) -> Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel();

        coordinator::spawn_coordinator_loop(options, callback, rx);

        Ok(Self { tx })
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(Command::Shutdown);
    }

    pub fn enable_system_media(&self) {
        let _ = self.tx.send(Command::EnableSystemMedia);
    }

    pub fn disable_system_media(&self) {
        let _ = self.tx.send(Command::DisableSystemMedia);
    }

    pub fn update_metadata(&self, payload: MetadataPayload) {
        let _ = self.tx.send(Command::UpdateMetadata(payload));
    }

    pub fn update_play_state(&self, payload: PlayStatePayload) {
        let _ = self.tx.send(Command::UpdatePlayState(payload));
    }

    pub fn update_playback_rate(&self, rate: f64) {
        let _ = self.tx.send(Command::UpdatePlaybackRate(rate));
    }

    pub fn update_volume(&self, volume: f64) {
        let _ = self.tx.send(Command::UpdateVolume(volume));
    }

    pub fn update_timeline(&self, payload: TimelinePayload) {
        let _ = self.tx.send(Command::UpdateTimeline(payload));
    }

    pub fn update_play_mode(&self, payload: PlayModePayload) {
        let _ = self.tx.send(Command::UpdatePlayMode(payload));
    }

    pub fn enable_discord_rpc(&self) {
        let _ = self.tx.send(Command::EnableDiscord);
    }

    pub fn disable_discord_rpc(&self) {
        let _ = self.tx.send(Command::DisableDiscord);
    }

    pub fn update_discord_config(&self, payload: DiscordConfigPayload) {
        let _ = self.tx.send(Command::UpdateDiscordConfig(payload));
    }
}
