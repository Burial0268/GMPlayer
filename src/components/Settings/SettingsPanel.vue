<template>
  <!-- 单分区模式：仅渲染某个分区的项（用于播放器快捷设置） -->
  <div v-if="section" class="settings-single">
    <div v-if="searchable" class="settings-search">
      <n-input
        v-model:value="searchQuery"
        clearable
        size="small"
        :placeholder="resolve('setting.searchSettings')"
      >
        <template #prefix>
          <n-icon class="search-icon" :component="SearchRound" />
        </template>
      </n-input>
    </div>
    <div class="settings-item-list">
      <div v-for="item in singleItems" :key="item.key" class="settings-item-row">
        <SettingsItem :item="item" @action="(k) => emit('action', k)">
          <template v-for="(_, name) in $slots" #[name]="slotProps">
            <slot :name="name" v-bind="slotProps ?? {}" />
          </template>
        </SettingsItem>
      </div>
    </div>
    <div v-if="!singleItems.length" class="settings-empty">
      <n-icon class="empty-icon" :component="InboxRound" />
      <span>{{ resolve("setting.noSettingsMatched") }}</span>
    </div>
  </div>

  <!-- 完整面板：左侧分区导航 + 右侧内容 -->
  <div v-else class="settings-panel">
    <aside class="settings-sidebar">
      <div v-if="searchable" class="settings-search">
        <n-input
          v-model:value="searchQuery"
          clearable
          size="small"
          :placeholder="resolve('setting.searchSettings')"
        >
          <template #prefix>
            <n-icon class="search-icon" :component="SearchRound" />
          </template>
        </n-input>
      </div>
      <nav class="settings-nav">
        <button
          v-for="s in visibleSections"
          :key="s.key"
          type="button"
          :class="['settings-nav-item', { active: s.key === activeKey }]"
          @click="setActive(s.key)"
        >
          <span v-if="s.key === activeKey" class="nav-indicator" aria-hidden="true" />
          <span class="nav-icon">
            <n-icon v-if="s.icon" :component="s.icon" />
          </span>
          <span class="nav-label">{{ resolve(s.label) }}</span>
          <span class="nav-count">{{ s.count }}</span>
        </button>
      </nav>
    </aside>

    <section class="settings-main" :data-direction="direction">
      <header v-if="activeSection" class="settings-head">
        <Transition name="settings-head-fade" mode="out-in">
          <div :key="activeSection.key" class="settings-head-inner">
            <span class="head-icon">
              <n-icon v-if="activeSection.icon" :component="activeSection.icon" />
            </span>
            <div class="head-text">
              <span class="head-title">{{ resolve(activeSection.label) }}</span>
              <span class="head-count">{{ headCountLabel }}</span>
            </div>
          </div>
        </Transition>
      </header>

      <div class="settings-content">
        <Transition name="settings-content-fade" mode="out-in">
          <n-virtual-list
            v-if="activeItems.length"
            :key="contentKey"
            class="settings-virtual-list"
            :items="activeItems"
            :item-size="SETTINGS_ITEM_ESTIMATED_SIZE"
            :item-resizable="true"
            key-field="key"
            :show-scrollbar="false"
          >
            <template #default="{ item }">
              <div class="settings-virtual-row">
                <SettingsItem :item="item" @action="(k) => emit('action', k)">
                  <template v-for="(_, name) in $slots" #[name]="slotProps">
                    <slot :name="name" v-bind="slotProps ?? {}" />
                  </template>
                </SettingsItem>
              </div>
            </template>
          </n-virtual-list>
          <div v-else :key="`${contentKey}-empty`" class="settings-content-empty">
            <div class="settings-empty">
              <n-icon class="empty-icon" :component="InboxRound" />
              <span>{{ resolve("setting.noSettingsMatched") }}</span>
            </div>
          </div>
        </Transition>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { NVirtualList } from "naive-ui";
import { useI18n } from "vue-i18n";
import { SearchRound, InboxRound } from "@vicons/material";
import SettingsItem from "./SettingsItem.vue";
import type { SettingsItem as SettingsItemSchema, SettingsSection } from "./types";

type VisibleSection = SettingsSection & {
  count: number;
  visibleItems: SettingsItemSchema[];
};

const SETTINGS_ITEM_ESTIMATED_SIZE = 84;

const props = withDefaults(
  defineProps<{
    sections: SettingsSection[];
    /** 单分区模式：仅渲染该 key 的分区 */
    section?: string;
    /** 当前激活分区（可 v-model:active） */
    active?: string;
    searchable?: boolean;
  }>(),
  {
    searchable: true,
  },
);
const emit = defineEmits<{
  (e: "action", key: string): void;
  (e: "update:active", key: string): void;
}>();

const { t, te } = useI18n();
const resolve = (x?: string) => (x ? (te(x) ? t(x) : x) : "");

const activeKey = ref(props.active || props.sections[0]?.key);
const searchQuery = ref("");
/** 内容切换方向：1 = 向下（更靠后分区），-1 = 向上 */
const direction = ref(1);

watch(
  () => props.active,
  (v) => {
    if (v) activeKey.value = v;
  },
);
watch(activeKey, (v) => {
  if (v) emit("update:active", v);
});

const normalizedSearch = computed(() => searchQuery.value.trim().toLocaleLowerCase());

const matchesSearch = (item: SettingsItemSchema, section?: SettingsSection) => {
  const query = normalizedSearch.value;
  if (!query) return true;
  return [item.key, item.label, item.tip, item.buttonText, section?.label, section?.searchText]
    .map((text) => resolve(text).toLocaleLowerCase())
    .some((text) => text.includes(query));
};

const sectionViews = computed<VisibleSection[]>(() =>
  props.sections
    .map((sec) => {
      const visibleItems = sec.items
        .filter((item) => (item.show ? item.show() : true))
        .filter((item) => matchesSearch(item, sec));

      return {
        ...sec,
        count: visibleItems.length,
        visibleItems,
      };
    })
    .filter((sec) => sec.count > 0 || !normalizedSearch.value),
);

const visibleSections = computed(() => sectionViews.value);
const activeSection = computed(
  () => sectionViews.value.find((s) => s.key === activeKey.value) ?? null,
);
const activeItems = computed(() => activeSection.value?.visibleItems ?? []);
const singleItems = computed(
  () => sectionViews.value.find((s) => s.key === props.section)?.visibleItems ?? [],
);
const contentKey = computed(() => activeKey.value ?? "settings");
const headCountLabel = computed(() => {
  const count = activeItems.value.length;
  const key = count === 1 ? "setting.itemCountSingle" : "setting.itemCount";
  return te(key) ? t(key, { count }) : `${count}`;
});

const setActive = (key: string) => {
  if (key === activeKey.value) return;
  const next = visibleSections.value.findIndex((s) => s.key === key);
  const cur = visibleSections.value.findIndex((s) => s.key === activeKey.value);
  direction.value = next >= cur ? 1 : -1;
  activeKey.value = key;
};

watch(
  visibleSections,
  (sections) => {
    if (!sections.length) return;
    if (!activeKey.value || !sections.some((section) => section.key === activeKey.value)) {
      activeKey.value = sections[0].key;
    }
  },
  { immediate: true },
);
</script>

<style lang="scss" scoped>
.settings-panel {
  --settings-sidebar-width: clamp(168px, 22%, 224px);
  --settings-nav-scrollbar-thumb: color-mix(in srgb, var(--n-text-color) 18%, transparent);
  --settings-nav-scrollbar-thumb-hover: color-mix(in srgb, var(--main-color) 36%, transparent);

  display: grid;
  grid-template-columns: var(--settings-sidebar-width) minmax(0, 1fr);
  gap: clamp(12px, 1.4vw, 20px);
  width: 100%;
  height: 100%;
  min-height: 0;
  overflow: hidden;

  .settings-sidebar {
    width: auto;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 8px;
    box-sizing: border-box;
    border-radius: var(--radius-panel);
    background: color-mix(in srgb, var(--n-text-color) 3.5%, transparent);
    border: 1px solid color-mix(in srgb, var(--n-border-color) 50%, transparent);
    overflow: hidden;
  }

  .settings-nav {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 2px;
    min-height: 0;
    box-sizing: border-box;
    overflow-x: hidden;
    overflow-y: auto;
    overscroll-behavior: contain;
    scrollbar-gutter: stable;
    scrollbar-width: thin;
    scrollbar-color: transparent transparent;
    transition: scrollbar-color var(--duration-150) var(--ease-out);

    &::-webkit-scrollbar {
      width: 8px;
    }

    &::-webkit-scrollbar-track {
      background: transparent;
    }

    &::-webkit-scrollbar-thumb {
      min-height: 28px;
      border: 2px solid transparent;
      border-radius: 999px;
      background-clip: content-box;
      background-color: transparent;
    }

    &:hover,
    &:focus-within {
      scrollbar-color: var(--settings-nav-scrollbar-thumb) transparent;
    }

    &:hover::-webkit-scrollbar-thumb,
    &:focus-within::-webkit-scrollbar-thumb {
      background-color: var(--settings-nav-scrollbar-thumb);
    }

    .settings-nav-item {
      position: relative;
      display: flex;
      align-items: center;
      gap: 10px;
      width: 100%;
      min-height: 40px;
      padding: 8px 12px;
      border: none;
      border-radius: var(--radius-md);
      background: transparent;
      color: var(--n-text-color-2, inherit);
      font-size: 14px;
      text-align: left;
      cursor: pointer;
      transition:
        color var(--duration-200) var(--ease-out),
        background-color var(--duration-200) var(--ease-out);

      &:hover {
        color: var(--n-text-color);
        background-color: color-mix(in srgb, var(--n-text-color) 6%, transparent);
      }

      &.active {
        color: var(--main-color);

        &:hover {
          background-color: transparent;
        }

        .nav-icon {
          color: var(--main-color);
        }

        .nav-count {
          color: var(--main-color);
          background-color: color-mix(in srgb, var(--main-color) 18%, transparent);
          opacity: 1;
        }
      }

      .nav-indicator {
        position: absolute;
        inset: 0;
        z-index: 0;
        border-radius: var(--radius-md);
        background-color: color-mix(in srgb, var(--main-color) 14%, transparent);
        box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--main-color) 22%, transparent);
        pointer-events: none;
      }

      .nav-icon {
        position: relative;
        z-index: 1;
        flex: 0 0 auto;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        font-size: 19px;
        color: var(--n-text-color-3, currentColor);
        transition: color var(--duration-200) var(--ease-out);
      }

      .nav-label {
        position: relative;
        z-index: 1;
        flex: 1 1 auto;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-weight: 500;
      }

      .nav-count {
        position: relative;
        z-index: 1;
        flex: 0 0 auto;
        min-width: 20px;
        height: 19px;
        padding: 0 6px;
        border-radius: 999px;
        background-color: color-mix(in srgb, var(--n-text-color) 8%, transparent);
        font-size: 11px;
        font-variant-numeric: tabular-nums;
        line-height: 19px;
        text-align: center;
        opacity: 0.7;
        transition:
          color var(--duration-200) var(--ease-out),
          background-color var(--duration-200) var(--ease-out);
      }
    }
  }

  .settings-main {
    min-width: 0;
    min-height: 0;
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .settings-head {
    flex: 0 0 auto;
    padding: 2px 2px 12px;
    margin-bottom: 4px;
    border-bottom: 1px solid color-mix(in srgb, var(--n-border-color) 55%, transparent);

    .settings-head-inner {
      display: flex;
      align-items: center;
      gap: 12px;
    }

    .head-icon {
      flex: 0 0 auto;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      width: 38px;
      height: 38px;
      border-radius: var(--radius-md);
      font-size: 22px;
      color: var(--main-color);
      background-color: color-mix(in srgb, var(--main-color) 12%, transparent);
    }

    .head-text {
      display: flex;
      flex-direction: column;
      gap: 1px;
      min-width: 0;
    }

    .head-title {
      font-size: 17px;
      font-weight: 650;
      line-height: 1.25;
    }

    .head-count {
      font-size: 12px;
      opacity: 0.6;
    }
  }

  .settings-content {
    position: relative;
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
}

.settings-virtual-list {
  width: 100%;
  height: 100%;
}

.settings-virtual-row {
  width: 100%;
  padding: 4px;
  box-sizing: border-box;
}

.settings-content-empty {
  width: 100%;
  height: 100%;
  overflow: hidden;
}

.settings-item-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.settings-item-row {
  width: 100%;
}

.settings-single {
  height: auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  overflow: visible;

  .settings-search {
    margin-bottom: 2px;
  }
}

.settings-search {
  flex: 0 0 auto;

  .search-icon {
    font-size: 16px;
    opacity: 0.6;
  }

  :deep(.n-input) {
    --n-border-radius: var(--radius-md);
  }
}

.settings-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  margin: 44px 0;
  color: var(--n-text-color-3);
  font-size: 13px;

  .empty-icon {
    font-size: 40px;
    opacity: 0.4;
  }
}

.settings-head-fade-enter-active,
.settings-head-fade-leave-active,
.settings-content-fade-enter-active,
.settings-content-fade-leave-active {
  transition:
    opacity 0.16s var(--ease-out),
    transform 0.16s var(--ease-out);
}

.settings-head-fade-enter-from,
.settings-content-fade-enter-from {
  opacity: 0;
  transform: translateY(7px);
}

.settings-main[data-direction="-1"] {
  .settings-head-fade-enter-from,
  .settings-content-fade-enter-from {
    transform: translateY(-7px);
  }
}

.settings-head-fade-leave-to,
.settings-content-fade-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

.settings-main[data-direction="-1"] {
  .settings-head-fade-leave-to,
  .settings-content-fade-leave-to {
    transform: translateY(4px);
  }
}

@media (max-width: 640px) {
  .settings-panel {
    grid-template-columns: 1fr;
    grid-template-rows: auto minmax(0, 1fr);
    gap: 12px;

    .settings-sidebar {
      width: 100%;
      flex: 0 0 auto;
      padding: 8px;
      overflow: visible;
    }

    .settings-nav {
      flex: 0 0 auto;
      flex-direction: row;
      gap: 6px;
      overflow-x: auto;
      overflow-y: hidden;
      padding: 2px;

      .settings-nav-item {
        width: auto;
        flex: 0 0 auto;

        .nav-label {
          white-space: nowrap;
        }
      }
    }

    .settings-head {
      display: none;
    }
  }
}
</style>
