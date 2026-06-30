<template>
  <Provider>
    <div class="slave-settings">
      <TitleBar
        label="settings"
        variant="window"
        :title="t('nav.avatar.setting')"
        :icon="SettingsRound"
        draggable
        show-on-mac
      />

      <main class="content">
        <SettingsWorkspace
          :active="activeSection"
          variant="window"
          @update:active="handleActiveUpdate"
        />
      </main>
    </div>
  </Provider>
</template>

<script setup lang="ts">
import { useRoute, useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { SettingsRound } from "@vicons/material";
import Provider from "@/components/Provider/index.vue";
import SettingsWorkspace from "@/components/Settings/SettingsWorkspace.vue";
import TitleBar from "@/components/TitleBar/index.vue";
import { SETTINGS_SECTION_ALIASES } from "@/components/Settings/useSettingsSections";

const { t } = useI18n();
const route = useRoute();
const router = useRouter();

const resolveSectionKey = (key?: string | string[]) => {
  const value = Array.isArray(key) ? key[0] : key;
  if (!value) return "appearance";
  return SETTINGS_SECTION_ALIASES[value] ?? value;
};

const activeSection = ref(resolveSectionKey(route.params.section));

watch(
  () => route.params.section,
  (section) => {
    activeSection.value = resolveSectionKey(section);
  },
);

const handleActiveUpdate = (key: string) => {
  activeSection.value = key;
  if (route.params.section === key) return;
  router.replace({ path: `/settings/${key}` });
};

onMounted(() => {
  document.title = t("nav.avatar.setting");
});
</script>

<style lang="scss" scoped>
.slave-settings {
  position: relative;
  width: 100vw;
  height: 100vh;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
  background-color: var(--app-shell-bg, var(--layout-bg, #fff));
  color: var(--n-text-color);
  overflow: hidden;

  .content {
    flex: 1 1 auto;
    min-height: 0;
    padding: 16px 18px 16px;
    box-sizing: border-box;
    overflow: hidden;
  }
}
</style>
