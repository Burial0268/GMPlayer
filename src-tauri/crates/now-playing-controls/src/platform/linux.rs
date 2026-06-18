use std::{
    io::Write,
    process,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use mpris_server::{
    zbus::zvariant::ObjectPath, LoopStatus as MprisLoopStatus, Metadata,
    PlaybackStatus as MprisPlaybackStatus, Player, Time,
};
use tempfile::NamedTempFile;
use tokio::task::JoinHandle;
use tracing::{debug, error, info};

use crate::{
    model::{
        MetadataPayload, PlayModePayload, PlayStatePayload, PlaybackStatus, RepeatMode,
        SystemMediaEvent, SystemMediaEventType, TimelinePayload,
    },
    EventCallback, NowPlayingOptions,
};

pub struct LinuxImpl {
    player: Player,
    server_task: Option<JoinHandle<()>>,
    cover_file_guard: Option<NamedTempFile>,
    is_enabled: Arc<AtomicBool>,
}

#[allow(clippy::unused_async, clippy::future_not_send)]
impl LinuxImpl {
    pub async fn new(options: &NowPlayingOptions, callback: EventCallback) -> Result<Self> {
        info!("正在初始化 Linux MPRIS...");

        let app_name = options.app_name.as_deref();
        let pid = process::id();
        // 使用唯一标识符以避免多个实例冲突
        let identity = app_name.map_or_else(
            || format!("player.instance{pid}"),
            |name| format!("{name}.instance{pid}"),
        );

        let mut builder = Player::builder(&identity)
            .can_play(true)
            .can_pause(true)
            .can_go_next(true)
            .can_go_previous(true)
            .can_seek(true)
            .can_control(true)
            .minimum_rate(0.2)
            .maximum_rate(2.0)
            .playback_status(MprisPlaybackStatus::Stopped);

        if let Some(name) = app_name {
            builder = builder.identity(name).desktop_entry(name);
        }

        let player = builder
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("MPRIS Player 初始化失败: {e}"))?;

        let is_enabled = Arc::new(AtomicBool::new(false));

        Self::setup_mpris_signals(&player, callback, is_enabled.clone());

        let run_task = player.run();
        let server_task = tokio::task::spawn_local(async move {
            let () = run_task.await;
        });

        Ok(Self {
            player,
            server_task: Some(server_task),
            cover_file_guard: None,
            is_enabled,
        })
    }

    fn setup_mpris_signals(player: &Player, cb: EventCallback, is_enabled: Arc<AtomicBool>) {
        let dispatch = move |evt: SystemMediaEvent| {
            if is_enabled.load(Ordering::Relaxed) {
                cb(evt);
            }
        };

        // 播放
        let d = dispatch.clone();
        player.connect_play(move |_| {
            debug!("收到 play 命令");
            d(SystemMediaEvent::new(SystemMediaEventType::Play));
        });

        // 暂停
        let d = dispatch.clone();
        player.connect_pause(move |_| {
            debug!("收到 pause 命令");
            d(SystemMediaEvent::new(SystemMediaEventType::Pause));
        });

        // Toggle
        let d = dispatch.clone();
        player.connect_play_pause(move |p| {
            debug!("收到 play_pause 命令");
            let status = p.playback_status();
            let evt_type = if status == MprisPlaybackStatus::Playing {
                SystemMediaEventType::Pause
            } else {
                SystemMediaEventType::Play
            };
            d(SystemMediaEvent::new(evt_type));
        });

        // 上一首
        let d = dispatch.clone();
        player.connect_previous(move |_| {
            debug!("收到 previous 命令");
            d(SystemMediaEvent::new(SystemMediaEventType::PreviousSong));
        });

        // 下一首
        let d = dispatch.clone();
        player.connect_next(move |_| {
            debug!("收到 next 命令");
            d(SystemMediaEvent::new(SystemMediaEventType::NextSong));
        });

        // 停止
        let d = dispatch.clone();
        player.connect_stop(move |_| {
            debug!("收到 stop 命令");
            d(SystemMediaEvent::new(SystemMediaEventType::Stop));
        });

        let d = dispatch.clone();
        player.connect_set_loop_status(move |_, new_status| {
            debug!(?new_status, "收到 set_loop_status 命令");
            d(SystemMediaEvent::new(SystemMediaEventType::ToggleRepeat));
        });

        let d = dispatch.clone();
        player.connect_set_shuffle(move |_, new_val| {
            debug!(?new_val, "收到 set_shuffle 命令");
            d(SystemMediaEvent::new(SystemMediaEventType::ToggleShuffle));
        });

        // 播放速率
        let d = dispatch.clone();
        player.connect_set_rate(move |_, new_rate| {
            debug!(?new_rate, "收到 set_rate 命令");
            d(SystemMediaEvent::set_rate(new_rate));
        });

        // 音量
        let d = dispatch.clone();
        player.connect_set_volume(move |_, new_volume| {
            debug!(?new_volume, "收到 set_volume 命令");
            d(SystemMediaEvent::set_volume(new_volume));
        });

        // 相对跳转
        // 通过 Player 内部维护的进度来计算绝对跳转位置
        let d = dispatch.clone();
        player.connect_seek(move |p, offset| {
            debug!(?offset, "收到 seek 命令");
            let target_micros = p
                .position()
                .as_micros()
                .saturating_add(offset.as_micros())
                .max(0);
            d(SystemMediaEvent::seek(Duration::from_micros(
                target_micros as u64,
            )));
        });

        // 绝对跳转
        player.connect_set_position(move |_, trackid, position| {
            debug!(?position, ?trackid, "收到 set_position 命令");
            dispatch(SystemMediaEvent::seek(Duration::from_micros(
                position.as_micros() as u64,
            )));
        });
    }

    pub async fn enable(&self) -> Result<()> {
        self.is_enabled.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub async fn disable(&self) -> Result<()> {
        self.is_enabled.store(false, Ordering::Relaxed);
        self.player
            .set_playback_status(MprisPlaybackStatus::Stopped)
            .await?;
        self.player.set_metadata(Metadata::new()).await?;
        Ok(())
    }

    pub async fn update_metadata(&mut self, payload: MetadataPayload) -> Result<()> {
        if !self.is_enabled.load(Ordering::Relaxed) {
            return Ok(());
        }

        let art_url = if let Some(data) = payload.cover_data {
            let file_result = tokio::task::spawn_blocking(move || -> Result<_> {
                let mut file = tempfile::Builder::new().suffix(".jpg").tempfile()?;
                file.write_all(&data)?;

                let path = file.path().to_string_lossy().to_string();
                Ok((file, path))
            })
            .await;

            match file_result {
                Ok(Ok((file, path))) => {
                    let url = format!("file://{path}");
                    self.cover_file_guard = Some(file);
                    Some(url)
                }
                Ok(Err(e)) => {
                    error!("写入临时封面文件失败: {e:?}");
                    None
                }
                Err(e) => {
                    error!("写入临时封面文件任务失败: {e:?}");
                    None
                }
            }
        } else {
            self.cover_file_guard = None;
            payload.original_cover_url
        };

        let mut mb = Metadata::builder()
            .title(payload.song_name)
            .artist([payload.author_name])
            .album(payload.album_name);

        let track_id_str = payload.track_id.map_or_else(
            || {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    .to_string()
            },
            |id| id.to_string(),
        );

        let track_path = format!("/com/player/track/{track_id_str}");

        if let Ok(op) = ObjectPath::try_from(track_path.as_str()) {
            mb = mb.trackid(op);
        } else {
            error!("生成的 Track ID 不符合 D-Bus 路径规范: {track_path}");
        }

        if !payload.genre.is_empty() {
            mb = mb.genre(payload.genre);
        }

        if let Some(dur) = payload.duration {
            mb = mb.length(Time::from_micros(dur.as_micros() as i64));
        }

        if let Some(url) = art_url {
            mb = mb.art_url(url);
        }

        self.player.set_metadata(mb.build()).await?;
        self.player.set_position(Time::from_millis(0));
        Ok(())
    }

    pub async fn update_play_state(&self, payload: PlayStatePayload) -> Result<()> {
        if !self.is_enabled.load(Ordering::Relaxed) {
            return Ok(());
        }
        let status = match payload.status {
            PlaybackStatus::Playing => MprisPlaybackStatus::Playing,
            PlaybackStatus::Paused => MprisPlaybackStatus::Paused,
        };
        self.player.set_playback_status(status).await?;
        Ok(())
    }

    pub async fn update_playback_rate(&self, rate: f64) -> Result<()> {
        if !self.is_enabled.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.player.set_rate(rate).await?;
        Ok(())
    }

    pub async fn update_volume(&self, volume: f64) -> Result<()> {
        if !self.is_enabled.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.player.set_volume(volume).await?;
        Ok(())
    }

    pub async fn update_timeline(&self, payload: TimelinePayload) -> Result<()> {
        if !self.is_enabled.load(Ordering::Relaxed) {
            return Ok(());
        }
        let pos = Time::from_micros(payload.current_time.as_micros() as i64);
        self.player.set_position(pos);
        // seek 操作时发出 Seeked D-Bus 信号，通知外部客户端立即刷新进度
        if payload.seeked.unwrap_or(false) {
            self.player.seeked(pos).await?;
        }
        Ok(())
    }

    pub async fn update_play_mode(&self, payload: PlayModePayload) -> Result<()> {
        if !self.is_enabled.load(Ordering::Relaxed) {
            return Ok(());
        }
        let loop_status = match payload.repeat_mode {
            RepeatMode::None => MprisLoopStatus::None,
            RepeatMode::Track => MprisLoopStatus::Track,
            RepeatMode::List => MprisLoopStatus::Playlist,
        };
        self.player.set_loop_status(loop_status).await?;
        self.player.set_shuffle(payload.is_shuffling).await?;
        Ok(())
    }

    pub fn shutdown(&mut self) {
        self.is_enabled.store(false, Ordering::Relaxed);
        if let Some(task) = self.server_task.take() {
            task.abort();
        }
    }
}

impl Drop for LinuxImpl {
    fn drop(&mut self) {
        self.shutdown();
    }
}
