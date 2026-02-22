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
import { getAutoMixEngine } from './AutoMix';
import { getAudioPreloader } from './AudioPreloader';
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
// Reusable spectrum array to avoid Array.from() allocation every frame
let spectrumReusableArray: number[] = [];
// Background monitor interval ID (setInterval fallback when RAF is paused)
let backgroundMonitorId: ReturnType<typeof setInterval> | null = null;

/**
 * Start a setInterval(1000) fallback for when the page is hidden.
 * RAF is paused in background tabs, but setInterval still fires (~1/sec).
 * This keeps AutoMix state machine and time tracking alive.
 */
const startBackgroundMonitor = (): void => {
  if (backgroundMonitorId !== null) return;
  backgroundMonitorId = setInterval(() => {
    const sound = SoundManager.getCurrentSound();
    if (!sound) {
      stopBackgroundMonitor();
      return;
    }
    const autoMix = getAutoMixEngine();
    autoMix.monitorPlayback(sound);
    checkAudioTime(sound, musicStore());
  }, 1000);
};

/**
 * Stop the background monitor interval.
 */
const stopBackgroundMonitor = (): void => {
  if (backgroundMonitorId !== null) {
    clearInterval(backgroundMonitorId);
    backgroundMonitorId = null;
  }
};

// Track page visibility for spectrum throttling
if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', () => {
    isPageVisible = document.visibilityState === 'visible';
    if (!isPageVisible && spectrumAnimationId !== null) {
      // Page hidden while spectrum loop is active — start background fallback
      startBackgroundMonitor();
    } else if (isPageVisible) {
      // Page visible again — RAF loop resumes naturally, stop interval
      stopBackgroundMonitor();
    }
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
  stopBackgroundMonitor();
};

/**
 * 启动频谱更新
 * Pauses updates when page is not visible (saves CPU in background)
 * On mobile, skips full spectrum copy when only lowFreqVolume is needed
 * @param sound - 音频对象
 * @param music - pinia store
 */
const startSpectrumUpdate = (sound: ISound, music: ReturnType<typeof musicStore>): void => {
  stopSpectrumUpdate();

  // If page is already hidden, start background monitor immediately
  if (!isPageVisible) {
    startBackgroundMonitor();
  }

  const settings = settingStore();
  const needsSpectrum = settings.musicFrequency;
  const needsLowFreq = settings.dynamicFlowSpeed;

  const updateLoop = (): void => {
    if (!sound) {
      spectrumAnimationId = null;
      return;
    }

    // AutoMix: monitor playback position per frame
    const autoMix = getAutoMixEngine();
    autoMix.monitorPlayback(sound);

    // Skip spectrum computation when page is not visible
    if (isPageVisible) {
      if (needsSpectrum) {
        // Full spectrum path: read AnalyserNode + copy to store
        const frequencyData = sound.getFrequencyData();
        const len = frequencyData.length;

        // Reuse array to avoid allocating a new one every frame (~60fps)
        if (spectrumReusableArray.length !== len) {
          spectrumReusableArray = new Array(len);
        }
        for (let i = 0; i < len; i++) {
          spectrumReusableArray[i] = frequencyData[i];
        }
        music.spectrumsData = spectrumReusableArray;

        // 使用 AudioEffectManager 内部计算的平均振幅
        music.spectrumsScaleData = Math.round((sound.getAverageAmplitude() / 255 + 1) * 100) / 100;
      } else if (needsLowFreq) {
        // Only lowFreqVolume needed — populate AnalyserNode buffer for mobile fallback
        sound.getFrequencyData();
      }

      if (needsLowFreq) {
        // 获取低频音量 (直接从 effectManager 计算，已内置平滑处理)
        music.lowFreqVolume = sound.getLowFrequencyVolume();
      }
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
export const createSound = (src: string, autoPlay = true, preloadedSound?: BufferedSound): ISound | undefined => {
  try {
    // If AutoMix is crossfading, it handles sound creation — skip normal flow
    const autoMix = getAutoMixEngine();
    if (autoMix.isCrossfading()) {
      if (IS_DEV) {
        console.log('[createSound] AutoMix crossfade active, skipping normal sound creation');
      }
      return window.$player;
    }

    SoundManager.unload();
    stopSpectrumUpdate();

    // If not using a preloaded sound, clean up any orphaned preload
    if (!preloadedSound) {
      getAudioPreloader().cleanup();
    }

    const music = musicStore();
    const site = siteStore();
    const settings = settingStore();
    const user = userStore();

    // Use preloaded sound or create a new BufferedSound
    const sound = preloadedSound ?? new BufferedSound({
      src: [src],
      preload: true,
      volume: music.persistData.playVolume,
    });
    if (preloadedSound) {
      // Preloaded sound was created with volume=0 — restore actual volume
      sound.volume(music.persistData.playVolume);
    }
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
      // If this sound is no longer the active player (e.g., AutoMix transitioned
      // to a new sound), don't run the full play handler — avoids duplicate
      // notifications, wrong time tracking, and wrong spectrum monitoring.
      if (window.$player && window.$player !== sound) return;
      // During AutoMix crossfade, this sound is the outgoing one being resumed —
      // skip to prevent side effects (window.$player may not be updated yet).
      const autoMixCheck = getAutoMixEngine();
      if (autoMixCheck.isCrossfading()) {
        if (IS_DEV) {
          console.log('[createSound play handler] Skipped during AutoMix crossfade');
        }
        return;
      }
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
      getAudioPreloader().preloadNext();
      music.preloadUpcomingSongs();

      // Notify AutoMix engine that a new track started
      const songId = music.getPlaySongData?.id;
      if (songId) {
        getAutoMixEngine().onTrackStarted(sound, songId);
      }

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

      // 启动频谱更新 (also needed for AutoMix playback monitoring)
      if (settings.musicFrequency || settings.dynamicFlowSpeed || settings.autoMixEnabled) {
        startSpectrumUpdate(sound, music);
      }
    });

    // 暂停事件
    sound?.on('pause', () => {
      // If this sound is no longer the active player, ignore
      if (window.$player && window.$player !== sound) return;
      // During AutoMix crossfade, outgoing sound pausing is expected — don't change play state
      const autoMix = getAutoMixEngine();
      if (autoMix.isCrossfading()) {
        if (IS_DEV) {
          console.log('[pause handler] Ignored during AutoMix crossfade');
        }
        return;
      }
      if (timeupdateInterval) cancelAnimationFrame(timeupdateInterval);
      if (IS_DEV) {
        console.log('音乐暂停');
      }
      music.setPlayState(false);
      // 更改页面标题
      window.$setSiteTitle("");
    });
    // 结束事件
    sound?.on('end', () => {
      // If this sound is no longer the active player (e.g., AutoMix transitioned
      // to a new sound), don't cancel the current player's time/spectrum loops
      if (window.$player && window.$player !== sound) return;
      if (timeupdateInterval) cancelAnimationFrame(timeupdateInterval);
      stopSpectrumUpdate();
      if (IS_DEV) {
        console.log('歌曲播放结束');
      }
      // If AutoMix handled the transition, don't trigger next song again
      const autoMixEngine = getAutoMixEngine();
      if (!autoMixEngine.isCrossfading()) {
        music.setPlaySongIndex('next');
      }
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
        music.isLoadingSong = false;
      }
    });
    sound?.on('playerror', () => {
      window.$message.error(getLanguageData('songPlayError'));
      console.error(getLanguageData('songPlayError'));
      music.setPlayState(false);
      music.isLoadingSong = false;
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

  // When AutoMix normalization is active, apply the persistent gain adjustment
  // to the GainNode so that manual volume changes maintain LUFS normalization.
  const autoMix = getAutoMixEngine();
  const gainAdj = autoMix.getActiveGainAdjustment();
  if (gainAdj !== 1 && sound) {
    const gainNode = (sound as any).getGainNode?.();
    if (gainNode) {
      gainNode.gain.value = volume * gainAdj;
    }
  }
};

/**
 * 设置进度
 * @param sound - 音频对象
 * @param seek - 设置的进度值（秒）
 */
export const setSeek = (sound: ISound | undefined, seek: number): void => {
  const music = musicStore();
  // Cancel AutoMix crossfade on seek
  const autoMix = getAutoMixEngine();
  if (autoMix.isCrossfading()) {
    autoMix.cancelCrossfade();
  }
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
 * Adopt an incoming sound created by AutoMix.
 * Registers event handlers (pause/end/error), starts time tracking and spectrum updates.
 * Called after the incoming sound is already playing.
 * @param incomingSound - The incoming sound instance from AutoMix crossfade
 */
export const adoptIncomingSound = (incomingSound: ISound): void => {
  const music = musicStore();
  const settings = settingStore();

  // Stop any existing tracking from outgoing sound
  stopSpectrumUpdate();
  if (timeupdateInterval) {
    cancelAnimationFrame(timeupdateInterval);
    timeupdateInterval = null;
  }

  // Ensure play state is true (incoming is already playing)
  music.setPlayState(true);
  music.isLoadingSong = false;

  // Initialize time from the incoming sound immediately.
  // The song data watcher no longer resets time during crossfade (to prevent duration=0),
  // so we must do it here to avoid briefly showing the outgoing song's time.
  const initDuration = incomingSound.duration();
  const initTime = (incomingSound.seek() as number) || 0;
  music.setPlaySongTime({
    currentTime: initTime,
    duration: initDuration > 0 ? initDuration : 0,
  });

  // Update cover color for the new track (AutoMix bypasses createSound, so do it here)
  const site = siteStore();
  const coverUrl = music.getPlaySongData?.album?.picUrl;
  if (coverUrl) {
    getCoverColor(coverUrl)
      .then((color) => {
        site.songPicGradient = color;
      })
      .catch((err) => {
        console.error('取色出错 (AutoMix)', err);
      });
  }

  // Start time tracking immediately
  const timeLoop = (): void => {
    checkAudioTime(incomingSound, music);
    timeupdateInterval = requestAnimationFrame(timeLoop);
  };
  timeLoop();

  // Start spectrum update (also handles AutoMix monitorPlayback per frame)
  if (settings.musicFrequency || settings.dynamicFlowSpeed || settings.autoMixEnabled) {
    startSpectrumUpdate(incomingSound, music);
  }

  // Update media session with current song info
  setMediaSession(music);

  // Write play history & preload next songs (AutoMix bypasses createSound's play handler)
  const playSongData = music.getPlaySongData;
  if (playSongData) {
    music.setPlayHistory(playSongData);
    // Page title
    const songName = playSongData.name;
    const songArtist = playSongData.artist?.[0]?.name;
    if (songName && songArtist) {
      window.document.title = `${songName} - ${songArtist} - ${import.meta.env.VITE_SITE_TITLE}`;
    }
  }
  getAudioPreloader().preloadNext();
  music.preloadUpcomingSongs();

  // Register pause handler with crossfade guard
  incomingSound.on('pause', () => {
    // If this sound is no longer the active player, ignore
    if (window.$player && window.$player !== incomingSound) return;
    const autoMix = getAutoMixEngine();
    if (autoMix.isCrossfading()) {
      if (IS_DEV) {
        console.log('[adoptIncomingSound pause] Ignored during AutoMix crossfade');
      }
      return;
    }
    if (timeupdateInterval) cancelAnimationFrame(timeupdateInterval);
    if (IS_DEV) {
      console.log('音乐暂停 (adopted sound)');
    }
    music.setPlayState(false);
    window.$setSiteTitle("");
  });

  // Register play handler to restart time loop + spectrum after pause/resume.
  // Without this, pausing and resuming after AutoMix crossfade permanently
  // kills time tracking (pause handler cancels timeupdateInterval but nothing restarts it).
  incomingSound.on('play', () => {
    if (timeupdateInterval) {
      cancelAnimationFrame(timeupdateInterval);
    }

    music.setPlayState(true);

    // Restart time tracking
    const tLoop = (): void => {
      checkAudioTime(incomingSound, music);
      timeupdateInterval = requestAnimationFrame(tLoop);
    };
    tLoop();

    // Restart spectrum + AutoMix monitoring
    if (settings.musicFrequency || settings.dynamicFlowSpeed || settings.autoMixEnabled) {
      startSpectrumUpdate(incomingSound, music);
    }
  });

  // Register end handler
  incomingSound.on('end', () => {
    // If this sound is no longer the active player, don't cancel current loops
    if (window.$player && window.$player !== incomingSound) return;
    if (timeupdateInterval) cancelAnimationFrame(timeupdateInterval);
    stopSpectrumUpdate();
    if (IS_DEV) {
      console.log('歌曲播放结束 (adopted sound)');
    }
    const autoMixEngine = getAutoMixEngine();
    if (!autoMixEngine.isCrossfading()) {
      music.setPlaySongIndex('next');
    }
  });

  // Register error handlers
  incomingSound.on('loaderror', () => {
    music.setPlayState(false);
  });
  incomingSound.on('playerror', () => {
    music.setPlayState(false);
  });

  if (IS_DEV) {
    console.log('[adoptIncomingSound] Adopted incoming sound with event handlers');
  }
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
