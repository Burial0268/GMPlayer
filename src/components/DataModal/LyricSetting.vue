<template>
  <GlassSheet v-model:show="lyricSettingModal" :z-index="10000" :title="t('setting.sectionLyrics')">
    <SettingsWorkspace section="lyrics" variant="modal" />
    <n-space justify="center">
      <n-button class="more" size="large" strong secondary round @click="openFullSettings">
        {{ t("setting.moreSettings") }}
      </n-button>
    </n-space>
  </GlassSheet>
</template>

<script setup lang="ts">
import GlassSheet from "@/components/Common/GlassSheet.vue";
import { useLayerNavigation } from "@/utils/navigation";
import { useI18n } from "vue-i18n";
import SettingsWorkspace from "@/components/Settings/SettingsWorkspace.vue";

const navigation = useLayerNavigation();
const { t } = useI18n();
const lyricSettingModal = ref(false);

const openLyricSetting = () => {
  lyricSettingModal.value = true;
};

const openFullSettings = () => {
  lyricSettingModal.value = false;
  navigation.openPage("/setting/lyrics");
};

defineExpose({
  openLyricSetting,
});
</script>

<style lang="scss" scoped>
.more {
  margin: 14px 0 4px;
}
</style>
