<template>
  <div class="settings-dsp">
    <div class="custom-heading">
      <div class="name">
        {{ t("setting.dspTitle") }}
        <span class="tip">{{ t("setting.dspTip") }}</span>
      </div>
      <n-switch v-model:value="dspEnabled" :round="false" />
    </div>

    <n-alert v-if="!supported" type="warning" :bordered="false">
      {{ t("setting.dspNativeOnly") }}
    </n-alert>

    <div class="dsp-controls">
      <div class="control-grid">
        <div class="control-row">
          <div class="row-name">
            {{ t("setting.dspEqualizer") }}
            <span>{{ t("setting.dspEqualizerTip") }}</span>
          </div>
          <n-switch v-model:value="dspEqEnabled" :round="false" :disabled="!dspEnabled" />
        </div>

        <div class="control-row">
          <div class="row-name">
            {{ t("setting.dspPreset") }}
            <span>{{ t("setting.dspPresetTip") }}</span>
          </div>
          <n-select
            class="compact-control"
            :value="dspEqPreset"
            :options="presetOptions"
            :disabled="!dspEnabled || !dspEqEnabled"
            @update:value="applyPreset"
          />
        </div>

        <div class="control-row">
          <div class="row-name">
            {{ t("setting.dspBandCount") }}
            <span>{{ t("setting.dspBandCountTip") }}</span>
          </div>
          <n-select
            class="compact-control"
            :value="dspEqBandCount"
            :options="bandCountOptions"
            :disabled="!dspEnabled || !dspEqEnabled"
            @update:value="setBandCount"
          />
        </div>
      </div>

      <div class="preamp-row">
        <span>{{ t("setting.dspPreamp") }}</span>
        <n-slider
          :value="dspEqPreampDb"
          :min="-12"
          :max="12"
          :step="0.1"
          :disabled="!dspEnabled || !dspEqEnabled"
          :format-tooltip="formatDb"
          @update:value="setPreamp"
        />
        <strong>{{ formatDb(dspEqPreampDb) }}</strong>
      </div>

      <div class="eq-stage" :class="{ disabled: !dspEnabled || !dspEqEnabled }">
        <div class="eq-ruler">
          <span>+12</span>
          <span>0</span>
          <span>-12</span>
        </div>
        <div class="eq-strip">
          <div v-for="(band, index) in dspEqBands" :key="index" class="eq-band">
            <span class="band-gain">{{ formatDb(band.gainDb) }}</span>
            <n-slider
              class="band-slider"
              vertical
              :value="band.gainDb"
              :min="-12"
              :max="12"
              :step="0.1"
              :disabled="!dspEnabled || !dspEqEnabled || !band.enabled"
              :format-tooltip="formatDb"
              @update:value="(value) => setBandGain(index, value)"
            />
            <n-button
              class="band-enable"
              size="tiny"
              strong
              secondary
              :type="band.enabled ? 'primary' : 'default'"
              :disabled="!dspEnabled || !dspEqEnabled"
              @click="toggleBand(index)"
            >
              {{ band.enabled ? "ON" : "OFF" }}
            </n-button>
            <span class="band-frequency">{{ formatFrequency(band.frequency) }}</span>
            <n-select
              class="band-filter"
              size="small"
              :value="band.filterType"
              :options="filterTypeOptions"
              :disabled="!dspEnabled || !dspEqEnabled"
              @update:value="(value) => setBandFilter(index, value)"
            />
            <n-input-number
              class="band-frequency-input"
              size="small"
              :value="band.frequency"
              :min="10"
              :max="22000"
              :step="frequencyStep(band.frequency)"
              :disabled="!dspEnabled || !dspEqEnabled"
              @update:value="(value) => setBandFrequency(index, value)"
            />
            <n-input-number
              class="band-q"
              size="small"
              :value="band.q"
              :min="0.1"
              :max="18"
              :step="0.1"
              :disabled="!dspEnabled || !dspEqEnabled"
              @update:value="(value) => setBandQ(index, value)"
            />
          </div>
        </div>
      </div>

      <div class="button-row">
        <n-button strong secondary :disabled="!dspEnabled || !dspEqEnabled" @click="resetEq">
          {{ t("setting.dspResetEq") }}
        </n-button>
      </div>

      <n-divider />

      <div class="control-row">
        <div class="row-name">
          {{ t("setting.dspLimiter") }}
          <span>{{ t("setting.dspLimiterTip") }}</span>
        </div>
        <n-switch v-model:value="dspLimiterEnabled" :round="false" :disabled="!dspEnabled" />
      </div>

      <div class="limiter-grid">
        <n-form-item :label="t('setting.dspLimiterThreshold')">
          <n-input-number
            v-model:value="dspLimiterThresholdDb"
            :min="-24"
            :max="-0.1"
            :step="0.5"
            :disabled="!dspEnabled || !dspLimiterEnabled"
          />
        </n-form-item>
        <n-form-item :label="t('setting.dspLimiterCeiling')">
          <n-input-number
            v-model:value="dspLimiterCeilingDb"
            :min="-12"
            :max="0"
            :step="0.5"
            :disabled="!dspEnabled || !dspLimiterEnabled"
          />
        </n-form-item>
        <n-form-item :label="t('setting.dspLimiterRelease')">
          <n-input-number
            v-model:value="dspLimiterReleaseMs"
            :min="5"
            :max="2000"
            :step="5"
            :disabled="!dspEnabled || !dspLimiterEnabled"
          />
        </n-form-item>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { storeToRefs } from "pinia";
import { useI18n } from "vue-i18n";
import { settingStore } from "@/store";
import { isTauri } from "@/utils/tauri";

type FilterType = "peaking" | "lowShelf" | "highShelf";

type EqBandSetting = {
  enabled: boolean;
  filterType: FilterType;
  frequency: number;
  gainDb: number;
  q: number;
};

type EqPreset = {
  label: string;
  value: string;
  gainAt: (frequency: number) => number;
};

const EQ_FREQUENCY_SETS: Record<number, number[]> = {
  10: [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000],
  15: [25, 40, 63, 100, 160, 250, 400, 630, 1000, 1600, 2500, 4000, 6300, 10000, 16000],
  31: [
    20, 25, 31.5, 40, 50, 63, 80, 100, 125, 160, 200, 250, 315, 400, 500, 630, 800, 1000, 1250,
    1600, 2000, 2500, 3150, 4000, 5000, 6300, 8000, 10000, 12500, 16000, 20000,
  ],
};

const { t } = useI18n();
const setting = settingStore();
const {
  dspEnabled,
  dspEqEnabled,
  dspEqPreampDb,
  dspEqPreset,
  dspEqBandCount,
  dspEqBands,
  dspLimiterEnabled,
  dspLimiterThresholdDb,
  dspLimiterCeilingDb,
  dspLimiterReleaseMs,
} = storeToRefs(setting);

const supported = computed(() => isTauri());

const curve = (points: Array<[number, number]>) => (frequency: number) => {
  const x = Math.log2(frequency);
  const mapped = points.map(([freq, gain]) => [Math.log2(freq), gain] as const);
  if (x <= mapped[0][0]) return mapped[0][1];
  for (let i = 1; i < mapped.length; i += 1) {
    const [nextFreq, nextGain] = mapped[i];
    const [prevFreq, prevGain] = mapped[i - 1];
    if (x <= nextFreq) {
      const t = (x - prevFreq) / (nextFreq - prevFreq);
      return prevGain + (nextGain - prevGain) * t;
    }
  }
  return mapped[mapped.length - 1][1];
};

const presets: EqPreset[] = [
  { label: "setting.dspPresetFlat", value: "flat", gainAt: () => 0 },
  {
    label: "setting.dspPresetBass",
    value: "bass",
    gainAt: curve([
      [20, 5],
      [63, 4],
      [125, 3],
      [250, 1.5],
      [500, 0],
      [20000, 0],
    ]),
  },
  {
    label: "setting.dspPresetVocal",
    value: "vocal",
    gainAt: curve([
      [20, -2],
      [125, -1],
      [250, 0],
      [500, 2],
      [1000, 3],
      [2500, 2.5],
      [5000, 0],
      [20000, -2],
    ]),
  },
  {
    label: "setting.dspPresetRock",
    value: "rock",
    gainAt: curve([
      [20, 3],
      [80, 2],
      [250, -1],
      [500, -2],
      [1000, 0],
      [4000, 3],
      [8000, 2],
      [20000, 1],
    ]),
  },
  {
    label: "setting.dspPresetElectronic",
    value: "electronic",
    gainAt: curve([
      [20, 4],
      [80, 3],
      [250, 1],
      [500, -1],
      [1000, 0],
      [4000, 3],
      [10000, 4],
      [20000, 3],
    ]),
  },
  {
    label: "setting.dspPresetClassical",
    value: "classical",
    gainAt: curve([
      [20, 0],
      [2000, 0],
      [4000, -1],
      [8000, 0],
      [16000, 2],
      [20000, 2],
    ]),
  },
];

const presetOptions = computed(() => [
  ...presets.map((preset) => ({
    label: t(preset.label),
    value: preset.value,
  })),
  { label: t("setting.dspPresetCustom"), value: "custom", disabled: true },
]);

const bandCountOptions = computed(() => [
  { label: "10", value: 10 },
  { label: "15", value: 15 },
  { label: "31", value: 31 },
]);

const filterTypeOptions = computed(() => [
  { label: t("setting.dspFilterLowShelf"), value: "lowShelf" },
  { label: t("setting.dspFilterPeaking"), value: "peaking" },
  { label: t("setting.dspFilterHighShelf"), value: "highShelf" },
]);

const formatDb = (value: number) => {
  const fixed = value.toFixed(value % 1 === 0 ? 0 : 1);
  return `${value > 0 ? "+" : ""}${fixed} dB`;
};

const formatFrequency = (frequency: number) => {
  if (frequency >= 1000) return `${Number((frequency / 1000).toFixed(2))}k`;
  return `${Number(frequency.toFixed(1))}`;
};

const frequencyStep = (frequency: number) => {
  if (frequency >= 10000) return 100;
  if (frequency >= 1000) return 10;
  return 1;
};

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

const defaultBand = (frequency: number, index: number, count: number): EqBandSetting => ({
  enabled: true,
  filterType: index === 0 ? "lowShelf" : index === count - 1 ? "highShelf" : "peaking",
  frequency,
  gainDb: 0,
  q: count >= 31 ? 4.318 : 1.414,
});

const nearestBand = (frequency: number, bands: EqBandSetting[]) =>
  bands.reduce((nearest, band) => {
    const nearestDistance = Math.abs(Math.log2(nearest.frequency) - Math.log2(frequency));
    const bandDistance = Math.abs(Math.log2(band.frequency) - Math.log2(frequency));
    return bandDistance < nearestDistance ? band : nearest;
  }, bands[0]);

const markCustomPreset = () => {
  dspEqPreset.value = "custom";
};

const setBandGain = (index: number, value: number) => {
  const band = dspEqBands.value[index];
  if (!band) return;
  band.gainDb = value;
  markCustomPreset();
};

const setBandFilter = (index: number, value: FilterType) => {
  const band = dspEqBands.value[index];
  if (!band) return;
  band.filterType = value;
  markCustomPreset();
};

const setBandFrequency = (index: number, value: number | null) => {
  const band = dspEqBands.value[index];
  if (!band || value === null) return;
  band.frequency = clamp(value, 10, 22000);
  markCustomPreset();
};

const setBandQ = (index: number, value: number | null) => {
  const band = dspEqBands.value[index];
  if (!band || value === null) return;
  band.q = clamp(value, 0.1, 18);
  markCustomPreset();
};

const toggleBand = (index: number) => {
  const band = dspEqBands.value[index];
  if (!band) return;
  band.enabled = !band.enabled;
  markCustomPreset();
};

const setPreamp = (value: number) => {
  dspEqPreampDb.value = value;
  markCustomPreset();
};

const setBandCount = (value: number) => {
  const frequencies = EQ_FREQUENCY_SETS[value] ?? EQ_FREQUENCY_SETS[10];
  const previous = dspEqBands.value.slice() as EqBandSetting[];
  dspEqBandCount.value = value;
  dspEqBands.value = frequencies.map((frequency, index) => {
    const fallback = defaultBand(frequency, index, frequencies.length);
    if (!previous.length) return fallback;
    const nearest = nearestBand(frequency, previous);
    return {
      ...fallback,
      enabled: nearest.enabled,
      gainDb: nearest.gainDb,
    };
  });
  markCustomPreset();
};

const applyPreset = (value: string) => {
  const preset = presets.find((item) => item.value === value);
  if (!preset) return;
  dspEqPreset.value = preset.value;
  dspEqPreampDb.value = 0;
  dspEqBands.value.forEach((band) => {
    band.enabled = true;
    band.gainDb = Number(preset.gainAt(band.frequency).toFixed(1));
  });
};

const resetEq = () => {
  applyPreset("flat");
};
</script>

<style lang="scss" scoped>
.settings-dsp {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 14px;

  .custom-heading {
    width: 100%;
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;

    .name {
      min-width: 0;
      display: flex;
      flex-direction: column;
      gap: 3px;
      font-size: 15px;

      .tip {
        font-size: 12px;
        line-height: 1.45;
        opacity: 0.68;
      }
    }
  }

  .dsp-controls {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .control-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 12px;
  }

  .control-row,
  .preamp-row,
  .button-row {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
  }

  .control-row {
    min-width: 0;
  }

  .row-name {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 14px;

    span {
      font-size: 12px;
      line-height: 1.45;
      opacity: 0.68;
    }
  }

  .compact-control {
    width: min(156px, 36vw);
  }

  .preamp-row {
    display: grid;
    grid-template-columns: 86px minmax(160px, 1fr) 72px;

    > span {
      font-size: 13px;
      opacity: 0.8;
    }

    strong {
      font-size: 13px;
      font-variant-numeric: tabular-nums;
      text-align: right;
    }
  }

  .eq-stage {
    width: 100%;
    display: grid;
    grid-template-columns: 34px minmax(0, 1fr);
    gap: 10px;
    padding: 10px 0 2px;
    transition: opacity var(--duration-150) var(--ease-out);

    &.disabled {
      opacity: 0.56;
    }
  }

  .eq-ruler {
    height: 214px;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    padding: 22px 0 34px;
    color: var(--n-text-color-3);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    box-sizing: border-box;
  }

  .eq-strip {
    min-width: 0;
    display: flex;
    gap: 10px;
    padding: 2px 2px 10px;
    overflow-x: auto;
    overscroll-behavior-x: contain;
    scrollbar-width: thin;
  }

  .eq-band {
    flex: 0 0 72px;
    min-width: 72px;
    display: grid;
    grid-template-rows: 20px 176px 22px 20px 30px 30px 30px;
    align-items: center;
    justify-items: center;
    gap: 6px;
  }

  .band-slider {
    height: 176px;
  }

  .band-enable {
    width: 46px;
    height: 22px;
    font-size: 11px;
    font-weight: 700;
  }

  .band-frequency,
  .band-gain {
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    opacity: 0.72;
  }

  .band-filter,
  .band-frequency-input,
  .band-q {
    width: 68px;
  }

  .button-row {
    justify-content: flex-end;
  }

  .limiter-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 12px;

    :deep(.n-form-item-feedback-wrapper) {
      min-height: 0;
    }

    :deep(.n-input-number) {
      width: 100%;
    }
  }
}

@media (max-width: 920px) {
  .settings-dsp {
    .control-grid {
      grid-template-columns: 1fr;
    }

    .control-row {
      align-items: flex-start;
      flex-direction: column;
    }

    .compact-control {
      width: 100%;
    }
  }
}

@media (max-width: 760px) {
  .settings-dsp {
    .preamp-row,
    .limiter-grid {
      grid-template-columns: 1fr;
    }

    .eq-stage {
      grid-template-columns: 28px minmax(0, 1fr);
    }
  }
}
</style>
