import { settingStore } from "@/store";
import { audioSendMsg, type DspConfig, type EqualizerBand } from "@/utils/tauri/audioBridge";

const EQ_EPSILON_DB = 0.001;
const DISABLED_DSP_CONFIG: DspConfig = {
  enabled: false,
  inputGainDb: 0,
  equalizer: {
    enabled: false,
    preampDb: 0,
    bands: [],
  },
  outputGainDb: 0,
  limiter: {
    enabled: false,
    thresholdDb: -1,
    ceilingDb: -1,
    releaseMs: 80,
  },
};

let initialized = false;
let pendingTimer: ReturnType<typeof window.setTimeout> | null = null;

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

export const buildDspConfig = (): DspConfig => {
  const setting = settingStore();

  if (!setting.dspEnabled) return DISABLED_DSP_CONFIG;

  const eqBands: EqualizerBand[] = setting.dspEqEnabled
    ? setting.dspEqBands
        .filter((band) => band.enabled && Math.abs(band.gainDb) > EQ_EPSILON_DB)
        .map((band) => ({
          enabled: true,
          filterType: band.filterType,
          frequency: band.frequency,
          gainDb: clamp(band.gainDb, -12, 12),
          q: clamp(band.q, 0.1, 18),
        }))
    : [];

  const preampDb = setting.dspEqEnabled ? clamp(setting.dspEqPreampDb, -12, 12) : 0;
  const equalizerEnabled =
    setting.dspEqEnabled && (eqBands.length > 0 || Math.abs(preampDb) > EQ_EPSILON_DB);
  const limiterEnabled = setting.dspLimiterEnabled;

  const config: DspConfig = {
    enabled: equalizerEnabled || limiterEnabled,
    inputGainDb: 0,
    equalizer: {
      enabled: equalizerEnabled,
      preampDb,
      bands: eqBands,
    },
    outputGainDb: 0,
    limiter: {
      enabled: limiterEnabled,
      thresholdDb: clamp(setting.dspLimiterThresholdDb, -24, -0.1),
      ceilingDb: clamp(setting.dspLimiterCeilingDb, -12, 0),
      releaseMs: clamp(setting.dspLimiterReleaseMs, 5, 2000),
    },
  };

  return config.enabled ? config : DISABLED_DSP_CONFIG;
};

export const applyDspSettings = () => {
  audioSendMsg({ type: "setDsp", config: buildDspConfig() });
};

const scheduleApplyDspSettings = () => {
  if (pendingTimer !== null) {
    window.clearTimeout(pendingTimer);
  }

  pendingTimer = window.setTimeout(() => {
    pendingTimer = null;
    applyDspSettings();
  }, 32);
};

export function useDspSettings() {
  if (initialized) {
    return {
      applyDspSettings,
      scheduleApplyDspSettings,
    };
  }

  initialized = true;
  const setting = settingStore();

  watch(
    () => [
      setting.dspEnabled,
      setting.dspEqEnabled,
      setting.dspEqPreampDb,
      setting.dspEqBandCount,
      setting.dspLimiterEnabled,
      setting.dspLimiterThresholdDb,
      setting.dspLimiterCeilingDb,
      setting.dspLimiterReleaseMs,
      setting.dspEqBands
        .map(
          (band) => `${band.enabled}:${band.filterType}:${band.frequency}:${band.gainDb}:${band.q}`,
        )
        .join(","),
    ],
    scheduleApplyDspSettings,
    { immediate: true },
  );

  return {
    applyDspSettings,
    scheduleApplyDspSettings,
  };
}
