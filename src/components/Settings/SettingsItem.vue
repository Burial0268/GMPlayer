<template>
  <n-card
    v-if="visible"
    class="set-item"
    :content-style="isColumn ? { flexDirection: 'column', alignItems: 'flex-start' } : undefined"
  >
    <!-- 自定义项：交给宿主插槽 -->
    <slot v-if="item.control === 'custom'" :name="item.slot" />

    <template v-else>
      <div class="name">
        <div v-if="item.dev" class="dev">
          {{ label }}
          <n-tag round :bordered="false" size="small" type="warning">{{ t("setting.dev") }}</n-tag>
        </div>
        <template v-else>{{ label }}</template>
        <span v-if="tip" class="tip">{{ tip }}</span>
      </div>

      <n-switch
        v-if="item.control === 'switch'"
        :value="value"
        :round="false"
        :disabled="disabled"
        @update:value="update"
      />
      <n-select
        v-else-if="item.control === 'select'"
        class="set"
        :value="value"
        :options="resolvedOptions"
        :disabled="disabled"
        @update:value="update"
      />
      <n-slider
        v-else-if="item.control === 'slider'"
        :value="value"
        :min="item.min"
        :max="item.max"
        :step="item.step"
        :marks="resolvedMarks"
        :tooltip="!item.marks"
        :disabled="disabled"
        @update:value="update"
      />
      <n-input-number
        v-else-if="item.control === 'number'"
        class="set"
        :value="value"
        :min="item.min"
        :max="item.max"
        :step="item.step"
        :disabled="disabled"
        @update:value="update"
      />
      <n-button
        v-else-if="item.control === 'button'"
        class="set"
        strong
        secondary
        :type="item.buttonType || 'default'"
        :disabled="disabled"
        @click="emit('action', item.key)"
      >
        {{ buttonText }}
      </n-button>
    </template>
  </n-card>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { settingStore } from "@/store";
import type { SettingsItem } from "./types";

const props = defineProps<{ item: SettingsItem }>();
const emit = defineEmits<{ (e: "action", key: string): void }>();

const { t, te } = useI18n();
const setting = settingStore() as Record<string, any>;

// i18n key 解析：能命中则翻译，否则按原始文本显示
const resolve = (x?: string) => (x ? (te(x) ? t(x) : x) : "");

const label = computed(() => resolve(props.item.label));
const tip = computed(() => resolve(props.item.tip));
const buttonText = computed(() => resolve(props.item.buttonText));
const isColumn = computed(() => props.item.control === "slider" || props.item.control === "custom");
const visible = computed(() => (props.item.show ? props.item.show() : true));
const disabled = computed(() => (props.item.disabled ? props.item.disabled() : false));

const value = computed(() => setting[props.item.key]);
const update = (v: unknown) => {
  setting[props.item.key] = v;
  props.item.onUpdate?.(v);
};

const resolvedOptions = computed(() => {
  const raw =
    typeof props.item.options === "function" ? props.item.options() : (props.item.options ?? []);
  return raw.map((o) => ({ label: resolve(o.label), value: o.value, disabled: o.disabled }));
});

const resolvedMarks = computed(() => {
  if (!props.item.marks) return undefined;
  return Object.fromEntries(
    Object.entries(props.item.marks).map(([key, value]) => [Number(key), resolve(value)]),
  );
});
</script>

<style lang="scss" scoped>
.set-item {
  width: 100%;
  border-radius: 8px;
  margin-bottom: 10px;
  border-color: color-mix(in srgb, var(--n-border-color) 72%, transparent);
  transition:
    border-color var(--duration-150) var(--ease-out),
    background-color var(--duration-150) var(--ease-out);

  &:hover {
    border-color: color-mix(in srgb, var(--main-color) 30%, var(--n-border-color));
  }

  :deep(.n-card__content) {
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    min-height: 42px;
    padding: 14px 16px;
    box-sizing: border-box;
  }

  .name {
    min-width: 0;
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding-right: 8px;
    font-size: 15px;
    line-height: 1.35;

    .dev {
      display: flex;
      flex-direction: row;
      align-items: center;
      flex-wrap: wrap;
      gap: 6px;
    }

    .tip {
      font-size: 12px;
      line-height: 1.45;
      opacity: 0.68;
    }
  }

  .set {
    flex: 0 0 auto;
    width: min(220px, 38vw);
  }

  :deep(.n-slider) {
    width: 100%;
    padding: 8px 0 2px;
  }

  :deep(.n-input-number) {
    width: min(220px, 38vw);
  }
}

@media (max-width: 640px) {
  .set-item {
    :deep(.n-card__content) {
      align-items: flex-start;
      flex-direction: column;
      gap: 12px;
    }

    .name {
      width: 100%;
      padding-right: 0;
    }

    .set,
    :deep(.n-input-number) {
      width: 100%;
    }
  }
}
</style>
