<template>
  <!-- 单分区模式：仅渲染某个分区的项（用于播放器快捷设置） -->
  <div v-if="section" class="settings-single">
    <n-input
      v-if="searchable"
      v-model:value="searchQuery"
      class="settings-search"
      clearable
      size="small"
      :placeholder="resolve('setting.searchSettings')"
    />
    <Motion
      v-for="(item, index) in singleItems"
      :key="item.key"
      class="settings-item-motion"
      :initial="{ opacity: 0, y: 8 }"
      :animate="{ opacity: 1, y: 0 }"
      :transition="itemMotionTransition(index)"
    >
      <SettingsItem :item="item" @action="(k) => emit('action', k)">
        <template v-for="(_, name) in $slots" #[name]="slotProps">
          <slot :name="name" v-bind="slotProps ?? {}" />
        </template>
      </SettingsItem>
    </Motion>
    <n-empty
      v-if="!singleItems.length"
      class="settings-empty"
      :description="resolve('setting.noSettingsMatched')"
    />
  </div>

  <!-- 完整面板：左侧分区导航 + 右侧内容 -->
  <div v-else class="settings-panel">
    <aside class="settings-sidebar">
      <n-input
        v-if="searchable"
        v-model:value="searchQuery"
        class="settings-search"
        clearable
        size="small"
        :placeholder="resolve('setting.searchSettings')"
      />
      <nav class="settings-nav">
        <Motion
          v-for="s in visibleSections"
          :key="s.key"
          tag="button"
          :class="['settings-nav-item', { active: s.key === activeKey }]"
          type="button"
          :animate="{ x: s.key === activeKey ? 3 : 0 }"
          :transition="navMotionTransition"
          @click="activeKey = s.key"
        >
          <span class="nav-label">{{ resolve(s.label) }}</span>
          <span class="nav-count">{{ s.count }}</span>
        </Motion>
      </nav>
    </aside>
    <n-scrollbar class="settings-content">
      <Motion
        :key="activeKey"
        class="settings-content-inner"
        :initial="{ opacity: 0, y: 10 }"
        :animate="{ opacity: 1, y: 0 }"
        :transition="contentMotionTransition"
      >
        <Motion
          v-for="(item, index) in activeItems"
          :key="item.key"
          class="settings-item-motion"
          :initial="{ opacity: 0, y: 8 }"
          :animate="{ opacity: 1, y: 0 }"
          :transition="itemMotionTransition(index)"
        >
          <SettingsItem :item="item" @action="(k) => emit('action', k)">
            <template v-for="(_, name) in $slots" #[name]="slotProps">
              <slot :name="name" v-bind="slotProps ?? {}" />
            </template>
          </SettingsItem>
        </Motion>
        <n-empty
          v-if="!activeItems.length"
          class="settings-empty"
          :description="resolve('setting.noSettingsMatched')"
        />
      </Motion>
    </n-scrollbar>
  </div>
</template>

<script setup lang="ts">
import { Motion } from "motion-v";
import { useI18n } from "vue-i18n";
import SettingsItem from "./SettingsItem.vue";
import type { SettingsSection } from "./types";

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
const navMotionTransition = {
  type: "spring",
  stiffness: 560,
  damping: 36,
  mass: 0.7,
} as const;
const contentMotionTransition = {
  type: "tween",
  duration: 0.18,
  ease: [0.22, 1, 0.36, 1],
} as const;
const itemMotionTransition = (index: number) => ({
  type: "tween",
  duration: 0.18,
  delay: Math.min(index * 0.015, 0.09),
  ease: [0.22, 1, 0.36, 1],
});

const activeKey = ref(props.active || props.sections[0]?.key);
const searchQuery = ref("");
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

const matchesSearch = (item: SettingsSection["items"][number], section?: SettingsSection) => {
  const query = normalizedSearch.value;
  if (!query) return true;
  return [item.key, item.label, item.tip, item.buttonText, section?.label, section?.searchText]
    .map((text) => resolve(text).toLocaleLowerCase())
    .some((text) => text.includes(query));
};

const visibleItems = (key: string) => {
  const sec = props.sections.find((s) => s.key === key);
  return (sec?.items ?? [])
    .filter((item) => (item.show ? item.show() : true))
    .filter((item) => matchesSearch(item, sec));
};

const visibleSections = computed(() =>
  props.sections
    .map((sec) => ({ ...sec, count: visibleItems(sec.key).length }))
    .filter((sec) => sec.count > 0 || !normalizedSearch.value),
);
const activeItems = computed(() => (activeKey.value ? visibleItems(activeKey.value) : []));
const singleItems = computed(() => (props.section ? visibleItems(props.section) : []));

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
  --settings-sidebar-width: clamp(168px, 21%, 236px);
  --settings-nav-scrollbar-space: 10px;
  --settings-nav-scrollbar-thumb: color-mix(in srgb, var(--n-text-color) 18%, transparent);
  --settings-nav-scrollbar-thumb-hover: color-mix(in srgb, var(--main-color) 36%, transparent);

  display: grid;
  grid-template-columns: var(--settings-sidebar-width) minmax(0, 1fr);
  gap: clamp(12px, 1.8%, 18px);
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
    overflow: hidden;
  }

  .settings-nav {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 4px var(--settings-nav-scrollbar-space) 4px 4px;
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
      height: 8px;
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

    &::-webkit-scrollbar-thumb:hover {
      border-radius: 999px;
      background-color: var(--settings-nav-scrollbar-thumb-hover);
    }

    .settings-nav-item {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
      width: 100%;
      min-height: 34px;
      padding: 8px 12px;
      border: none;
      border-radius: var(--radius-md);
      background: transparent;
      color: var(--n-text-color-2, inherit);
      font-size: 14px;
      text-align: left;
      cursor: pointer;
      transition:
        background-color var(--duration-150) var(--ease-out),
        color var(--duration-150) var(--ease-out);

      &:hover {
        background-color: color-mix(in srgb, var(--n-text-color) 6%, transparent);
      }

      &.active {
        font-weight: 600;
        color: var(--main-color);
        background-color: color-mix(in srgb, var(--main-color) 12%, transparent);
      }

      .nav-label {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }

      .nav-count {
        flex: 0 0 auto;
        min-width: 18px;
        height: 18px;
        padding: 0 5px;
        border-radius: 9px;
        background-color: color-mix(in srgb, var(--n-text-color) 8%, transparent);
        font-size: 11px;
        line-height: 18px;
        text-align: center;
        opacity: 0.72;
      }
    }
  }

  .settings-content {
    min-width: 0;
    min-height: 0;
    height: 100%;
    overflow: hidden;

    .settings-content-inner {
      display: flex;
      flex-direction: column;
      padding-right: 6px;
      padding-bottom: 8px;
    }
  }
}

.settings-item-motion {
  width: 100%;
}

.settings-single {
  height: auto;
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: visible;
}

.settings-search {
  flex: 0 0 auto;
}

.settings-empty {
  margin: 36px 0;
}

@media (max-width: 560px) {
  .settings-panel {
    grid-template-columns: 1fr;
    grid-template-rows: auto minmax(0, 1fr);
    gap: 10px;

    .settings-sidebar {
      width: 100%;
      flex: 0 0 auto;
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
        padding: 7px 14px;
      }
    }
  }
}
</style>
