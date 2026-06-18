use std::{fmt, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemMediaEventType {
    Play,
    Pause,
    Stop,
    NextSong,
    PreviousSong,
    ToggleShuffle,
    ToggleRepeat,
    SetRate,
    SetVolume,
    /// 绝对位置，毫秒
    Seek,
}

#[derive(Clone, Debug)]
pub struct SystemMediaEvent {
    pub type_: SystemMediaEventType,
    pub position: Option<Duration>,
    pub rate: Option<f64>,
    pub volume: Option<f64>,
}

impl SystemMediaEvent {
    #[must_use]
    pub const fn new(t: SystemMediaEventType) -> Self {
        Self {
            type_: t,
            position: None,
            rate: None,
            volume: None,
        }
    }
    #[must_use]
    pub const fn seek(position: Duration) -> Self {
        Self {
            type_: SystemMediaEventType::Seek,
            position: Some(position),
            rate: None,
            volume: None,
        }
    }
    #[must_use]
    pub const fn set_rate(rate: f64) -> Self {
        Self {
            type_: SystemMediaEventType::SetRate,
            position: None,
            rate: Some(rate),
            volume: None,
        }
    }
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    #[must_use]
    pub const fn set_volume(volume: f64) -> Self {
        Self {
            type_: SystemMediaEventType::SetVolume,
            position: None,
            rate: None,
            volume: Some(volume),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct MetadataPayload {
    pub song_name: String,
    pub author_name: String,
    pub album_name: String,

    /// 封面的原始字节数据，适用于除 Discord RPC 之外的其他平台
    pub cover_data: Option<Vec<u8>>,

    /// 封面的 HTTP URL，更新 Discord RPC 时必传，其他平台可不传
    ///
    /// Linux 平台在没有提供 `cover_data` 时会使用它
    pub original_cover_url: Option<String>,

    /// 流派信息
    ///
    /// 在 macOS 上会使用逗号连接多个流派
    pub genre: Vec<String>,

    /// 可选的曲目 ID，用于 macOS PersistentID 和 Linux D-Bus Track ID 的唯一标识
    pub track_id: Option<i64>,

    /// Discord RPC 按钮。最多支持 2 个，超出部分将被自动忽略
    pub discord_buttons: Option<Vec<DiscordButton>>,

    /// 当前歌曲时长
    ///
    /// 用于 Linux、MacOS、Discord RPC 的元数据更新。Windows 使用 [`TimelinePayload`] 的
    /// `total_time` 字段。
    pub duration: Option<Duration>,
}

impl fmt::Debug for MetadataPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MetadataPayload")
            .field("song_name", &self.song_name)
            .field("author_name", &self.author_name)
            .field("album_name", &self.album_name)
            .field(
                "cover_data",
                &self.cover_data.as_ref().map_or_else(
                    || "None".to_string(),
                    |bytes| format!("Some({} bytes)", bytes.len()),
                ),
            )
            .field("original_cover_url", &self.original_cover_url)
            .field("genre", &self.genre)
            .field("track_id", &self.track_id)
            .field("discord_buttons", &self.discord_buttons)
            .field("duration", &self.duration)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    Playing,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    None,
    Track,
    List,
}

#[derive(Debug, Clone, Copy)]
pub struct PlayStatePayload {
    pub status: PlaybackStatus,
}

#[derive(Debug, Clone, Copy)]
pub struct TimelinePayload {
    pub current_time: Duration,

    pub total_time: Duration,

    /// 是否为 seek 操作触发的更新
    pub seeked: Option<bool>,
}

#[derive(Debug, Clone, Copy)]
pub struct PlayModePayload {
    pub is_shuffling: bool,
    pub repeat_mode: RepeatMode,
}

/// Discord 显示模式枚举
///
/// 不打开详细信息面板时，在用户名下方显示的小字
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscordDisplayMode {
    /// Listening to SPlayer
    Name,
    /// Listening to Rick Astley
    State,
    /// Listening to Never Gonna Give You Up
    Details,
}

/// Discord 配置参数
#[derive(Debug, Clone)]
pub struct DiscordConfigPayload {
    /// 暂停时是否显示
    ///
    /// 注意暂停时进度会固定为 0
    pub show_when_paused: bool,

    /// 显示模式，参考 [`DiscordDisplayMode`]
    pub display_mode: Option<DiscordDisplayMode>,
}

/// Discord 按钮定义
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordButton {
    /// 按钮上显示的文字
    pub label: String,
    /// 点击按钮后跳转的 URL
    pub url: String,
}

/// Discord RPC 的静态初始化配置
#[derive(Debug, Clone)]
pub struct DiscordOptions {
    /// Discord 开发者平台上的 Application ID
    pub app_id: String,

    /// 默认图标的 Asset Key。
    /// 当没有提供封面时，会将其作为大图标；同时它也会作为右下角的小图标显示。
    pub default_icon_asset_key: String,

    /// 小图标的 Hover 文本，通常是你的应用名称
    pub small_icon_hover_text: String,
}

/// 统一的初始化配置项
#[derive(Debug, Clone, Default)]
pub struct NowPlayingOptions {
    /// 窗口句柄，Windows 环境下必传
    pub hwnd: Option<isize>,
    /// Discord 配置，传 None 则代表禁用/不初始化 Discord RPC
    pub discord: Option<DiscordOptions>,
    /// 应用名称，Linux 下用于 MPRIS 的 identity 和 desktop_entry
    pub app_name: Option<String>,
}
