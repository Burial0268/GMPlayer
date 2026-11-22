<template>
  <div class="settings-storage">
    <div class="custom-heading">
      <div class="name">
        {{ t("setting.storage.title") }}
        <span class="tip">{{ t("setting.storage.tip") }}</span>
      </div>
      <n-button quaternary size="small" :loading="loading" @click="refresh">
        {{ t("setting.storage.refresh") }}
      </n-button>
    </div>

    <div class="control-row">
      <div class="row-name">
        {{ t("setting.storage.budget") }}
        <span>{{ t("setting.storage.budgetTip") }}</span>
      </div>
      <n-select
        class="compact-control"
        :value="setting.audioCacheBudget"
        :options="budgetOptions"
        @update:value="setBudget"
      />
    </div>

    <div v-for="row in rows" :key="row.kind" class="control-row">
      <div class="row-name">
        {{ t(row.label) }}
        <span>{{ t(row.tip) }}</span>
      </div>
      <n-space :size="12" align="center" :wrap="false">
        <n-text :depth="3">{{ usage ? formatBytes(usage[row.kind]) : "…" }}</n-text>
        <n-button
          strong
          secondary
          round
          size="small"
          :disabled="!usage || usage[row.kind] === 0"
          :loading="clearing === row.kind"
          @click="clear(row.kind)"
        >
          {{ t("setting.storage.clear") }}
        </n-button>
      </n-space>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { settingStore } from "@/store";
import {
  storageClear,
  storageUsage,
  type StorageKind,
  type StorageUsage,
} from "@/utils/tauri/storage";

declare const $message: any;

const { t } = useI18n();
const setting = settingStore();

const MB = 1024 * 1024;
const GB = 1024 * MB;

const rows: { kind: StorageKind; label: string; tip: string }[] = [
  { kind: "audio", label: "setting.storage.audio", tip: "setting.storage.audioTip" },
  { kind: "api", label: "setting.storage.api", tip: "setting.storage.apiTip" },
  {
    kind: "localStream",
    label: "setting.storage.localStream",
    tip: "setting.storage.localStreamTip",
  },
  { kind: "temp", label: "setting.storage.temp", tip: "setting.storage.tempTip" },
];

const budgetOptions = computed(() => [
  { label: t("setting.storage.off"), value: 0 },
  { label: "256 MB", value: 256 * MB },
  { label: "512 MB", value: 512 * MB },
  { label: "1 GB", value: GB },
  { label: "2 GB", value: 2 * GB },
  { label: "4 GB", value: 4 * GB },
]);

const usage = ref<StorageUsage | null>(null);
const loading = ref(false);
const clearing = ref<StorageKind | null>(null);

const formatBytes = (bytes: number): string =>
  bytes >= GB ? `${(bytes / GB).toFixed(2)} GB` : `${(bytes / MB).toFixed(1)} MB`;

const refresh = async () => {
  loading.value = true;
  try {
    usage.value = await storageUsage();
  } catch (error) {
    $message.error(String(error));
  } finally {
    loading.value = false;
  }
};

const clear = async (kind: StorageKind) => {
  clearing.value = kind;
  try {
    const freed = await storageClear(kind);
    $message.success(t("setting.storage.cleared", { size: formatBytes(freed) }));
  } catch (error) {
    $message.error(String(error));
  } finally {
    clearing.value = null;
    await refresh();
  }
};

// main.ts pushes the new budget to Rust, which evicts at once; show the result.
const setBudget = (value: number) => {
  setting.audioCacheBudget = value;
  setTimeout(refresh, 300);
};

onMounted(refresh);
</script>

<style lang="scss" scoped>
.settings-storage {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 14px;

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
        line-height: 1.45;
        opacity: 0.68;
      }
    }
  }

  .control-row {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;

    .row-name {
      min-width: 0;
      display: flex;
      flex-direction: column;
      gap: 3px;
      font-size: 14px;

      span {
        font-size: 12px;
        line-height: 1.45;
        opacity: 0.68;
      }
    }
  }

  .compact-control {
    width: 220px;
    flex-shrink: 0;
  }

  @media (max-width: 620px) {
    .control-row {
      flex-direction: column;
      align-items: flex-start;
    }
    .compact-control {
      width: 100%;
    }
  }
}
</style>
