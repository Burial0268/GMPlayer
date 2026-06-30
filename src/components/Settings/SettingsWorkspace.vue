<template>
  <div :class="['settings-workspace', `is-${variant}`]">
    <SettingsPanel
      :sections="sections"
      :section="resolvedSection"
      :active="activeKey"
      :searchable="searchable"
      @update:active="handleActiveUpdate"
      @action="handleAction"
    >
      <template #themeColors>
        <div class="custom-heading">
          <div class="name">
            {{ t("setting.themeType") }}
            <span class="tip">{{ t("setting.themeTypeTip") }}</span>
          </div>
          <n-button
            v-if="themeType !== 'red'"
            strong
            secondary
            @click="changeThemeColor(null, true)"
          >
            {{ t("general.name.restore") }}
          </n-button>
        </div>
        <n-grid
          class="color-select"
          :x-gap="12"
          :y-gap="12"
          responsive="screen"
          cols="2 s:3 m:4 l:5"
        >
          <n-grid-item
            v-for="item in themeColorList"
            :key="item.label"
            :style="{ '--color': item.primaryColor }"
            :class="['color-item', { check: item.label === themeType }]"
            @click="changeThemeColor(item)"
          >
            <span>{{ language === "zh-CN" ? item.name : item.label }}</span>
          </n-grid-item>
          <n-grid-item
            :class="['color-item', { check: themeType === 'custom' }]"
            :style="{ '--color': themeData.primaryColor }"
            @click="openThemeCustom"
          >
            <span>{{ t("general.name.customTheme") }}</span>
          </n-grid-item>
        </n-grid>
      </template>

      <template #springParams>
        <div class="custom-heading">
          <div class="name">
            {{ t("setting.springParams") }}
            <span class="tip">{{ t("setting.springParamsTip") }}</span>
          </div>
        </div>
        <n-collapse class="spring-panel" accordion>
          <n-collapse-item :title="t('setting.springPosX')" name="posX">
            <div class="spring-grid">
              <n-form-item :label="t('setting.springMass')">
                <n-input-number v-model:value="springParams.posX.mass" :min="0.1" :step="0.1" />
              </n-form-item>
              <n-form-item :label="t('setting.springDamping')">
                <n-input-number v-model:value="springParams.posX.damping" :min="0" :step="1" />
              </n-form-item>
              <n-form-item :label="t('setting.springStiffness')">
                <n-input-number v-model:value="springParams.posX.stiffness" :min="0" :step="1" />
              </n-form-item>
            </div>
          </n-collapse-item>
          <n-collapse-item :title="t('setting.springPosY')" name="posY">
            <div class="spring-grid">
              <n-form-item :label="t('setting.springMass')">
                <n-input-number v-model:value="springParams.posY.mass" :min="0.1" :step="0.1" />
              </n-form-item>
              <n-form-item :label="t('setting.springDamping')">
                <n-input-number v-model:value="springParams.posY.damping" :min="0" :step="1" />
              </n-form-item>
              <n-form-item :label="t('setting.springStiffness')">
                <n-input-number v-model:value="springParams.posY.stiffness" :min="0" :step="1" />
              </n-form-item>
            </div>
          </n-collapse-item>
          <n-collapse-item :title="t('setting.springScale')" name="scale">
            <div class="spring-grid">
              <n-form-item :label="t('setting.springMass')">
                <n-input-number v-model:value="springParams.scale.mass" :min="0.1" :step="0.1" />
              </n-form-item>
              <n-form-item :label="t('setting.springDamping')">
                <n-input-number v-model:value="springParams.scale.damping" :min="0" :step="1" />
              </n-form-item>
              <n-form-item :label="t('setting.springStiffness')">
                <n-input-number v-model:value="springParams.scale.stiffness" :min="0" :step="1" />
              </n-form-item>
            </div>
          </n-collapse-item>
        </n-collapse>
      </template>

      <template #appUpdate>
        <SettingsAppUpdate />
      </template>

      <template #dspSettings>
        <SettingsDsp />
      </template>
    </SettingsPanel>

    <n-modal
      v-model:show="showThemeCustom"
      class="s-modal"
      preset="card"
      :title="t('general.name.customTheme')"
      :bordered="false"
    >
      <n-form class="color-custom" :model="customColorData">
        <n-form-item :label="t('general.name.primaryColor')" path="primaryColor">
          <n-color-picker v-model:value="customColorData.primaryColor" />
        </n-form-item>
        <n-form-item :label="`${t('general.name.primaryColor')} Hover`" path="primaryColorHover">
          <n-color-picker v-model:value="customColorData.primaryColorHover" />
        </n-form-item>
        <n-form-item :label="`${t('general.name.primaryColor')} Suppl`" path="primaryColorSuppl">
          <n-color-picker v-model:value="customColorData.primaryColorSuppl" />
        </n-form-item>
        <n-form-item
          :label="`${t('general.name.primaryColor')} Pressed`"
          path="primaryColorPressed"
        >
          <n-color-picker v-model:value="customColorData.primaryColorPressed" />
        </n-form-item>
      </n-form>
      <template #footer>
        <n-space justify="end">
          <n-button @click="showThemeCustom = false">{{ t("general.dialog.cancel") }}</n-button>
          <n-button type="primary" @click="setThemeCustom">
            {{ t("general.name.customTheme") }}
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <n-modal
      v-model:show="showEploryModal"
      class="s-modal"
      preset="dialog"
      :title="t('setting.eploryBackgroundConfig')"
    >
      <n-form class="settings-form">
        <n-form-item :label="t('setting.eplorySetting.fps.title')">
          <n-input-number v-model:value="fps" :min="0.1" />
          <template #feedback>{{ t("setting.eplorySetting.fps.tip") }}</template>
        </n-form-item>
        <n-form-item :label="t('setting.dynamicFlowSpeed')">
          <n-switch v-model:value="dynamicFlowSpeed" />
          <template #feedback>{{ t("setting.dynamicFlowSpeedTip") }}</template>
        </n-form-item>
        <n-form-item :label="t('setting.dynamicFlowSpeedScale')">
          <n-input-number
            v-model:value="dynamicFlowSpeedScale"
            :min="0.1"
            :disabled="!dynamicFlowSpeed"
          />
          <template #feedback>{{ t("setting.dynamicFlowSpeedScaleTip") }}</template>
        </n-form-item>
        <n-form-item :label="t('setting.eplorySetting.flowSpeed.title')">
          <n-input-number v-model:value="flowSpeed" :min="0.1" :disabled="dynamicFlowSpeed" />
          <template #feedback>{{ t("setting.eplorySetting.flowSpeed.tip") }}</template>
        </n-form-item>
        <n-form-item :label="t('setting.eplorySetting.renderScale.title')">
          <n-input-number v-model:value="renderScale" :min="0.1" />
          <template #feedback>{{ t("setting.eplorySetting.renderScale.tip") }}</template>
        </n-form-item>
        <n-form-item :label="t('setting.eplorySetting.albumImageUrl.title')">
          <n-input v-model:value="albumImageUrl" />
          <template #feedback>{{ t("setting.eplorySetting.albumImageUrl.tip") }}</template>
        </n-form-item>
      </n-form>
    </n-modal>

    <n-modal
      v-model:show="showBlurModal"
      class="s-modal"
      preset="dialog"
      :title="t('setting.blurBackgroundConfig')"
    >
      <n-form class="settings-form">
        <n-form-item :label="t('setting.blurFps')">
          <n-input-number v-model:value="fps" :min="1" :max="60" />
          <template #feedback>{{ t("setting.blurFpsTip") }}</template>
        </n-form-item>
        <n-form-item :label="t('setting.blurAmount')">
          <n-input-number v-model:value="blurAmount" :min="1" :max="100" />
          <template #feedback>{{ t("setting.blurAmountTip") }}</template>
        </n-form-item>
        <n-form-item :label="t('setting.contrastAmount')">
          <n-input-number v-model:value="contrastAmount" :min="0.1" :max="3" :step="0.1" />
          <template #feedback>{{ t("setting.contrastAmountTip") }}</template>
        </n-form-item>
        <n-form-item :label="t('setting.blurRenderScale')">
          <n-input-number v-model:value="renderScale" :min="0.1" :max="1" :step="0.1" />
          <template #feedback>{{ t("setting.blurRenderScaleTip") }}</template>
        </n-form-item>
      </n-form>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { storeToRefs } from "pinia";
import { useI18n } from "vue-i18n";
import { settingStore } from "@/store";
import themeColorData from "@/components/Provider/themeColor.json";
import SettingsPanel from "./SettingsPanel.vue";
import SettingsAppUpdate from "./SettingsAppUpdate.vue";
import SettingsDsp from "./SettingsDsp.vue";
import { SETTINGS_SECTION_ALIASES, useSettingsSections } from "./useSettingsSections";

declare const $cleanAll: any;
declare const $dialog: any;
declare const $message: any;

type ThemeColorItem = {
  label: string;
  name: string;
  primaryColor: string;
  primaryColorHover: string;
  primaryColorSuppl: string;
  primaryColorPressed: string;
};

const props = withDefaults(
  defineProps<{
    section?: string;
    active?: string;
    searchable?: boolean;
    variant?: "page" | "window" | "modal";
  }>(),
  {
    searchable: true,
    variant: "page",
  },
);

const emit = defineEmits<{
  (e: "update:active", key: string): void;
}>();

const { t } = useI18n();
const setting = settingStore();
const { sections } = useSettingsSections();
const {
  themeType,
  themeData,
  language,
  fps,
  flowSpeed,
  renderScale,
  albumImageUrl,
  dynamicFlowSpeed,
  dynamicFlowSpeedScale,
  blurAmount,
  contrastAmount,
  springParams,
} = storeToRefs(setting);

const resolveSectionKey = (key?: string) => (key ? (SETTINGS_SECTION_ALIASES[key] ?? key) : key);
const activeKey = ref(resolveSectionKey(props.active) ?? "appearance");
const resolvedSection = computed(() => resolveSectionKey(props.section));
const themeColorList = computed(() =>
  Object.values(themeColorData as Record<string, ThemeColorItem>),
);

const showThemeCustom = ref(false);
const showEploryModal = ref(false);
const showBlurModal = ref(false);
const customColorData = ref({
  primaryColor: "",
  primaryColorHover: "",
  primaryColorSuppl: "",
  primaryColorPressed: "",
});

watch(
  () => props.active,
  (value) => {
    const next = resolveSectionKey(value);
    if (next) activeKey.value = next;
  },
);

const handleActiveUpdate = (key: string) => {
  activeKey.value = key;
  emit("update:active", key);
};

const openThemeCustom = () => {
  showThemeCustom.value = true;
  customColorData.value = {
    primaryColor: themeData.value.primaryColor,
    primaryColorHover: themeData.value.primaryColorHover,
    primaryColorSuppl: themeData.value.primaryColorSuppl,
    primaryColorPressed: themeData.value.primaryColorPressed,
  };
};

const setThemeCustom = () => {
  themeType.value = "custom";
  themeData.value = {
    label: "custom",
    name: t("general.name.customTheme"),
    ...customColorData.value,
  };
  showThemeCustom.value = false;
};

const changeThemeColor = (data: ThemeColorItem | null, reset = false) => {
  if (reset) {
    $dialog.warning({
      class: "s-dialog",
      title: t("general.name.restore"),
      content: t("setting.themeTypeDialog"),
      positiveText: t("general.name.restore"),
      negativeText: t("general.dialog.cancel"),
      onPositiveClick: () => {
        $message.success(t("other.cleanAll"));
        themeType.value = "red";
      },
    });
    return;
  }

  if (!data) return;
  $message.success(t("setting.themeChange", { name: data.name }));
  themeType.value = data.label;
};

const resetApp = () => {
  const cleanAll = () => {
    $message?.success(t("other.cleanAll"));
    localStorage.clear();
    window.location.href = "/";
  };

  $dialog.warning({
    class: "s-dialog",
    title: t("setting.resetApp"),
    content: t("setting.resetAppWarning"),
    positiveText: t("setting.resetApp"),
    negativeText: t("general.dialog.cancel"),
    onPositiveClick: () => {
      if (typeof $cleanAll !== "undefined" && $cleanAll) {
        $cleanAll();
      } else {
        cleanAll();
      }
    },
  });
};

const handleAction = (key: string) => {
  if (key === "openEploryConfig") showEploryModal.value = true;
  else if (key === "openBlurConfig") showBlurModal.value = true;
  else if (key === "resetApp") resetApp();
};
</script>

<style lang="scss" scoped>
.settings-workspace {
  width: 100%;
  height: 100%;
  min-height: 0;
  overflow: hidden;

  &.is-modal {
    overflow: visible;
    color: #fff;

    :deep(.settings-search .n-input-wrapper),
    :deep(.set-item) {
      color: #fff;
      background-color: rgb(255 255 255 / 0.18);
      border-color: transparent;
    }

    :deep(.settings-nav-item) {
      color: rgb(255 255 255 / 0.76);

      &:hover,
      &.active {
        color: #fff;
        background-color: rgb(255 255 255 / 0.16);
      }
    }

    :deep(.n-base-selection),
    :deep(.n-input),
    :deep(.n-input-number) {
      --n-color: rgb(255 255 255 / 0.18) !important;
      --n-color-focus: rgb(255 255 255 / 0.22) !important;
      --n-text-color: #fff !important;
      --n-border: 1px solid rgb(255 255 255 / 0.14) !important;
      --n-border-hover: 1px solid rgb(255 255 255 / 0.26) !important;
      --n-border-focus: 1px solid rgb(255 255 255 / 0.34) !important;
    }

    :deep(.n-switch.n-switch--active .n-switch__rail) {
      background-color: rgb(255 255 255 / 0.48);
    }
  }
}

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
      opacity: 0.68;
    }
  }
}

.color-select {
  width: 100%;
  margin-top: 14px;

  .color-item {
    position: relative;
    width: 100%;
    min-height: 58px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 8px;
    background-color: var(--color, var(--main-color));
    cursor: pointer;
    overflow: hidden;
    transition:
      transform var(--duration-150) var(--ease-out),
      box-shadow var(--duration-150) var(--ease-out);

    &::before {
      content: "";
      position: absolute;
      inset: 4px;
      border: 2px solid rgb(255 255 255 / 0.72);
      border-radius: 6px;
      opacity: 0;
      transition: opacity var(--duration-150) var(--ease-out);
    }

    &.check::before {
      opacity: 1;
    }

    &:active {
      transform: scale(0.98);
    }

    span {
      position: relative;
      color: #fff;
      font-size: 13px;
      font-weight: 600;
      text-shadow: 0 1px 8px rgb(0 0 0 / 0.28);
    }
  }
}

.spring-panel {
  width: 100%;
  margin-top: 8px;

  .spring-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 10px;
  }
}

.settings-form {
  margin-top: 12px;

  :deep(.n-form-item-feedback-wrapper) {
    min-height: 0;
  }
}

@media (max-width: 640px) {
  .custom-heading {
    flex-direction: column;
  }

  .spring-panel {
    .spring-grid {
      grid-template-columns: 1fr;
    }
  }
}
</style>
