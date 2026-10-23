<template>
  <div class="settings-download">
    <div class="custom-heading">
      <div class="name">
        {{ t("setting.download.title") }}
        <span class="tip">{{ t("setting.download.tip") }}</span>
      </div>
      <n-button strong secondary round size="small" @click="openManager">
        {{ t("download.manager") }}
      </n-button>
    </div>

    <n-alert v-if="!available" type="warning" :bordered="false">
      {{ t("download.tauriOnly") }}
    </n-alert>

    <div v-else class="download-controls">
      <div class="destination">
        <div class="text">
          <n-text class="key">{{ t("download.destination") }}</n-text>
          <n-text class="value" :depth="3">{{ destination }}</n-text>
          <n-text v-if="usingDefault" class="value" :depth="3">
            {{ t(defaultHintKey) }}
          </n-text>
        </div>
        <n-space :size="8">
          <n-button v-if="revealable" quaternary size="small" @click="reveal">
            {{ t("download.reveal") }}
          </n-button>
          <n-button strong secondary round size="small" @click="pick">
            {{ t("download.change") }}
          </n-button>
        </n-space>
      </div>

      <div class="control-row">
        <div class="row-name">
          {{ t("setting.download.defaultBr") }}
          <span>{{ t("setting.download.defaultBrTip") }}</span>
        </div>
        <n-select
          class="compact-control"
          :value="settings?.defaultBr ?? 320000"
          :options="brOptions"
          @update:value="(value) => patch({ defaultBr: Number(value) })"
        />
      </div>

      <div class="control-row">
        <div class="row-name">
          {{ t("setting.download.concurrency") }}
          <span>{{ t("setting.download.concurrencyTip") }}</span>
        </div>
        <n-input-number
          class="compact-control"
          :value="settings?.concurrency ?? 3"
          :min="1"
          :max="6"
          :step="1"
          @update:value="(value) => patch({ concurrency: Number(value) || 1 })"
        />
      </div>

      <div class="control-row">
        <div class="row-name">
          {{ t("setting.download.segments") }}
          <span>{{ t("setting.download.segmentsTip") }}</span>
        </div>
        <n-input-number
          class="compact-control"
          :value="settings?.segments ?? 4"
          :min="1"
          :max="8"
          :step="1"
          @update:value="(value) => patch({ segments: Number(value) || 1 })"
        />
      </div>

      <div class="control-row">
        <div class="row-name">
          {{ t("setting.download.template") }}
          <span>{{ t("setting.download.templateTip") }}</span>
        </div>
        <n-input
          class="compact-control"
          :value="template"
          :placeholder="DEFAULT_TEMPLATE"
          @update:value="(value) => (template = value)"
          @blur="commitTemplate"
          @keyup.enter="commitTemplate"
        />
      </div>

      <div class="control-row">
        <div class="row-name">
          {{ t("setting.download.embedTags") }}
          <span>{{ t("setting.download.embedTagsTip") }}</span>
        </div>
        <n-switch
          :value="settings?.embedTags ?? true"
          :round="false"
          @update:value="(value) => patch({ embedTags: value })"
        />
      </div>

      <div class="control-row">
        <div class="row-name">
          {{ t("setting.download.writeLyric") }}
          <span>{{ t("setting.download.writeLyricTip") }}</span>
        </div>
        <n-switch
          :value="settings?.writeLyric ?? true"
          :round="false"
          @update:value="(value) => patch({ writeLyric: value })"
        />
      </div>

      <div class="control-row">
        <div class="row-name">
          {{ t("setting.download.writeCover") }}
          <span>{{ t("setting.download.writeCoverTip") }}</span>
        </div>
        <n-switch
          :value="settings?.writeCover ?? false"
          :round="false"
          @update:value="(value) => patch({ writeCover: value })"
        />
      </div>
    </div>

    <DownloadManager ref="managerRef" />
  </div>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useDownloadStore } from "@/store";
import { dirIsRevealable, dirLabelOf, downloadAvailable } from "@/utils/download";
import DownloadManager from "@/components/DataModal/DownloadManager.vue";

const { t } = useI18n();
const download = useDownloadStore();

const DEFAULT_TEMPLATE = "{artist} - {title}";

const available = downloadAvailable();
const managerRef = ref<InstanceType<typeof DownloadManager> | null>(null);
const settings = computed(() => download.settings);

/**
 * 模板是本页唯一的自由文本，所以它是本地 ref 而不是直接双向绑到 store。
 *
 * 每敲一个字符都写一次 Rust 就是每敲一个字符都 `fsync` 一次 `download.json`；提交时机
 * 定在 blur 和回车。
 */
const template = ref(DEFAULT_TEMPLATE);

const destination = computed(() => dirLabelOf(download.settings) || t("download.noDestination"));
const revealable = computed(() => dirIsRevealable(download.settings));
/** 还没选过目录：桌面会在第一次下载时自动落到平台的音乐目录。 */
const usingDefault = computed(() => available && !download.settings?.dir);
/**
 * Android 没有可以先说出来的默认值，所以这两句话不能共用一句。
 *
 * `defaultDir` 在 Android 上是 app 私有目录，`dirLabelOf` 故意不显示它，于是"会落到
 * 上面这个目录"上面什么都没有。那里真正会发生的是弹一次目录选择框——而 `Download`
 * 从 targetSdk 30 起根本授不了权，所以落点是 Music 下的子目录。
 */
const defaultHintKey = computed(() =>
  download.settings?.requiresPick
    ? "setting.download.usingDefaultPick"
    : "setting.download.usingDefault",
);

const brOptions = computed(() => [
  { label: t("general.type.quality.l"), value: 128000 },
  { label: t("general.type.quality.m"), value: 192000 },
  { label: t("general.type.quality.h"), value: 320000 },
  { label: t("general.type.quality.sq"), value: 420000 },
  { label: "Hi-Res", value: 999000 },
]);

const patch = async (value: Parameters<typeof download.patchConfig>[0]) => {
  try {
    await download.patchConfig(value);
  } catch (error) {
    $message.error(String(error));
  }
};

const commitTemplate = () => {
  const next = template.value.trim() || DEFAULT_TEMPLATE;
  template.value = next;
  if (next === download.settings?.filenameTemplate) return;
  patch({ filenameTemplate: next });
};

const pick = async () => {
  try {
    await download.pick();
  } catch (error) {
    $message.error(String(error));
  }
};

/** 桌面独有：Android 的 `content://` 文档没有可以交给文件管理器的路径。 */
const reveal = async () => {
  const dir = download.settings?.dir;
  if (!dir) return;
  try {
    const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
    await revealItemInDir(dir);
  } catch (error) {
    $message.error(String(error));
  }
};

const openManager = () => managerRef.value?.open();

onMounted(async () => {
  await download.hydrate();
  template.value = download.settings?.filenameTemplate || DEFAULT_TEMPLATE;
});

// 目录换了之后 Rust 会把整份配置推回来，模板输入框要跟上——否则一次外部改动会被
// 下一次 blur 用陈旧的本地值覆盖掉。
watch(
  () => download.settings?.filenameTemplate,
  (value) => {
    if (value && value !== template.value) template.value = value;
  },
);
</script>

<style lang="scss" scoped>
.settings-download {
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

  .download-controls {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .destination {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
    border-radius: 10px;
    background-color: rgba(var(--main-color), 0.08);
    .text {
      display: flex;
      flex-direction: column;
      min-width: 0;
      .key {
        font-size: 13px;
        font-weight: bold;
      }
      .value {
        font-size: 12px;
        word-break: break-all;
      }
    }
    > :last-child {
      margin-left: auto;
      flex-shrink: 0;
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
    .destination {
      flex-direction: column;
      align-items: flex-start;
      > :last-child {
        margin-left: 0;
      }
    }
  }
}
</style>
