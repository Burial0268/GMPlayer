<template>
  <Provider>
    <div class="slave-settings">
      <div class="drag-region" data-tauri-drag-region />
      <header class="setting-header">
        <div>
          <div class="title">{{ t("nav.avatar.setting") }}</div>
          <div class="subtitle">{{ t("setting.settingsSubtitle") }}</div>
        </div>
      </header>
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
import Provider from "@/components/Provider/index.vue";
import SettingsWorkspace from "@/components/Settings/SettingsWorkspace.vue";
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
  padding: 34px 24px 22px;
  box-sizing: border-box;
  background-color: var(--app-shell-bg, var(--layout-bg, #fff));
  color: var(--n-text-color);
  overflow: hidden;

  .drag-region {
    position: fixed;
    top: 0;
    left: 0;
    right: 128px;
    height: 34px;
    z-index: 1;
  }

  .setting-header {
    flex: 0 0 auto;
    margin-bottom: 14px;
  }

  .title {
    font-size: 30px;
    line-height: 1.1;
    font-weight: 700;
  }

  .subtitle {
    margin-top: 5px;
    font-size: 13px;
    opacity: 0.64;
  }

  .content {
    flex: 1 1 auto;
    min-height: 0;
    overflow: hidden;
  }
}
</style>
