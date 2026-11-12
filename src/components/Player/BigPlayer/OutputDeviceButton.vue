<template>
  <n-popselect
    v-model:value="setting.audioOutputDevice"
    :options="options"
    :placement="placement"
    trigger="click"
    scrollable
    :z-index="2200"
    @update:show="(show) => show && refresh()"
  >
    <button
      v-bind="$attrs"
      type="button"
      class="output-device-button"
      :aria-label="$t('player.controls.output')"
    >
      <n-icon :component="AirplayRound" />
    </button>
  </n-popselect>
</template>

<script setup lang="ts">
/**
 * Apple Music's AirPlay button for the native output: the list of devices, the current one
 * ticked. Tauri only — the parent decides whether to render it. Picking writes
 * `setting.audioOutputDevice`; `useAudioOutputSetting` sends it to the backend.
 */
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { AirplayRound } from "@vicons/material";
import { settingStore } from "@/store";
import { audioListOutputDevices } from "@/utils/tauri/audio/bridge";

defineOptions({ inheritAttrs: false });
withDefaults(defineProps<{ placement?: "top" | "top-end" }>(), { placement: "top" });

const { t } = useI18n();
const setting = settingStore();
const devices = ref<string[]>([]);

// Listed on open, not once: devices come and go while the app runs.
const refresh = async () => {
  devices.value = await audioListOutputDevices();
};

const options = computed(() => {
  const chosen = setting.audioOutputDevice;
  const list = [
    { label: t("player.controls.outputDefault"), value: "" },
    ...devices.value.map((name) => ({ label: name, value: name })),
  ];
  // A remembered device that is unplugged stays listed, so the tick says what was picked;
  // the backend plays on the default until it returns.
  if (chosen && !devices.value.includes(chosen)) {
    list.push({ label: `${chosen} · ${t("player.controls.outputUnavailable")}`, value: chosen });
  }
  return list;
});
</script>

<style lang="scss" scoped>
// The parent sizes and colours it; this is only the reset a native <button> needs.
.output-device-button {
  appearance: none;
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  cursor: pointer;

  // The AMLL toggle glyphs fill about 70% of their box, Material's about 85%; match their weight.
  :deep(svg) {
    transform: scale(0.82);
  }
}
</style>
