import { computed } from "vue";
import { musicStore, settingStore } from "@/store";
import { activeSound } from "@/utils/AudioContext/SoundManager";
import { NativeRustSound } from "@/utils/tauri/audio/nativeRustSound";

/** 音质标签：所选音质，原生后端在播时再加上实际的采样率与码率。 */
export function useQualityText() {
  const music = musicStore();
  const setting = settingStore();

  const qualityLevelText = computed(() => {
    const level = setting.songLevel;
    const qualityMap = {
      standard: "标准",
      higher: "较高",
      exhigh: "极高",
      lossless: "无损",
      hires: "Hi-Res",
    };
    return qualityMap[level] || null;
  });

  const nativeAudioQuality = computed(() => {
    if (!music.getPlaySongData || music.getLoadingState) return null;
    const player = activeSound.value;
    if (!(player instanceof NativeRustSound)) return null;
    return player.getAudioQuality();
  });

  const formatSampleRate = (sampleRate) => {
    if (!Number.isFinite(sampleRate) || sampleRate <= 0) return null;
    const khz = sampleRate / 1000;
    return `${Number.isInteger(khz) ? khz.toFixed(0) : khz.toFixed(1)} kHz`;
  };

  const formatBitrate = (bitrate) => {
    if (!Number.isFinite(bitrate) || bitrate <= 0) return null;
    const kbps = bitrate / 1000;
    return `${kbps >= 100 ? Math.round(kbps) : kbps.toFixed(1)} kbps`;
  };

  const qualityText = computed(() => {
    const quality = nativeAudioQuality.value;
    if (!quality) return qualityLevelText.value;

    const details = [formatSampleRate(quality.sampleRate), formatBitrate(quality.bitrate)].filter(
      Boolean,
    );
    if (!details.length) return qualityLevelText.value;
    return [qualityLevelText.value, ...details].filter(Boolean).join(" · ");
  });

  return qualityText;
}
