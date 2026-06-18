use std::time::Duration;

use anyhow::Result;
use tracing::{debug, info, trace};
use windows::{
    core::{factory, Ref, HSTRING},
    Foundation::{TimeSpan, TypedEventHandler},
    Media::{
        MediaPlaybackAutoRepeatMode, MediaPlaybackStatus, MediaPlaybackType,
        PlaybackPositionChangeRequestedEventArgs, PlaybackRateChangeRequestedEventArgs,
        SystemMediaTransportControls, SystemMediaTransportControlsButton,
        SystemMediaTransportControlsButtonPressedEventArgs,
        SystemMediaTransportControlsTimelineProperties,
    },
    Storage::Streams::{DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference},
    Win32::{Foundation::HWND, System::WinRT::ISystemMediaTransportControlsInterop},
};

use crate::{
    model::{
        MetadataPayload, PlayModePayload, PlayStatePayload, PlaybackStatus, RepeatMode,
        SystemMediaEvent, SystemMediaEventType, TimelinePayload,
    },
    EventCallback, NowPlayingOptions,
};

pub struct WindowsImpl {
    smtc: SystemMediaTransportControls,
    button_pressed_token: i64,
    shuffle_changed_token: i64,
    repeat_changed_token: i64,
    seek_requested_token: i64,
    playback_rate_changed_token: i64,
    is_enabled: bool,
}

#[allow(clippy::unused_async)]
impl WindowsImpl {
    pub async fn new(options: &NowPlayingOptions, callback: EventCallback) -> Result<Self> {
        info!("正在初始化 SMTC...");

        let hwnd = options
            .hwnd
            .ok_or_else(|| anyhow::anyhow!("Windows 环境下必须提供有效的 HWND"))?;

        let interop =
            factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;

        let smtc: SystemMediaTransportControls = unsafe { interop.GetForWindow(HWND(hwnd as _)) }?;

        smtc.SetIsEnabled(false)?;
        smtc.SetIsPlayEnabled(true)?;
        smtc.SetIsPauseEnabled(true)?;
        smtc.SetIsStopEnabled(true)?;
        smtc.SetIsNextEnabled(true)?;
        smtc.SetIsPreviousEnabled(true)?;

        let cb_clone = callback.clone();
        let handler = TypedEventHandler::new(
            move |_, args: Ref<SystemMediaTransportControlsButtonPressedEventArgs>| {
                if let Some(args) = args.as_ref() {
                    let button = args.Button()?;
                    debug!(?button, "SMTC 按钮被按下");
                    let event = match button {
                        SystemMediaTransportControlsButton::Play => {
                            Some(SystemMediaEvent::new(SystemMediaEventType::Play))
                        }
                        SystemMediaTransportControlsButton::Pause => {
                            Some(SystemMediaEvent::new(SystemMediaEventType::Pause))
                        }
                        SystemMediaTransportControlsButton::Stop => {
                            Some(SystemMediaEvent::new(SystemMediaEventType::Stop))
                        }
                        SystemMediaTransportControlsButton::Next => {
                            Some(SystemMediaEvent::new(SystemMediaEventType::NextSong))
                        }
                        SystemMediaTransportControlsButton::Previous => {
                            Some(SystemMediaEvent::new(SystemMediaEventType::PreviousSong))
                        }
                        _ => None,
                    };
                    if let Some(e) = event {
                        cb_clone(e);
                    }
                }
                Ok(())
            },
        );
        let button_pressed_token = smtc.ButtonPressed(&handler)?;

        let cb_clone = callback.clone();
        let shuffle_handler = TypedEventHandler::new(move |_, _| {
            debug!("SMTC 请求切换随机播放模式");
            cb_clone(SystemMediaEvent::new(SystemMediaEventType::ToggleShuffle));
            Ok(())
        });
        let shuffle_changed_token = smtc.ShuffleEnabledChangeRequested(&shuffle_handler)?;

        let cb_clone = callback.clone();
        let repeat_handler = TypedEventHandler::new(move |_, _| {
            debug!("SMTC 请求切换重复播放模式");
            cb_clone(SystemMediaEvent::new(SystemMediaEventType::ToggleRepeat));
            Ok(())
        });
        let repeat_changed_token = smtc.AutoRepeatModeChangeRequested(&repeat_handler)?;

        let cb_clone = callback.clone();
        let seek_handler = TypedEventHandler::new(
            move |_, args: Ref<PlaybackPositionChangeRequestedEventArgs>| {
                if let Some(args) = args.as_ref() {
                    let hns = args.RequestedPlaybackPosition()?.Duration;
                    let position = Duration::from_hns(hns.cast_unsigned());
                    debug!(?position, "SMTC 请求跳转播放位置");
                    cb_clone(SystemMediaEvent::seek(position));
                }
                Ok(())
            },
        );
        let seek_requested_token = smtc.PlaybackPositionChangeRequested(&seek_handler)?;

        let cb_clone = callback;
        let playback_rate_handler =
            TypedEventHandler::new(move |_, args: Ref<PlaybackRateChangeRequestedEventArgs>| {
                if let Some(args) = args.as_ref() {
                    let rate = args.RequestedPlaybackRate()?;
                    debug!(rate, "SMTC 请求更改播放速率");
                    cb_clone(SystemMediaEvent::set_rate(rate));
                }
                Ok(())
            });
        let playback_rate_changed_token =
            smtc.PlaybackRateChangeRequested(&playback_rate_handler)?;

        debug!("SMTC 已初始化，事件处理器绑定完毕");

        Ok(Self {
            smtc,
            button_pressed_token,
            shuffle_changed_token,
            repeat_changed_token,
            seek_requested_token,
            playback_rate_changed_token,
            is_enabled: false,
        })
    }

    pub async fn enable(&mut self) -> Result<()> {
        self.is_enabled = true;
        self.smtc.SetIsEnabled(true)?;
        Ok(())
    }

    pub async fn disable(&mut self) -> Result<()> {
        self.is_enabled = false;
        self.smtc.SetIsEnabled(false)?;
        Ok(())
    }

    pub async fn update_metadata(&self, payload: MetadataPayload) -> Result<()> {
        if !self.is_enabled {
            return Ok(());
        }

        let thumbnail_stream_ref = get_cover_stream_ref(payload.cover_data).await?;

        debug!(
            title = %payload.song_name,
            artist = %payload.author_name,
            album = %payload.album_name,
            genre = ?payload.genre,
            "正在更新 SMTC 歌曲元数据"
        );

        let updater = self.smtc.DisplayUpdater()?;
        updater.SetType(MediaPlaybackType::Music)?;

        let props = updater.MusicProperties()?;
        props.SetTitle(&HSTRING::from(&payload.song_name))?;
        props.SetArtist(&HSTRING::from(&payload.author_name))?;
        props.SetAlbumTitle(&HSTRING::from(&payload.album_name))?;

        let genres = props.Genres()?;
        genres.Clear()?;
        for g in &payload.genre {
            genres.Append(&HSTRING::from(g))?;
        }

        if let Some(stream_ref) = thumbnail_stream_ref {
            updater.SetThumbnail(Some(&stream_ref))?;
        } else {
            updater.SetThumbnail(None)?;
        }

        updater.Update()?;
        Ok(())
    }

    pub async fn update_play_state(&self, payload: PlayStatePayload) -> Result<()> {
        if !self.is_enabled {
            return Ok(());
        }
        let win_status = match payload.status {
            PlaybackStatus::Playing => MediaPlaybackStatus::Playing,
            PlaybackStatus::Paused => MediaPlaybackStatus::Paused,
        };
        debug!(new_status = ?payload.status, "正在更新 SMTC 播放状态");
        self.smtc.SetPlaybackStatus(win_status)?;
        Ok(())
    }

    pub async fn update_playback_rate(&self, rate: f64) -> Result<()> {
        if !self.is_enabled {
            return Ok(());
        }
        self.smtc.SetPlaybackRate(rate)?;
        Ok(())
    }

    pub async fn update_volume(&self, _volume: f64) -> Result<()> {
        // 未实现
        Ok(())
    }

    pub async fn update_timeline(&self, payload: TimelinePayload) -> Result<()> {
        if !self.is_enabled {
            return Ok(());
        }

        trace!(
            current_time = ?payload.current_time,
            total_time = ?payload.total_time, "正在更新 SMTC 时间线"
        );

        let props = SystemMediaTransportControlsTimelineProperties::new()?;
        props.SetStartTime(TimeSpan { Duration: 0 })?;
        props.SetPosition(TimeSpan {
            Duration: (payload.current_time.as_hns()) as i64,
        })?;
        props.SetEndTime(TimeSpan {
            Duration: (payload.total_time.as_hns()) as i64,
        })?;
        self.smtc.UpdateTimelineProperties(&props)?;
        Ok(())
    }

    pub async fn update_play_mode(&self, payload: PlayModePayload) -> Result<()> {
        if !self.is_enabled {
            return Ok(());
        }

        debug!(payload.is_shuffling, ?payload.repeat_mode, "正在更新 SMTC 播放模式");
        self.smtc.SetShuffleEnabled(payload.is_shuffling)?;

        let repeat_mode_win = match payload.repeat_mode {
            RepeatMode::Track => MediaPlaybackAutoRepeatMode::Track,
            RepeatMode::List => MediaPlaybackAutoRepeatMode::List,
            RepeatMode::None => MediaPlaybackAutoRepeatMode::None,
        };
        self.smtc.SetAutoRepeatMode(repeat_mode_win)?;
        Ok(())
    }

    pub fn shutdown(&mut self) {
        self.is_enabled = false;
        let _ = self.smtc.SetIsEnabled(false);
        let _ = self.smtc.RemoveButtonPressed(self.button_pressed_token);
        let _ = self
            .smtc
            .RemoveShuffleEnabledChangeRequested(self.shuffle_changed_token);
        let _ = self
            .smtc
            .RemoveAutoRepeatModeChangeRequested(self.repeat_changed_token);
        let _ = self
            .smtc
            .RemovePlaybackPositionChangeRequested(self.seek_requested_token);
        let _ = self
            .smtc
            .RemovePlaybackRateChangeRequested(self.playback_rate_changed_token);
    }
}

impl Drop for WindowsImpl {
    fn drop(&mut self) {
        self.shutdown();
    }
}

async fn get_cover_stream_ref(
    cover_data: Option<Vec<u8>>,
) -> Result<Option<RandomAccessStreamReference>> {
    let Some(bytes) = cover_data else {
        return Ok(None);
    };

    let stream = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&stream)?;
    writer.WriteBytes(&bytes)?;
    writer.StoreAsync()?.await?;
    writer.DetachStream()?;
    stream.Seek(0)?;

    Ok(Some(RandomAccessStreamReference::CreateFromStream(
        &stream,
    )?))
}

const HNS_PER_SEC: u64 = 10_000_000;
const NANOS_PER_HNS: u32 = 100;

pub trait WindowsTimeExt {
    fn from_hns(hns: u64) -> Self;

    fn as_hns(&self) -> u128;
}

impl WindowsTimeExt for Duration {
    fn from_hns(hns: u64) -> Self {
        let secs = hns / HNS_PER_SEC;
        let nanos = (hns % HNS_PER_SEC) as u32 * NANOS_PER_HNS;

        Self::new(secs, nanos)
    }

    fn as_hns(&self) -> u128 {
        self.as_nanos() / u128::from(NANOS_PER_HNS)
    }
}
