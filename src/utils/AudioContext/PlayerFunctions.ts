/**
 * PlayerFunctions - Exported player functions with store integration
 */

import { h } from 'vue';
import { songScrobble } from '@/api/song';
import { musicStore, settingStore, siteStore, userStore } from '@/store';
import { NIcon } from 'naive-ui';
import { MusicNoteFilled } from '@vicons/material';
import getLanguageData from '@/utils/getLanguageData';
import { getCoverColor } from '@/utils/getCoverColor';
import { NativeSound } from './NativeSound';
import { SoundManager } from './SoundManager';
import type { ISound, PlaySongTime } from './types';

// 歌曲信息更新定时器
let timeupdateInterval: number | null = null;
// 听歌打卡延时器
let scrobbleTimeout: ReturnType<typeof setTimeout> | null = null;
// 重试次数
let testNumber = 0;
// 频谱更新动画帧 ID
let spectrumAnimationId: number | null = null;

/**
 * 停止频谱更新
 */
const stopSpectrumUpdate = (): void => {
  if (spectrumAnimationId) {
    cancelAnimationFrame(spectrumAnimationId);
    spectrumAnimationId = null;
  }
};

/**
 * 启动频谱更新
 * @param sound - 音频对象
 * @param music - pinia store
 */
const startSpectrumUpdate = (sound: NativeSound, music: ReturnType<typeof musicStore>): void => {
  stopSpectrumUpdate();

  const updateLoop = (): void => {
    if (!sound) {
      spectrumAnimationId = null;
      return;
    }

    // 获取频谱数据
    const frequencyData = sound.getFrequencyData();
    music.spectrumsData = [...frequencyData];

    // 计算平均振幅用于 scale
    const averageAmplitude = frequencyData.reduce((acc, val) => acc + val, 0) / frequencyData.length;
    music.spectrumsScaleData = (averageAmplitude / 255 + 1).toFixed(2);

    // 获取低频音量 (直接从 effectManager 计算，已内置平滑处理)
    music.lowFreqVolume = sound.getLowFrequencyVolume();

    spectrumAnimationId = requestAnimationFrame(updateLoop);
  };

  updateLoop();
};

/**
 * 获取播放进度
 * @param sound - 音频对象
 * @param music - pinia
 */
const checkAudioTime = (sound: ISound, music: ReturnType<typeof musicStore>): void => {
  if (sound.playing()) {
    const currentTime = sound.seek() as number;
    const duration = sound.duration();
    music.setPlaySongTime({ currentTime, duration });
  }
};

/**
 * 生成 MediaSession
 * @param music - pinia
 */
const setMediaSession = (music: ReturnType<typeof musicStore>): void => {
  if ('mediaSession' in navigator && Object.keys(music.getPlaySongData).length) {
    const artists = music.getPlaySongData.artist.map((a: { name: string }) => a.name);
    if (music.getPlaySongData.album.picUrl == undefined) {
      // getAlbum is not imported, skip this case
      // In the original code this was also an issue - getAlbum was used but not imported
    }
    navigator.mediaSession.metadata = new MediaMetadata({
      title: music.getPlaySongData.name,
      artist: artists.join(' & '),
      album: music.getPlaySongData.album.name,
      artwork: [
        {
          src: music.getPlaySongData.album.picUrl.replace(/^http:/, 'https:') + '?param=96y96',
          sizes: '96x96',
        },
        {
          src: music.getPlaySongData.album.picUrl.replace(/^http:/, 'https:') + '?param=128y128',
          sizes: '128x128',
        },
        {
          src: music.getPlaySongData.album.picUrl.replace(/^http:/, 'https:') + '?param=512x512',
          sizes: '512x512',
        },
      ],
    });
    navigator.mediaSession.setActionHandler('nexttrack', () => {
      music.setPlaySongIndex('next');
    });
    navigator.mediaSession.setActionHandler('previoustrack', () => {
      music.setPlaySongIndex('prev');
    });
    navigator.mediaSession.setActionHandler('play', () => {
      music.setPlayState(true);
    });
    navigator.mediaSession.setActionHandler('pause', () => {
      music.setPlayState(false);
    });
  }
};

/**
 * 创建音频对象
 * @param src - 音频文件地址
 * @param autoPlay - 是否自动播放（默认为 true）
 * @return 音频对象
 */
export const createSound = (src: string, autoPlay = true): ISound | undefined => {
  try {
    SoundManager.unload();
    stopSpectrumUpdate();

    const music = musicStore();
    const site = siteStore();
    const settings = settingStore();
    const user = userStore();
    const sound = new NativeSound({
      src: [src],
      preload: true,
      volume: music.persistData.playVolume,
    });
    SoundManager.setCurrentSound(sound);

    // 更新取色
    getCoverColor(music.getPlaySongData.album.picUrl)
      .then((color) => {
        site.songPicGradient = color;
      })
      .catch((err) => {
        console.error('取色出错', err);
      });

    console.log('[createSound] autoPlay:', autoPlay, 'getPlayState:', music.getPlayState);
    if (autoPlay) {
      console.log('[createSound] Calling fadePlayOrPause with play');
      fadePlayOrPause(sound, 'play', music.persistData.playVolume);
    }
    // 首次加载事件
    sound?.once('load', () => {
      const songId = music.getPlaySongData?.id;
      const sourceId = music.getPlaySongData?.sourceId ? music.getPlaySongData.sourceId : 0;
      const isLogin = user.userLogin;
      const isMemory = settings.memoryLastPlaybackPosition;
      console.log('首次缓冲完成：' + songId + ' / 来源：' + sourceId);
      if (isMemory) {
        sound?.seek(music.persistData.playSongTime.currentTime);
      } else {
        music.persistData.playSongTime = {
          currentTime: 0,
          duration: 0,
          barMoveDistance: 0,
          songTimePlayed: '00:00',
          songTimeDuration: '00:00',
        };
      }
      // 取消加载状态
      music.isLoadingSong = false;
      // 听歌打卡
      if (isLogin) {
        if (scrobbleTimeout) clearTimeout(scrobbleTimeout);
        scrobbleTimeout = setTimeout(() => {
          songScrobble(songId, sourceId)
            .then((res) => {
              console.log('歌曲打卡完成', res);
            })
            .catch((err) => {
              console.error('歌曲打卡失败：' + err);
            });
        }, 3000);
      }
    });

    // 播放事件
    sound?.on('play', () => {
      if (timeupdateInterval) {
        cancelAnimationFrame(timeupdateInterval);
      }
      const playSongData = music.getPlaySongData;
      if (!Object.keys(playSongData).length) {
        window.$message.error(getLanguageData('songLoadError'));
        return;
      }

      const songName = playSongData?.name;
      const songArtist = playSongData.artist[0]?.name;

      testNumber = 0;
      music.setPlayState(true);

      // 播放通知
      if (typeof window.$message !== 'undefined' && songArtist !== null) {
        window.$message.info(`${songName} - ${songArtist}`, {
          icon: () =>
            h(NIcon, null, {
              default: () => h(MusicNoteFilled),
            }),
        });
      } else {
        window.$message.warning(getLanguageData('songNotDetails'));
      }

      console.log(`开始播放：${songName} - ${songArtist}`);
      setMediaSession(music);

      // 预加载下一首
      music.preloadUpcomingSongs();

      // 获取播放器信息
      const timeLoop = (): void => {
        checkAudioTime(sound, music);
        timeupdateInterval = requestAnimationFrame(timeLoop);
      };
      timeLoop();

      // 写入播放历史
      music.setPlayHistory(playSongData);

      // 播放时页面标题
      window.document.title = `${songName} - ${songArtist} - ${import.meta.env.VITE_SITE_TITLE}`;

      // 启动频谱更新
      if (settings.musicFrequency || settings.dynamicFlowSpeed) {
        startSpectrumUpdate(sound, music);
      }
    });

    // 暂停事件
    sound?.on('pause', () => {
      if (timeupdateInterval) cancelAnimationFrame(timeupdateInterval);
      console.log('音乐暂停');
      music.setPlayState(false);
      // 更改页面标题
      window.$setSiteTitle();
    });
    // 结束事件
    sound?.on('end', () => {
      if (timeupdateInterval) cancelAnimationFrame(timeupdateInterval);
      stopSpectrumUpdate();
      console.log('歌曲播放结束');
      music.setPlaySongIndex('next');
    });
    // 错误事件
    sound?.on('loaderror', () => {
      if (testNumber > 2) {
        window.$message.error(getLanguageData('songPlayError'));
        console.error(getLanguageData('songPlayError'));
        music.setPlayState(false);
      }
      if (testNumber < 4) {
        if (music.getPlaylists[0]) window.$getPlaySongData(music.getPlaySongData);
        testNumber++;
      } else {
        window.$message.error(getLanguageData('songLoadTest'), {
          closable: true,
          duration: 0,
        });
      }
    });
    sound?.on('playerror', () => {
      window.$message.error(getLanguageData('songPlayError'));
      console.error(getLanguageData('songPlayError'));
      music.setPlayState(false);
    });

    // 返回音频对象
    return (window.$player = sound);
  } catch (err) {
    window.$message.error(getLanguageData('songLoadError'));
    console.error(getLanguageData('songLoadError'), err);
  }
};

/**
 * 设置音量
 * @param sound - 音频对象
 * @param volume - 设置的音量值，0-1之间的浮点数
 */
export const setVolume = (sound: ISound | undefined, volume: number): void => {
  sound?.volume(volume);
};

/**
 * 设置进度
 * @param sound - 音频对象
 * @param seek - 设置的进度值（秒）
 */
export const setSeek = (sound: ISound | undefined, seek: number): void => {
  const music = musicStore();
  sound?.seek(seek);
  // 直接调用 setPlaySongTime 确保 UI 状态立即更新
  if (sound) {
    music.setPlaySongTime({
      currentTime: seek,
      duration: sound.duration(),
    });
  }
};

/**
 * 音频渐入渐出
 * @param sound - 音频对象
 * @param type - 渐入还是渐出 ('play' | 'pause')
 * @param volume - 渐出音量的大小，0-1之间的浮点数
 * @param duration - 渐出音量的时长，单位为毫秒
 */
export const fadePlayOrPause = (
  sound: ISound | undefined,
  type: 'play' | 'pause',
  volume: number,
  duration = 300
): void => {
  console.log('[fadePlayOrPause] type:', type, 'sound:', !!sound, 'playing:', sound?.playing());
  const settingData = JSON.parse(localStorage.getItem('settingData') || '{}');
  const isFade = settingData.songVolumeFade ?? true;
  console.log('[fadePlayOrPause] isFade:', isFade);
  if (isFade) {
    if (type === 'play') {
      if (sound?.playing()) {
        console.log('[fadePlayOrPause] Already playing, returning');
        return;
      }
      console.log('[fadePlayOrPause] Calling sound.play()');
      sound?.play();
      sound?.once('play', () => {
        console.log('[fadePlayOrPause] play event received, starting fade');
        sound?.fade(0, volume, duration);
      });
    } else if (type === 'pause') {
      sound?.fade(volume, 0, duration);
      sound?.once('fade', () => {
        sound?.pause();
      });
    }
  } else {
    type === 'play' ? sound?.play() : sound?.pause();
  }
};

/**
 * 停止播放器
 * @param sound - 音频对象
 */
export const soundStop = (sound: ISound | undefined): void => {
  sound?.stop();
  setSeek(sound, 0);
};

/**
 * 生成频谱数据 - 快速傅里叶变换（ FFT ）
 * @deprecated Use NativeSound.getFrequencyData() instead
 * @param sound - NativeSound 音频对象
 */
export const processSpectrum = (sound: ISound | undefined): void => {
  // No longer needed - spectrum is handled internally by NativeSound
  console.log('processSpectrum called - now handled internally by NativeSound');
};
