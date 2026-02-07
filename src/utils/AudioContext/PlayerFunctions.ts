/**
 * PlayerFunctions - Exported player functions with store integration
 *
 * Key improvements:
 * - Visibility-aware spectrum updates (pause in background)
 * - Reduced debug logging in production
 * - Improved error handling
 */

import { h } from 'vue';
import { songScrobble } from '@/api/song';
// Import stores directly to avoid circular dependency through barrel exports
// (musicData.ts imports from @/utils/AudioContext, which re-exports this file)
import useMusicDataStore from '@/store/musicData';
import useSettingDataStore from '@/store/settingData';
import useSiteDataStore from '@/store/siteData';
import useUserDataStore from '@/store/userData';

const musicStore = () => useMusicDataStore();
const settingStore = () => useSettingDataStore();
const siteStore = () => useSiteDataStore();
const userStore = () => useUserDataStore();
import { NIcon } from 'naive-ui';
import { MusicNoteFilled } from '@vicons/material';
import getLanguageData from '@/utils/getLanguageData';
import { getCoverColor } from '@/utils/getCoverColor';
import { BufferedSound } from './BufferedSound';
import { SoundManager } from './SoundManager';
import { AudioContextManager } from './AudioContextManager';
import type { ISound } from './types';

const IS_DEV = import.meta.env?.DEV ?? false;

// 歌曲信息更新定时器
let timeupdateInterval: number | null = null;
// 听歌打卡延时器
let scrobbleTimeout: ReturnType<typeof setTimeout> | null = null;
// 重试次数
let testNumber = 0;
// 频谱更新动画帧 ID
let spectrumAnimationId: number | null = null;
// 页面可见性状态
let isPageVisible = true;

// Track page visibility for spectrum throttling
if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', () => {
    isPageVisible = document.visibilityState === 'visible';
  });
}

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
 * Pauses updates when page is not visible (saves CPU in background)
 * @param sound - 音频对象
 * @param music - pinia store
 */
const startSpectrumUpdate = (sound: ISound, music: ReturnType<typeof musicStore>): void => {
  stopSpectrumUpdate();

  const updateLoop = (): void => {
    if (!sound) {
      spectrumAnimationId = null;
      return;
    }

    // Skip spectrum computation when page is not visible
    if (isPageVisible) {
      // 获取频谱数据 (getFrequencyData also computes average internally)
      const frequencyData = sound.getFrequencyData();
      music.spectrumsData = Array.from(frequencyData);

      // 使用 AudioEffectManager 内部计算的平均振幅
      music.spectrumsScaleData = Math.round((sound.getAverageAmplitude() / 255 + 1) * 100) / 100;

      // 获取低频音量 (直接从 effectManager 计算，已内置平滑处理)
      music.lowFreqVolume = sound.getLowFrequencyVolume();
    }

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
    const picUrl = music.getPlaySongData.album?.picUrl;
    const artwork = picUrl
      ? [
          { src: picUrl.replace(/^http:/, 'https:') + '?param=96y96', sizes: '96x96' },
          { src: picUrl.replace(/^http:/, 'https:') + '?param=128y128', sizes: '128x128' },
          { src: picUrl.replace(/^http:/, 'https:') + '?param=512x512', sizes: '512x512' },
        ]
      : [];

    navigator.mediaSession.metadata = new MediaMetadata({
      title: music.getPlaySongData.name,
      artist: artists.join(' & '),
      album: music.getPlaySongData.album?.name || '',
      artwork,
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

    // Mobile-specific: seekto support
    if (AudioContextManager.isMobile()) {
      try {
        navigator.mediaSession.setActionHandler('seekto', (details) => {
          if (details.seekTime !== undefined && window.$player) {
            setSeek(window.$player, details.seekTime);
          }
        });
      } catch (e) {
        // seekto not supported
      }
    }
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

    // Use BufferedSound for full audio buffering (prevents interruption on tab switch)
    const sound = new BufferedSound({
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

    if (IS_DEV) {
      console.log('[createSound] autoPlay:', autoPlay, 'getPlayState:', music.getPlayState);
    }

    if (autoPlay) {
      fadePlayOrPause(sound, 'play', music.persistData.playVolume);
    }

    // 首次加载事件
    sound?.once('load', () => {
      const songId = music.getPlaySongData?.id;
      const sourceId = music.getPlaySongData?.sourceId ? music.getPlaySongData.sourceId : 0;
      const isLogin = user.userLogin;
      const isMemory = settings.memoryLastPlaybackPosition;

      if (IS_DEV) {
        console.log('首次缓冲完成：' + songId + ' / 来源：' + sourceId);
      }

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
              if (IS_DEV) {
                console.log('歌曲打卡完成', res);
              }
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

      if (IS_DEV) {
        console.log(`开始播放：${songName} - ${songArtist}`);
      }
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
      if (IS_DEV) {
        console.log('音乐暂停');
      }
      music.setPlayState(false);
      // 更改页面标题
      window.$setSiteTitle();
    });
    // 结束事件
    sound?.on('end', () => {
      if (timeupdateInterval) cancelAnimationFrame(timeupdateInterval);
      stopSpectrumUpdate();
      if (IS_DEV) {
        console.log('歌曲播放结束');
      }
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
  if (IS_DEV) {
    console.log('[fadePlayOrPause] type:', type, 'sound:', !!sound, 'playing:', sound?.playing());
  }
  const settingData = JSON.parse(localStorage.getItem('settingData') || '{}');
  const isFade = settingData.songVolumeFade ?? true;
  if (isFade) {
    if (type === 'play') {
      if (sound?.playing()) {
        return;
      }
      sound?.play();
      sound?.once('play', () => {
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
  if (IS_DEV) {
    console.log('processSpectrum called - now handled internally by NativeSound');
  }
};
