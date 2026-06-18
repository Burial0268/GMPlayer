use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use discord_rich_presence::{
    activity::{Activity, ActivityType, Assets, Button, StatusDisplayType, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use tokio::time::Instant;
use tracing::{debug, info, warn};

use crate::model::{
    DiscordConfigPayload, DiscordDisplayMode, DiscordOptions, MetadataPayload, PlayStatePayload,
    PlaybackStatus, TimelinePayload,
};

const TIMESTAMP_UPDATE_THRESHOLD_MS: i64 = 100;
const RECONNECT_COOLDOWN_SECONDS: u64 = 5;

#[derive(Debug, Clone, PartialEq)]
struct ActivityData {
    metadata: MetadataPayload,
    status: PlaybackStatus,
    current_time: Duration,
    cached_cover_url: String,
}

impl ActivityData {
    fn from_metadata(metadata: MetadataPayload, default_icon: &str) -> Self {
        let cached_cover_url =
            Self::process_cover_url(metadata.original_cover_url.as_deref(), default_icon);

        Self {
            metadata,
            status: PlaybackStatus::Paused,
            current_time: Duration::ZERO,
            cached_cover_url,
        }
    }

    fn update_metadata(&mut self, metadata: MetadataPayload, default_icon: &str) {
        self.cached_cover_url =
            Self::process_cover_url(metadata.original_cover_url.as_deref(), default_icon);
        self.metadata = metadata;
        self.current_time = Duration::ZERO;
    }

    fn process_cover_url(original_url: Option<&str>, default_icon: &str) -> String {
        original_url.map_or_else(
            || default_icon.to_string(),
            |url| {
                if !url.starts_with("http") {
                    return default_icon.to_string();
                }
                url.replace("http://", "https://")
            },
        )
    }
}

#[derive(Debug)]
pub struct DiscordAdapter {
    options: Option<DiscordOptions>,
    client: Option<DiscordIpcClient>,
    data: Option<ActivityData>,
    is_enabled: bool,
    next_retry_time: Option<Instant>,
    // 上次发送的结束时间戳
    // 用于防抖，也用于判断是否要清除 Activity
    last_sent_end_timestamp: Option<i64>,
    show_when_paused: bool,
    display_mode: DiscordDisplayMode,
    dirty: bool,
}

#[allow(clippy::unused_async)]
impl DiscordAdapter {
    pub const fn new(options: Option<DiscordOptions>) -> Self {
        Self {
            options,
            client: None,
            data: None,
            is_enabled: false,
            next_retry_time: None,
            last_sent_end_timestamp: None,
            show_when_paused: false,
            display_mode: DiscordDisplayMode::Name,
            dirty: true,
        }
    }

    pub async fn enable(&mut self) -> Result<()> {
        info!("启用 Discord RPC");
        self.is_enabled = true;
        self.next_retry_time = None;
        self.sync_discord();
        Ok(())
    }

    pub async fn disable(&mut self) -> Result<()> {
        info!("禁用 Discord RPC");
        self.is_enabled = false;
        self.disconnect();
        Ok(())
    }

    /// 处理断线重连等后台任务
    pub async fn tick(&mut self) {
        if self.is_enabled && self.client.is_none() {
            self.sync_discord();
        }
    }

    pub async fn update_config(&mut self, payload: DiscordConfigPayload) -> Result<()> {
        info!(show_when_paused = ?payload.show_when_paused, display_mode = ?payload.display_mode, "更新 Discord 配置");
        self.show_when_paused = payload.show_when_paused;

        if let Some(mode) = payload.display_mode {
            self.display_mode = mode;
        }

        self.last_sent_end_timestamp = None;
        self.dirty = true;
        self.sync_discord();
        Ok(())
    }

    pub async fn update_metadata(&mut self, payload: MetadataPayload) -> Result<()> {
        let default_icon = self
            .options
            .as_ref()
            .map_or("", |o| o.default_icon_asset_key.as_str());
        match &mut self.data {
            Some(d) => d.update_metadata(payload, default_icon),
            None => self.data = Some(ActivityData::from_metadata(payload, default_icon)),
        }
        self.last_sent_end_timestamp = None;
        self.dirty = true;
        self.sync_discord();
        Ok(())
    }

    pub async fn update_play_state(&mut self, payload: PlayStatePayload) -> Result<()> {
        if let Some(data) = &mut self.data {
            if payload.status == PlaybackStatus::Playing && data.status != PlaybackStatus::Playing {
                self.last_sent_end_timestamp = None;
            }
            data.status = payload.status;
            self.dirty = true;
            self.sync_discord();
        }
        Ok(())
    }

    pub async fn update_timeline(&mut self, payload: TimelinePayload) -> Result<()> {
        if let Some(data) = &mut self.data {
            data.current_time = payload.current_time;
            self.sync_discord();
        }
        Ok(())
    }

    pub fn shutdown(&mut self) {
        self.disconnect();
    }

    fn disconnect(&mut self) {
        if let Some(mut client) = self.client.take() {
            let _ = client.close();
        }
        self.last_sent_end_timestamp = None;
    }

    fn connect(&mut self) -> bool {
        let Some(opts) = &self.options else {
            return false;
        };

        if let Some(retry_time) = self.next_retry_time {
            if Instant::now() < retry_time {
                return false;
            }
        }

        let mut client = DiscordIpcClient::new(&opts.app_id);
        match client.connect() {
            Ok(()) => {
                info!("Discord IPC 已连接");
                self.client = Some(client);
                self.next_retry_time = None;
                self.last_sent_end_timestamp = None;
                true
            }
            Err(e) => {
                debug!("连接 Discord IPC 失败: {e:?}. Discord 可能未运行");
                self.next_retry_time =
                    Some(Instant::now() + Duration::from_secs(RECONNECT_COOLDOWN_SECONDS));
                false
            }
        }
    }

    fn sync_discord(&mut self) {
        if !self.is_enabled || self.options.is_none() {
            if self.client.is_some() {
                self.disconnect();
            }
            return;
        }

        if self.data.is_none() {
            if let Some(client) = &mut self.client {
                let _ = client.clear_activity();
                self.last_sent_end_timestamp = None;
            }
            return;
        }

        if self.client.is_none() && !self.connect() {
            return;
        }

        if !self.perform_update() {
            self.disconnect();
            self.next_retry_time =
                Some(Instant::now() + Duration::from_secs(RECONNECT_COOLDOWN_SECONDS));
        }
    }

    fn perform_update(&mut self) -> bool {
        let Some(client) = &mut self.client else {
            return false;
        };
        let Some(data) = &self.data else {
            return true;
        };
        let Some(options) = &self.options else {
            return false;
        };

        if data.status == PlaybackStatus::Paused && !self.show_when_paused {
            debug!("播放暂停且配置为隐藏，清除 Activity");
            if let Err(e) = client.clear_activity() {
                warn!("清除 Discord Activity 失败: {e:?}");
                return false;
            }
            self.last_sent_end_timestamp = None;
            self.dirty = false;
            return true;
        }

        let mut activity = Self::build_base_activity(data, self.display_mode, options);
        let mut next_end = None;
        let mut should_send = self.dirty;

        match data.status {
            PlaybackStatus::Paused => {
                if let Some(duration) = data.metadata.duration.filter(|d| !d.is_zero()) {
                    let (start, end) = Self::calc_paused_timestamps(data.current_time, duration);
                    activity = activity
                        .timestamps(Timestamps::new().start(start).end(end))
                        .assets(
                            Assets::new()
                                .large_image(&data.cached_cover_url)
                                .large_text(&data.metadata.album_name)
                                .small_image(&options.default_icon_asset_key)
                                .small_text("Paused"),
                        );
                }
            }
            PlaybackStatus::Playing => {
                let duration = data.metadata.duration.unwrap_or(Duration::ZERO);

                if !duration.is_zero() {
                    let (start, end) = Self::calc_playing_timestamps(data.current_time, duration);
                    next_end = Some(end);

                    // 频繁调用 Discord RPC 接口会导致限流，所以在跳转发生时再更新时间戳
                    if let Some(last_end) = self.last_sent_end_timestamp {
                        if (last_end - end).abs() >= TIMESTAMP_UPDATE_THRESHOLD_MS {
                            should_send = true;
                        }
                    } else {
                        should_send = true;
                    }

                    if should_send {
                        activity = activity.timestamps(Timestamps::new().start(start).end(end));
                    }
                } else if self.last_sent_end_timestamp.is_some() {
                    should_send = true;
                }
            }
        }

        if should_send {
            debug!(
                song = %data.metadata.song_name,
                state = ?data.status,
                "更新 Discord Activity"
            );

            if let Err(e) = client.set_activity(activity) {
                warn!("设置 Discord Activity 失败: {e:?}, 尝试重连");
                return false;
            }
            self.dirty = false;
            self.last_sent_end_timestamp = next_end;
        }

        true
    }

    fn build_base_activity<'a>(
        data: &'a ActivityData,
        display_mode: DiscordDisplayMode,
        options: &'a DiscordOptions,
    ) -> Activity<'a> {
        let assets = Assets::new()
            .large_image(&data.cached_cover_url)
            .large_text(&data.metadata.album_name)
            .small_image(&options.default_icon_asset_key)
            .small_text(&options.small_icon_hover_text);

        // 不打开详细信息面板时，在用户名下方显示的小字
        let status_type = match display_mode {
            DiscordDisplayMode::Name => StatusDisplayType::Name,
            DiscordDisplayMode::State => StatusDisplayType::State,
            DiscordDisplayMode::Details => StatusDisplayType::Details,
        };

        let mut activity = Activity::new()
            .details(&data.metadata.song_name)
            .state(&data.metadata.author_name)
            .activity_type(ActivityType::Listening)
            .assets(assets)
            .status_display_type(status_type);

        if let Some(buttons) = &data.metadata.discord_buttons {
            let drp_buttons: Vec<Button<'a>> = buttons
                .iter()
                .take(2)
                .map(|b| Button::new(b.label.as_str(), b.url.as_str()))
                .collect();

            if !drp_buttons.is_empty() {
                activity = activity.buttons(drp_buttons);
            }
        }

        activity
    }

    fn calc_paused_timestamps(current_time: Duration, duration: Duration) -> (i64, i64) {
        // 来自 https://musicpresence.app/ 的 hack，通过将
        // 开始和结束时间戳向后平移一年以实现在暂停时进度静止的效果
        const ONE_YEAR_MS: i64 = 365 * 24 * 60 * 60 * 1000;

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let current_progress_ms = current_time.as_millis() as i64;
        let future_start = (now_ms - current_progress_ms) + ONE_YEAR_MS;
        let future_end = future_start + duration.as_millis() as i64;

        (future_start, future_end)
    }

    fn calc_playing_timestamps(current_time: Duration, duration: Duration) -> (i64, i64) {
        if current_time >= duration {
            return (0, 0);
        }

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let duration_ms = duration.as_millis() as i64;
        let current_time_ms = current_time.as_millis() as i64;
        let remaining_ms = (duration_ms - current_time_ms).max(0);

        let end = now_ms + remaining_ms;
        let start = end - duration_ms;

        (start, end)
    }
}
