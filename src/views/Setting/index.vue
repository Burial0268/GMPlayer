<template>
  <div class="setting">
    <header class="setting-header">
      <div>
        <div class="title">{{ t("nav.avatar.setting") }}</div>
        <div class="subtitle">{{ t("setting.settingsSubtitle") }}</div>
      </div>
    </header>

    <main class="content">
      <SettingsWorkspace
        :active="activeSection"
        variant="page"
        @update:active="handleActiveUpdate"
      />
    </main>
  </div>
</template>

<script setup lang="ts">
import { useRoute, useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import SettingsWorkspace from "@/components/Settings/SettingsWorkspace.vue";
import { SETTINGS_SECTION_ALIASES } from "@/components/Settings/useSettingsSections";

declare const $setSiteTitle: (title: string) => void;
declare const $scrollToTop: () => void;

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
  if (resolveSectionKey(route.params.section) === key) return;
  router.replace({ name: "setting", params: { section: key } });
};

onMounted(() => {
  $setSiteTitle(t("nav.avatar.setting"));
  if (typeof $scrollToTop !== "undefined") $scrollToTop();
});
</script>

<style lang="scss" scoped>
.setting {
  height: 100%;
  max-height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
  overflow: hidden;

  .setting-header {
    flex: 0 0 auto;
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 16px;
    margin-top: 8px;
    margin-bottom: 8px;
  }

  .title {
    font-size: clamp(24px, 3vw, 32px);
    line-height: 1.1;
    font-weight: 700;
  }

  .subtitle {
    margin-top: 6px;
    font-size: 13px;
    opacity: 0.64;
  }

  .content {
    flex: 1 1 auto;
    min-height: 0;
    padding-bottom: 4px;
    box-sizing: border-box;
    overflow: hidden;
  }
}

@media (max-width: 768px) {
  .setting {
    height: auto;
    max-height: none;
    overflow: visible;

    .setting-header {
      margin-top: 12px;
    }

    .content {
      min-height: 72vh;
      overflow: visible;
    }
  }
}
</style>
