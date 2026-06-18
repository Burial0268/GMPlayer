use std::{ptr::NonNull, time::Duration};

use anyhow::Result;
use block2::RcBlock;
use objc2::{
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    AnyThread as _, Message as _,
};
use objc2_app_kit::NSImage;
use objc2_foundation::{NSArray, NSData, NSMutableDictionary, NSNumber, NSSize, NSString};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPChangePlaybackRateCommandEvent,
    MPChangeRepeatModeCommandEvent, MPChangeShuffleModeCommandEvent, MPMediaItemArtwork,
    MPMediaItemPropertyAlbumTitle, MPMediaItemPropertyArtist, MPMediaItemPropertyArtwork,
    MPMediaItemPropertyGenre, MPMediaItemPropertyPersistentID, MPMediaItemPropertyPlaybackDuration,
    MPMediaItemPropertyTitle, MPNowPlayingInfoCenter, MPNowPlayingInfoPropertyElapsedPlaybackTime,
    MPNowPlayingInfoPropertyPlaybackRate, MPNowPlayingPlaybackState, MPRemoteCommand,
    MPRemoteCommandCenter, MPRemoteCommandEvent, MPRemoteCommandHandlerStatus, MPRepeatType,
    MPShuffleType,
};
use tracing::{debug, trace};

use crate::{
    model::{
        MetadataPayload, PlayModePayload, PlayStatePayload, PlaybackStatus, SystemMediaEvent,
        SystemMediaEventType, TimelinePayload,
    },
    EventCallback, NowPlayingOptions,
};

pub struct MacosImpl {
    np_info_ctr: Retained<MPNowPlayingInfoCenter>,
    cmd_ctr: Retained<MPRemoteCommandCenter>,
    info: Retained<NSMutableDictionary<NSString, AnyObject>>,
    target_tokens: Vec<(Retained<MPRemoteCommand>, Retained<AnyObject>)>,
}

#[allow(clippy::unused_async, clippy::future_not_send)]
impl MacosImpl {
    pub async fn new(_options: &NowPlayingOptions, callback: EventCallback) -> Result<Self> {
        let np_info_ctr = unsafe { MPNowPlayingInfoCenter::defaultCenter() };
        let cmd_ctr = unsafe { MPRemoteCommandCenter::sharedCommandCenter() };
        let info = NSMutableDictionary::new();

        let mut instance = Self {
            np_info_ctr,
            cmd_ctr,
            info,
            target_tokens: Vec::new(),
        };

        instance.setup_event_listeners(&callback);

        Ok(instance)
    }

    fn store_token(&mut self, command: &MPRemoteCommand, token: Retained<AnyObject>) {
        self.target_tokens.push((command.retain(), token));
    }

    fn setup_event_listeners(&mut self, callback: &EventCallback) {
        unsafe {
            // 播放
            self.add_simple_handler(
                &self.cmd_ctr.playCommand(),
                SystemMediaEventType::Play,
                callback,
            );
            // 暂停
            self.add_simple_handler(
                &self.cmd_ctr.pauseCommand(),
                SystemMediaEventType::Pause,
                callback,
            );
            // 上一首
            self.add_simple_handler(
                &self.cmd_ctr.previousTrackCommand(),
                SystemMediaEventType::PreviousSong,
                callback,
            );
            // 下一首
            self.add_simple_handler(
                &self.cmd_ctr.nextTrackCommand(),
                SystemMediaEventType::NextSong,
                callback,
            );
            // 停止
            self.add_simple_handler(
                &self.cmd_ctr.stopCommand(),
                SystemMediaEventType::Stop,
                callback,
            );
        }

        self.add_toggle_handler(callback);
        self.add_seek_handler(callback);
        self.add_rate_handler(callback);
        self.add_shuffle_handler(callback);
        self.add_repeat_handler(callback);
    }

    fn add_simple_handler(
        &mut self,
        command: &MPRemoteCommand,
        event_type: SystemMediaEventType,
        callback: &EventCallback,
    ) {
        let cb = callback.clone();
        let block = RcBlock::new(
            move |_: NonNull<MPRemoteCommandEvent>| -> MPRemoteCommandHandlerStatus {
                debug!(?event_type, "MPRemoteCommand 触发");
                cb(SystemMediaEvent::new(event_type));
                MPRemoteCommandHandlerStatus::Success
            },
        );

        unsafe {
            command.setEnabled(true);
            let token = command.addTargetWithHandler(&block);
            self.store_token(command, token);
        }
    }

    fn add_toggle_handler(&mut self, callback: &EventCallback) {
        let command = unsafe { self.cmd_ctr.togglePlayPauseCommand() };
        let cb = callback.clone();

        let info_ctr = self.np_info_ctr.clone();

        let block = RcBlock::new(move |_| -> MPRemoteCommandHandlerStatus {
            let current_state = unsafe { info_ctr.playbackState() };

            let event_type = if current_state == MPNowPlayingPlaybackState::Playing {
                SystemMediaEventType::Pause
            } else {
                SystemMediaEventType::Play
            };

            debug!(?event_type, "MPRemoteCommand Toggle 触发");
            cb(SystemMediaEvent::new(event_type));
            MPRemoteCommandHandlerStatus::Success
        });

        unsafe {
            command.setEnabled(true);
            let token = command.addTargetWithHandler(&block);
            self.store_token(&command, token);
        }
    }

    fn add_seek_handler(&mut self, callback: &EventCallback) {
        let command = unsafe { self.cmd_ctr.changePlaybackPositionCommand() };
        let cb = callback.clone();

        let block = RcBlock::new(
            move |event: NonNull<MPRemoteCommandEvent>| -> MPRemoteCommandHandlerStatus {
                let seek_evt_opt = unsafe { Retained::retain(event.as_ptr()) }
                    .and_then(|evt| evt.downcast::<MPChangePlaybackPositionCommandEvent>().ok());

                if let Some(seek_evt) = seek_evt_opt {
                    let position_secs = unsafe { seek_evt.positionTime() };
                    let position = Duration::from_secs_f64(position_secs);
                    debug!(?position, "MPChangePlaybackPositionCommand 触发");
                    cb(SystemMediaEvent::seek(position));
                }
                MPRemoteCommandHandlerStatus::Success
            },
        );

        unsafe {
            command.setEnabled(true);
            let token = command.addTargetWithHandler(&block);
            self.store_token(&command, token);
        }
    }

    fn add_rate_handler(&mut self, callback: &EventCallback) {
        let command = unsafe { self.cmd_ctr.changePlaybackRateCommand() };
        let cb = callback.clone();

        let block = RcBlock::new(
            move |event: NonNull<MPRemoteCommandEvent>| -> MPRemoteCommandHandlerStatus {
                let rate_evt_opt = unsafe { Retained::retain(event.as_ptr()) }
                    .and_then(|evt| evt.downcast::<MPChangePlaybackRateCommandEvent>().ok());

                if let Some(rate_evt) = rate_evt_opt {
                    let rate = unsafe { rate_evt.playbackRate() };
                    debug!(rate, "MPChangePlaybackRateCommand 触发");
                    cb(SystemMediaEvent::set_rate(f64::from(rate)));
                }
                MPRemoteCommandHandlerStatus::Success
            },
        );

        unsafe {
            command.setEnabled(true);
            let rates = NSArray::from_retained_slice(&[
                NSNumber::new_f64(0.25),
                NSNumber::new_f64(0.5),
                NSNumber::new_f64(0.75),
                NSNumber::new_f64(1.0),
                NSNumber::new_f64(1.25),
                NSNumber::new_f64(1.5),
                NSNumber::new_f64(1.75),
                NSNumber::new_f64(2.0),
            ]);
            command.setSupportedPlaybackRates(&rates);

            let token = command.addTargetWithHandler(&block);
            self.store_token(&command, token);
        }
    }

    fn add_shuffle_handler(&mut self, callback: &EventCallback) {
        let command = unsafe { self.cmd_ctr.changeShuffleModeCommand() };
        let cb = callback.clone();

        let block = RcBlock::new(
            move |event: NonNull<MPRemoteCommandEvent>| -> MPRemoteCommandHandlerStatus {
                if unsafe { Retained::retain(event.as_ptr()) }
                    .and_then(|e| e.downcast::<MPChangeShuffleModeCommandEvent>().ok())
                    .is_some()
                {
                    debug!("MPChangeShuffleModeCommand 触发");
                    cb(SystemMediaEvent::new(SystemMediaEventType::ToggleShuffle));
                }
                MPRemoteCommandHandlerStatus::Success
            },
        );

        unsafe {
            command.setEnabled(true);
            let token = command.addTargetWithHandler(&block);
            self.store_token(&command, token);
        }
    }

    fn add_repeat_handler(&mut self, callback: &EventCallback) {
        let command = unsafe { self.cmd_ctr.changeRepeatModeCommand() };
        let cb = callback.clone();

        let block = RcBlock::new(
            move |event: NonNull<MPRemoteCommandEvent>| -> MPRemoteCommandHandlerStatus {
                if unsafe { Retained::retain(event.as_ptr()) }
                    .and_then(|e| e.downcast::<MPChangeRepeatModeCommandEvent>().ok())
                    .is_some()
                {
                    debug!("MPChangeRepeatModeCommand 触发");
                    cb(SystemMediaEvent::new(SystemMediaEventType::ToggleRepeat));
                }
                MPRemoteCommandHandlerStatus::Success
            },
        );

        unsafe {
            command.setEnabled(true);
            let token = command.addTargetWithHandler(&block);
            self.store_token(&command, token);
        }
    }

    fn set_commands_enabled(&mut self, enabled: bool) {
        unsafe {
            self.cmd_ctr.playCommand().setEnabled(enabled);
            self.cmd_ctr.pauseCommand().setEnabled(enabled);
            self.cmd_ctr.togglePlayPauseCommand().setEnabled(enabled);
            self.cmd_ctr.nextTrackCommand().setEnabled(enabled);
            self.cmd_ctr.previousTrackCommand().setEnabled(enabled);
            self.cmd_ctr.stopCommand().setEnabled(enabled);
            self.cmd_ctr
                .changePlaybackPositionCommand()
                .setEnabled(enabled);
            self.cmd_ctr.changePlaybackRateCommand().setEnabled(enabled);
            self.cmd_ctr.changeShuffleModeCommand().setEnabled(enabled);
            self.cmd_ctr.changeRepeatModeCommand().setEnabled(enabled);
        }
    }

    pub async fn enable(&mut self) -> Result<()> {
        self.set_commands_enabled(true);
        Ok(())
    }

    pub async fn disable(&mut self) -> Result<()> {
        self.set_commands_enabled(false);
        Ok(())
    }

    pub async fn update_metadata(&mut self, payload: MetadataPayload) -> Result<()> {
        debug!(
            title = %payload.song_name,
            artist = %payload.author_name,
            album = %payload.album_name,
            track_id = ?payload.track_id,
            "正在更新 MPNowPlayingInfoCenter 元数据"
        );

        unsafe {
            // 基础文本信息
            let info = &self.info;

            info.setObject_forKey(
                &NSString::from_str(&payload.song_name),
                ProtocolObject::from_ref(MPMediaItemPropertyTitle),
            );
            info.setObject_forKey(
                &NSString::from_str(&payload.author_name),
                ProtocolObject::from_ref(MPMediaItemPropertyArtist),
            );
            info.setObject_forKey(
                &NSString::from_str(&payload.album_name),
                ProtocolObject::from_ref(MPMediaItemPropertyAlbumTitle),
            );

            // 流派
            if payload.genre.is_empty() {
                info.removeObjectForKey(MPMediaItemPropertyGenre);
            } else {
                let genre_str = payload.genre.join(", ");
                info.setObject_forKey(
                    &NSString::from_str(&genre_str),
                    ProtocolObject::from_ref(MPMediaItemPropertyGenre),
                );
            }

            // 重置已播放时间
            info.setObject_forKey(
                &NSNumber::new_f64(0.0),
                ProtocolObject::from_ref(MPNowPlayingInfoPropertyElapsedPlaybackTime),
            );

            // 设置唯一 PersistentID
            let persistent_id = payload.track_id.unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64
            });

            info.setObject_forKey(
                &NSNumber::new_i64(persistent_id),
                ProtocolObject::from_ref(MPMediaItemPropertyPersistentID),
            );

            // 时长
            if let Some(dur) = payload.duration {
                let duration_secs = dur.as_secs_f64();
                info.setObject_forKey(
                    &NSNumber::new_f64(duration_secs),
                    ProtocolObject::from_ref(MPMediaItemPropertyPlaybackDuration),
                );
            } else {
                info.removeObjectForKey(MPMediaItemPropertyPlaybackDuration);
            }

            // 封面
            if let Some(data) = payload.cover_data {
                let ns_data = NSData::from_vec(data);
                let img = NSImage::alloc();

                if let Some(img) = NSImage::initWithData(img, &ns_data) {
                    let img_size = img.size();

                    let handler = RcBlock::new(move |_: NSSize| -> NonNull<NSImage> {
                        let ptr = Retained::as_ptr(&img);
                        NonNull::new(ptr.cast_mut()).expect("NSImage 指针不应为空")
                    });

                    let artwork = MPMediaItemArtwork::alloc();
                    let artwork = MPMediaItemArtwork::initWithBoundsSize_requestHandler(
                        artwork, img_size, &handler,
                    );

                    info.setObject_forKey(
                        &artwork,
                        ProtocolObject::from_ref(MPMediaItemPropertyArtwork),
                    );
                }
            } else {
                info.removeObjectForKey(MPMediaItemPropertyArtwork);
            }

            self.np_info_ctr.setNowPlayingInfo(Some(info));
        }
        Ok(())
    }

    pub async fn update_play_state(&mut self, payload: PlayStatePayload) -> Result<()> {
        debug!(new_status = ?payload.status, "正在更新 MPNowPlayingInfoCenter 播放状态");
        let macos_state = match payload.status {
            PlaybackStatus::Playing => MPNowPlayingPlaybackState::Playing,
            PlaybackStatus::Paused => MPNowPlayingPlaybackState::Paused,
        };

        unsafe {
            self.np_info_ctr.setPlaybackState(macos_state);
        }
        Ok(())
    }

    pub async fn update_playback_rate(&mut self, rate: f64) -> Result<()> {
        trace!(new_rate = rate, "正在更新 MPNowPlayingInfoCenter 播放速率");
        unsafe {
            self.info.setObject_forKey(
                &NSNumber::new_f64(rate),
                ProtocolObject::from_ref(MPNowPlayingInfoPropertyPlaybackRate),
            );
            self.np_info_ctr.setNowPlayingInfo(Some(&self.info));
        }
        Ok(())
    }

    pub async fn update_volume(&self, _volume: f64) -> Result<()> {
        Ok(())
    }

    pub async fn update_timeline(&mut self, payload: TimelinePayload) -> Result<()> {
        trace!(
            new_curr = ?payload.current_time,
            new_total = ?payload.total_time,
            "正在更新 MPNowPlayingInfoCenter 时间线"
        );

        unsafe {
            // 播放进度
            self.info.setObject_forKey(
                &NSNumber::new_f64(payload.current_time.as_secs_f64()),
                ProtocolObject::from_ref(MPNowPlayingInfoPropertyElapsedPlaybackTime),
            );

            // 总时长
            self.info.setObject_forKey(
                &NSNumber::new_f64(payload.total_time.as_secs_f64()),
                ProtocolObject::from_ref(MPMediaItemPropertyPlaybackDuration),
            );

            self.np_info_ctr.setNowPlayingInfo(Some(&self.info));
        }
        Ok(())
    }

    pub async fn update_play_mode(&mut self, payload: PlayModePayload) -> Result<()> {
        debug!(
            is_shuffling = payload.is_shuffling,
            repeat_mode = ?payload.repeat_mode,
            "正在更新 MPNowPlayingInfoCenter 播放模式"
        );

        unsafe {
            let shuffle_type = if payload.is_shuffling {
                MPShuffleType::Items
            } else {
                MPShuffleType::Off
            };
            self.cmd_ctr
                .changeShuffleModeCommand()
                .setCurrentShuffleType(shuffle_type);

            let repeat_type = match payload.repeat_mode {
                crate::model::RepeatMode::None => MPRepeatType::Off,
                crate::model::RepeatMode::Track => MPRepeatType::One,
                crate::model::RepeatMode::List => MPRepeatType::All,
            };
            self.cmd_ctr
                .changeRepeatModeCommand()
                .setCurrentRepeatType(repeat_type);
        }
        Ok(())
    }

    pub fn shutdown(&mut self) {
        self.set_commands_enabled(false);
        for (command, token) in self.target_tokens.drain(..) {
            unsafe {
                command.removeTarget(Some(&token));
            }
        }
        unsafe {
            self.np_info_ctr.setNowPlayingInfo(None);
        }
    }
}

impl Drop for MacosImpl {
    fn drop(&mut self) {
        self.shutdown();
    }
}
