import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { musicStore } from "@/store";
import { activeSound } from "@/utils/AudioContext/sound/SoundManager";
import { isBackendSound } from "@/utils/AudioContext/sound/types";
import { getServedLevel } from "@/utils/AudioContext/resolveSongUrl";

// Every level the settings can ask for; each has a `setting.<level>` label.
const LEVELS = new Set([
  "standard",
  "higher",
  "exhigh",
  "lossless",
  "hires",
  "jyeffect",
  "jymaster",
]);
// No lossy codec Netease serves goes past 320 kbps; FLAC at 16/44.1 sits far above this.
const LOSSLESS_MIN_BITRATE = 400_000;
// By the names both native decoders give them (Symphonia's short name, FFmpeg's codec name).
const LOSSLESS_CODECS = new Set(["flac", "alac", "ape", "wavpack", "tta", "truehd", "mlp"]);
const isLosslessCodec = (codec: string) =>
  LOSSLESS_CODECS.has(codec) || codec.startsWith("pcm_") || codec.startsWith("dsd_");

/**
 * 音质标签：网易实际返回的音质（不是设置里请求的那档），原生后端在播时再加上实际的采样率与码率。
 */
export function useQualityText() {
  const music = musicStore();
  const { t } = useI18n();

  const nativeAudioQuality = computed(() => {
    if (!music.getPlaySongData || music.getLoadingState) return null;
    const player = activeSound.value;
    if (!isBackendSound(player)) return null;
    return player.getAudioQuality();
  });

  const qualityLevelText = computed(() => {
    const level = getServedLevel(music.getPlaySongData?.id);
    if (level && LEVELS.has(level)) return t(`setting.${level}`);
    // Not served by Netease through us (a local file, a UNM source, a track the backend resolved
    // by itself): name only what the stream itself shows.
    const quality = nativeAudioQuality.value;
    if (!quality) return null;
    if (quality.codec) {
      // A lossy stream is no tier at all, whatever its rate.
      if (!isLosslessCodec(quality.codec)) return null;
      const hires = quality.sampleRate > 48_000 || (quality.bitsPerSample ?? 0) > 16;
      return t(hires ? "setting.hires" : "setting.lossless");
    }
    // No codec reported (the web path, an older backend): the stream's rate is all there is.
    if (quality.sampleRate > 48_000) return t("setting.hires");
    if (quality.bitrate >= LOSSLESS_MIN_BITRATE) return t("setting.lossless");
    return null;
  });

  const formatSampleRate = (sampleRate: number) => {
    if (!Number.isFinite(sampleRate) || sampleRate <= 0) return null;
    const khz = sampleRate / 1000;
    return `${Number.isInteger(khz) ? khz.toFixed(0) : khz.toFixed(1)} kHz`;
  };

  const formatBitrate = (bitrate: number) => {
    if (!Number.isFinite(bitrate) || bitrate <= 0) return null;
    const kbps = bitrate / 1000;
    return `${kbps >= 100 ? Math.round(kbps) : kbps.toFixed(1)} kbps`;
  };

  return computed(() => {
    const quality = nativeAudioQuality.value;
    const details = quality
      ? [
          quality.bitsPerSample ? `${quality.bitsPerSample}-bit` : null,
          formatSampleRate(quality.sampleRate),
          formatBitrate(quality.bitrate),
        ]
      : [];
    const parts = [qualityLevelText.value, ...details].filter(Boolean);
    return parts.length ? parts.join(" · ") : null;
  });
}
