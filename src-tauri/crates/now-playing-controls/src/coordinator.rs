use std::thread;

use tokio::{
    runtime::Builder,
    sync::mpsc,
    task::LocalSet,
    time::{interval, Duration},
};
use tracing::{error, warn};

use crate::{
    discord::DiscordAdapter,
    model::{
        DiscordConfigPayload, MetadataPayload, PlayModePayload, PlayStatePayload, TimelinePayload,
    },
    platform::PlatformMediaControls,
    EventCallback, NowPlayingOptions,
};

#[derive(Debug)]
pub enum Command {
    UpdateMetadata(MetadataPayload),
    UpdatePlayState(PlayStatePayload),
    UpdatePlaybackRate(f64),
    UpdateVolume(f64),
    UpdateTimeline(TimelinePayload),
    UpdatePlayMode(PlayModePayload),
    EnableDiscord,
    DisableDiscord,
    UpdateDiscordConfig(DiscordConfigPayload),
    EnableSystemMedia,
    DisableSystemMedia,
    Shutdown,
}

pub fn spawn_coordinator_loop(
    options: NowPlayingOptions,
    callback: EventCallback,
    mut rx: mpsc::UnboundedReceiver<Command>,
) {
    thread::spawn(move || {
        let rt = match Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                error!("无法创建 Tokio 运行时: {e}");
                return;
            }
        };

        let local = LocalSet::new();
        local.block_on(&rt, async move {
            let mut os_adapter = match PlatformMediaControls::new(&options, callback).await {
                Ok(adapter) => adapter,
                Err(e) => {
                    error!("初始化媒体控件失败: {e}");
                    return;
                }
            };

            let mut discord_adapter = DiscordAdapter::new(options.discord);

            let mut ticker = interval(Duration::from_secs(5));

            loop {
                tokio::select! {
                    cmd_opt = rx.recv() => {
                        let Some(cmd) = cmd_opt else {
                            break;
                        };

                        if handle_command(cmd, &mut os_adapter, &mut discord_adapter).await {
                            break;
                        }
                    }

                    _ = ticker.tick() => {
                        discord_adapter.tick().await;
                    }
                }
            }

            os_adapter.shutdown();
            discord_adapter.shutdown();
        });
    });
}

#[allow(clippy::future_not_send, clippy::needless_pass_by_ref_mut)]
async fn handle_command(
    cmd: Command,
    os_adapter: &mut PlatformMediaControls,
    discord_adapter: &mut DiscordAdapter,
) -> bool {
    match cmd {
        Command::EnableSystemMedia => {
            if let Err(e) = os_adapter.enable().await {
                warn!("启用系统媒体控件失败: {e}");
            }
        }
        Command::DisableSystemMedia => {
            if let Err(e) = os_adapter.disable().await {
                warn!("禁用系统媒体控件失败: {e}");
            }
        }

        Command::UpdateMetadata(payload) => {
            if let Err(e) = discord_adapter.update_metadata(payload.clone()).await {
                warn!("更新 Discord 元数据失败: {e}");
            }
            if let Err(e) = os_adapter.update_metadata(payload).await {
                warn!("更新系统媒体元数据失败: {e}");
            }
        }
        Command::UpdatePlayState(payload) => {
            if let Err(e) = discord_adapter.update_play_state(payload).await {
                warn!("更新 Discord 播放状态失败: {e}");
            }
            if let Err(e) = os_adapter.update_play_state(payload).await {
                warn!("更新系统媒体播放状态失败: {e}");
            }
        }
        Command::UpdateTimeline(payload) => {
            if let Err(e) = discord_adapter.update_timeline(payload).await {
                warn!("更新 Discord 时间线失败: {e}");
            }
            if let Err(e) = os_adapter.update_timeline(payload).await {
                warn!("更新系统媒体时间线失败: {e}");
            }
        }
        Command::UpdatePlaybackRate(rate) => {
            if let Err(e) = os_adapter.update_playback_rate(rate).await {
                warn!("更新播放速率失败: {e}");
            }
        }
        Command::UpdateVolume(volume) => {
            if let Err(e) = os_adapter.update_volume(volume).await {
                warn!("更新音量失败: {e}");
            }
        }
        Command::UpdatePlayMode(payload) => {
            if let Err(e) = os_adapter.update_play_mode(payload).await {
                warn!("更新播放模式失败: {e}");
            }
        }

        Command::EnableDiscord => {
            if let Err(e) = discord_adapter.enable().await {
                warn!("启用 Discord RPC 失败: {e}");
            }
        }
        Command::DisableDiscord => {
            if let Err(e) = discord_adapter.disable().await {
                warn!("禁用 Discord RPC 失败: {e}");
            }
        }
        Command::UpdateDiscordConfig(payload) => {
            if let Err(e) = discord_adapter.update_config(payload).await {
                warn!("更新 Discord 配置失败: {e}");
            }
        }

        Command::Shutdown => {
            return true;
        }
    }

    false
}
