<template>
  <!-- 窄屏：先看到分区列表，点进去是一层新页面（有返回按钮和侧滑返回）。 -->
  <div v-if="isMobile" class="setting">
    <template v-if="!detail">
      <header class="setting-header">
        <div>
          <h1 class="title" data-navigation-title="page">{{ t("nav.avatar.setting") }}</h1>
          <div class="subtitle">{{ t("setting.settingsSubtitle") }}</div>
        </div>
      </header>
      <ul class="section-list">
        <li v-for="s in listSections" :key="s.key">
          <button type="button" class="section-row" @click="openSection(s.key)">
            <n-icon v-if="s.icon" class="row-icon" :size="22" :component="s.icon" />
            <span class="row-label">{{ resolve(s.label) }}</span>
            <n-icon class="row-chevron" :size="18" :component="Right" />
          </button>
        </li>
      </ul>
    </template>
    <template v-else>
      <header class="setting-header">
        <h1 class="title" data-navigation-title="page">{{ detailTitle }}</h1>
      </header>
      <main class="content">
        <SettingsWorkspace :section="detail" variant="page" :searchable="false" />
      </main>
    </template>
  </div>

  <div v-else class="setting">
    <header class="setting-header">
      <div>
        <h1 class="title">{{ t("nav.avatar.setting") }}</h1>
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
import {
  SETTINGS_SECTION_ALIASES,
  useSettingsSections,
} from "@/components/Settings/useSettingsSections";
import { Right } from "@icon-park/vue-next";
import { NIcon } from "naive-ui";
import { useResponsiveLayout } from "@/composables/useResponsiveLayout";
import { useLayerNavigation } from "@/utils/navigation";

declare const $setSiteTitle: (title: string) => void;
declare const $scrollToTop: () => void;

const { t, te } = useI18n();
const resolve = (x?: string) => (x ? (te(x) ? t(x) : x) : "");
const route = useRoute();
const router = useRouter();

const resolveSectionKey = (key?: string | string[]) => {
  const value = Array.isArray(key) ? key[0] : key;
  if (!value) return "appearance";
  return SETTINGS_SECTION_ALIASES[value] ?? value;
};

const { isMobile } = useResponsiveLayout();
const navigation = useLayerNavigation();
const { sections } = useSettingsSections();
// The query id is what gives each section its own page-cache key (and so its own layer).
const detail = computed(() => {
  const id = route.query.id;
  return typeof id === "string" && id ? resolveSectionKey(id) : null;
});
const listSections = computed(() =>
  sections.value.filter((sec) => sec.items.some((item) => !item.show || item.show())),
);
const detailTitle = computed(() => {
  const sec = sections.value.find((x) => x.key === detail.value);
  return sec ? resolve(sec.label) : t("nav.avatar.setting");
});
const openSection = (key: string) =>
  navigation.openPage({ name: "setting", params: { section: key }, query: { id: key } });

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
    margin: 0;
    font-size: var(--font-size-large-title);
    line-height: var(--line-height-heading);
    font-weight: 700;
  }

  .subtitle {
    margin-top: 6px;
    font-size: var(--font-size-footnote);
    line-height: var(--line-height-body);
    color: var(--text-secondary);
  }

  .content {
    flex: 1 1 auto;
    min-height: 0;
    padding-bottom: 4px;
    box-sizing: border-box;
    overflow: hidden;
  }
}

.section-list {
  list-style: none;
  margin: 12px 0 0;
  padding: 0;
  border-radius: var(--radius-panel);
  background: var(--surface-elevated, transparent);
  overflow: hidden;

  .section-row {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 52px;
    padding: 0 16px;
    border: 0;
    border-bottom: 1px solid var(--border-color);
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;

    &:active {
      background: var(--active-overlay);
    }
  }

  li:last-child .section-row {
    border-bottom: 0;
  }

  .row-icon {
    color: var(--main-color);
  }

  .row-label {
    flex: 1;
    min-width: 0;
  }

  .row-chevron {
    opacity: 0.4;
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
