<template>
  <div class="settings-app-update">
    <div class="top">
      <div class="name">
        <div class="dev">
          {{ t("setting.appUpdate") }}
          <n-tag
            v-if="hasUpdate && updaterState.update"
            type="warning"
            size="small"
            round
            :bordered="false"
          >
            v{{ updaterState.update.version }}
          </n-tag>
        </div>
        <span class="tip updater-status">
          <n-icon
            class="status-icon"
            :class="{ spin: statusVisual.spin }"
            :component="statusVisual.icon"
            :style="{ color: statusVisual.color }"
          />
          {{ updaterTip }}
        </span>
      </div>
      <div class="actions">
        <n-button
          strong
          secondary
          :loading="updaterState.status === 'checking'"
          :disabled="isBusy"
          @click="handleCheckUpdate"
        >
          <template #icon>
            <n-icon :component="RefreshRound" />
          </template>
          {{ t("setting.checkUpdate") }}
        </n-button>
        <n-button
          v-if="showInstallButton"
          strong
          secondary
          type="primary"
          :loading="updaterState.status === 'downloading' || updaterState.status === 'installing'"
          :disabled="updaterState.status === 'checking'"
          @click="handleInstallUpdate"
        >
          <template #icon>
            <n-icon :component="RocketLaunchRound" />
          </template>
          {{ installButtonText }}
        </n-button>
      </div>
    </div>

    <div v-if="showUpdaterDetails" class="more">
      <div v-if="updaterState.update" class="version-row">
        <div class="version-item">
          <span class="version-label">{{ t("setting.versionCurrent") }}</span>
          <span class="version-value">v{{ updaterState.update.currentVersion }}</span>
        </div>
        <n-icon class="version-arrow" :component="ArrowForwardRound" />
        <div class="version-item is-latest">
          <span class="version-label">{{ t("setting.versionLatest") }}</span>
          <span class="version-value" :style="{ color: themeVars.primaryColor }">
            v{{ updaterState.update.version }}
          </span>
        </div>
      </div>

      <div v-if="isDownloading || isInstalling" class="progress-block">
        <div class="progress-bar-row">
          <n-progress
            class="progress-bar"
            type="line"
            :percentage="isDownloading ? (progressPercent ?? 0) : 100"
            :show-indicator="false"
            :height="8"
            :border-radius="6"
            :processing="isInstalling || progressPercent === null"
          />
          <span class="progress-pct">{{ progressLabel }}</span>
        </div>
        <div class="progress-meta">
          <span v-if="isDownloading" class="meta-item">
            <n-icon :component="CloudDownloadRound" />
            {{ downloadedSizeText }}
          </span>
          <span v-if="isDownloading && updaterState.downloadSpeed > 0" class="meta-item">
            <n-icon :component="SpeedRound" />
            {{ speedText }}
          </span>
          <span v-if="isDownloading && etaText" class="meta-item">
            <n-icon :component="ScheduleRound" />
            {{ t("setting.updateEta", { time: etaText }) }}
          </span>
          <span v-if="isInstalling" class="meta-item">{{ t("setting.installingUpdate") }}...</span>
        </div>
      </div>

      <n-alert v-if="updaterState.status === 'installed'" type="success" :bordered="false">
        {{ t("setting.updateInstalledTip", { version: updaterState.installedVersion }) }}
      </n-alert>
      <n-alert v-else-if="updaterState.status === 'error'" type="error" :bordered="false">
        {{ updaterState.error || t("setting.updateFailed") }}
      </n-alert>

      <div v-if="updaterState.update?.body" class="release-notes">
        <div class="release-notes-title">
          <n-icon :component="DescriptionRound" />
          {{ t("setting.releaseNotesTitle") }}
        </div>
        <n-scrollbar class="release-notes-body">
          <div
            class="release-notes-markdown markdown-body"
            :data-theme="isDarkTheme ? 'dark' : 'light'"
            v-html="releaseNotesHtml"
          />
        </n-scrollbar>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useThemeVars } from "naive-ui";
import {
  ArrowForwardRound,
  CheckCircleRound,
  CloudDownloadRound,
  DescriptionRound,
  DownloadingRound,
  ErrorOutlineRound,
  NewReleasesRound,
  RefreshRound,
  RocketLaunchRound,
  ScheduleRound,
  SpeedRound,
  SystemUpdateAltRound,
  TaskAltRound,
} from "@vicons/material";
import { useAppUpdater } from "@/composables/useAppUpdater";
import { useSettingDataStore } from "@/store";
import MarkdownIt from "markdown-it";

declare const $dialog: any;
declare const $message: any;

const settingData = useSettingDataStore();
const isDarkTheme = computed(() => settingData.getSiteTheme === "dark");

const md = new MarkdownIt({
  html: false,
  breaks: true,
  linkify: true,
});

const { t } = useI18n();
const themeVars = useThemeVars();
const {
  updaterState,
  hasUpdate,
  isBusy,
  progressPercent,
  etaSeconds,
  checkForUpdate,
  installAvailableUpdate,
} = useAppUpdater();

const isDownloading = computed(() => updaterState.status === "downloading");
const isInstalling = computed(() => updaterState.status === "installing");
const showInstallButton = computed(
  () => hasUpdate.value || isDownloading.value || isInstalling.value,
);

const releaseNotesHtml = computed(() => md.render(updaterState.update?.body ?? ""));

const updaterTip = computed(() => {
  if (updaterState.status === "checking") return t("setting.checkingUpdate");
  if (updaterState.status === "available" && updaterState.update) {
    return t("setting.updateAvailableTip", { version: updaterState.update.version });
  }
  if (updaterState.status === "not-available") return t("setting.noUpdate");
  if (updaterState.status === "downloading") return t("setting.downloadingUpdate");
  if (updaterState.status === "installing") return t("setting.installingUpdate");
  if (updaterState.status === "installed") return t("setting.updateInstalled");
  if (updaterState.status === "error") return t("setting.updateFailed");
  return t("setting.appUpdateTip");
});

const statusVisual = computed(() => {
  switch (updaterState.status) {
    case "checking":
      return { icon: RefreshRound, color: themeVars.value.infoColor, spin: true };
    case "available":
      return { icon: NewReleasesRound, color: themeVars.value.warningColor, spin: false };
    case "downloading":
      return { icon: DownloadingRound, color: themeVars.value.primaryColor, spin: false };
    case "installing":
      return { icon: RocketLaunchRound, color: themeVars.value.primaryColor, spin: false };
    case "installed":
      return { icon: TaskAltRound, color: themeVars.value.successColor, spin: false };
    case "not-available":
      return { icon: CheckCircleRound, color: themeVars.value.successColor, spin: false };
    case "error":
      return { icon: ErrorOutlineRound, color: themeVars.value.errorColor, spin: false };
    default:
      return { icon: SystemUpdateAltRound, color: themeVars.value.textColor3, spin: false };
  }
});

const installButtonText = computed(() => {
  if (updaterState.status === "installing") return t("setting.installingUpdate");
  if (updaterState.status === "downloading") return t("setting.downloadingUpdate");
  return t("setting.installUpdate");
});

const showUpdaterDetails = computed(
  () =>
    !!updaterState.update ||
    ["downloading", "installing", "installed", "error"].includes(updaterState.status),
);

const formatBytes = (bytes: number) => {
  if (!bytes) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return `${(bytes / 1024 ** index).toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
};

const downloadedSizeText = computed(() => {
  if (!updaterState.contentLength) return formatBytes(updaterState.downloadedBytes);
  return `${formatBytes(updaterState.downloadedBytes)} / ${formatBytes(updaterState.contentLength)}`;
});

const speedText = computed(() => `${formatBytes(updaterState.downloadSpeed)}/s`);
const progressLabel = computed(() => {
  if (isInstalling.value) return "";
  if (progressPercent.value === null) return "";
  return `${progressPercent.value}%`;
});
const etaText = computed(() => {
  if (etaSeconds.value === null) return "";
  const total = Math.max(0, Math.ceil(etaSeconds.value));
  if (total >= 3600) {
    return `${Math.floor(total / 3600)}h ${Math.floor((total % 3600) / 60)}m`;
  }
  if (total >= 60) return `${Math.floor(total / 60)}m ${total % 60}s`;
  return `${total}s`;
});

const handleCheckUpdate = async () => {
  const update = await checkForUpdate();
  if (update) {
    $message?.success(t("setting.updateAvailableTip", { version: update.version }));
  } else if (updaterState.status === "error") {
    $message?.error(updaterState.error || t("setting.updateFailed"));
  } else {
    $message?.success(t("setting.noUpdate"));
  }
};

const handleInstallUpdate = () => {
  if (!updaterState.update) return;
  $dialog.warning({
    class: "s-dialog",
    title: t("setting.installUpdate"),
    content: t("setting.installUpdateConfirm", { version: updaterState.update.version }),
    positiveText: t("setting.installUpdate"),
    negativeText: t("general.dialog.cancel"),
    onPositiveClick: async () => {
      const installed = await installAvailableUpdate();
      if (installed) {
        $message?.success(t("setting.updateInstalled"));
      } else {
        $message?.error(updaterState.error || t("setting.updateFailed"));
      }
    },
  });
};
</script>

<style lang="scss" scoped>
.settings-app-update {
  width: 100%;

  .top {
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    width: 100%;
  }

  .name {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 15px;

    .dev {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 6px;
    }

    .tip {
      font-size: 12px;
      opacity: 0.68;
    }
  }

  .actions {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: 8px;
  }

  .updater-status {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-top: 2px;

    .status-icon {
      flex-shrink: 0;
      font-size: 15px;
    }

    .spin {
      animation: updater-spin 1s linear infinite;
    }
  }

  .more {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin-top: 12px;
  }

  .version-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    font-size: 12px;
  }

  .version-item {
    display: inline-flex;
    align-items: baseline;
    gap: 6px;

    .version-label {
      opacity: 0.6;
    }

    .version-value {
      font-size: 13px;
      font-weight: 600;
      font-variant-numeric: tabular-nums;
    }
  }

  .version-arrow {
    font-size: 18px;
    opacity: 0.4;
  }

  .progress-block {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .progress-bar-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .progress-bar {
    flex: 1 1 auto;
  }

  .progress-pct {
    min-width: 42px;
    font-size: 13px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .progress-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 16px;
    font-size: 12px;
    opacity: 0.75;

    .meta-item {
      display: inline-flex;
      align-items: center;
      gap: 4px;

      .n-icon {
        font-size: 14px;
        opacity: 0.8;
      }
    }
  }

  .release-notes {
    display: flex;
    flex-direction: column;
    gap: 6px;

    .release-notes-title {
      display: flex;
      align-items: center;
      gap: 6px;
      font-size: 12px;
      font-weight: 600;
      opacity: 0.8;

      .n-icon {
        font-size: 14px;
      }
    }

    .release-notes-body {
      max-height: 320px;
      border: 1px solid color-mix(in srgb, var(--n-border-color) 70%, transparent);
      border-radius: 6px;
      overflow: hidden;

      :deep(.release-notes-markdown) {
        --base-size-4: 0.25rem;
        --base-size-8: 0.5rem;
        --base-size-16: 1rem;
        --base-size-24: 1.5rem;
        --base-size-40: 2.5rem;
        --base-text-weight-medium: 500;
        --base-text-weight-normal: 400;
        --base-text-weight-semibold: 600;
        --fontStack-monospace:
          ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace;
        --fontStack-sansSerif:
          -apple-system, BlinkMacSystemFont, "Segoe UI", "Noto Sans", Helvetica, Arial, sans-serif,
          "Apple Color Emoji", "Segoe UI Emoji";
        --fgColor-accent: #0969da;
        --fgColor-attention: #9a6700;
        --fgColor-danger: #d1242f;
        --fgColor-default: #1f2328;
        --fgColor-done: #8250df;
        --fgColor-muted: #59636e;
        --fgColor-success: #1a7f37;
        --bgColor-attention-muted: #fff8c5;
        --bgColor-default: #fff;
        --bgColor-muted: #f6f8fa;
        --bgColor-neutral-muted: #818b981f;
        --borderColor-accent-emphasis: #0969da;
        --borderColor-attention-emphasis: #9a6700;
        --borderColor-danger-emphasis: #cf222e;
        --borderColor-default: #d1d9e0;
        --borderColor-done-emphasis: #8250df;
        --borderColor-muted: #d1d9e0b3;
        --borderColor-neutral-muted: var(--borderColor-muted);
        --borderColor-success-emphasis: #1a7f37;
        --focus-outlineColor: var(--borderColor-accent-emphasis);

        box-sizing: border-box;
        min-width: 0;
        padding: 16px;
        margin: 0;
        color: var(--fgColor-default);
        font-family: var(--fontStack-sansSerif);
        font-size: 16px;
        font-weight: var(--base-text-weight-normal);
        line-height: 1.5;
        word-wrap: break-word;
        color-scheme: light;
        background-color: var(--bgColor-default);
        -webkit-text-size-adjust: 100%;
      }

      :deep(.release-notes-markdown[data-theme="dark"]) {
        --fgColor-accent: #4493f8;
        --fgColor-attention: #d29922;
        --fgColor-danger: #f85149;
        --fgColor-default: #f0f6fc;
        --fgColor-done: #ab7df8;
        --fgColor-muted: #9198a1;
        --fgColor-success: #3fb950;
        --bgColor-attention-muted: #bb800926;
        --bgColor-default: #0d1117;
        --bgColor-muted: #151b23;
        --bgColor-neutral-muted: #656c7633;
        --borderColor-accent-emphasis: #1f6feb;
        --borderColor-attention-emphasis: #9e6a03;
        --borderColor-danger-emphasis: #da3633;
        --borderColor-default: #3d444d;
        --borderColor-done-emphasis: #8957e5;
        --borderColor-muted: #3d444db3;
        --borderColor-success-emphasis: #238636;

        color-scheme: dark;
      }

      :deep(.release-notes-markdown *) {
        box-sizing: border-box;
      }

      :deep(.release-notes-markdown::before),
      :deep(.release-notes-markdown::after) {
        display: table;
        content: "";
      }

      :deep(.release-notes-markdown::after) {
        clear: both;
      }

      :deep(.release-notes-markdown > *:first-child) {
        margin-top: 0 !important;
      }

      :deep(.release-notes-markdown > *:last-child) {
        margin-bottom: 0 !important;
      }

      :deep(.release-notes-markdown h1),
      :deep(.release-notes-markdown h2),
      :deep(.release-notes-markdown h3),
      :deep(.release-notes-markdown h4),
      :deep(.release-notes-markdown h5),
      :deep(.release-notes-markdown h6) {
        margin-top: var(--base-size-24);
        margin-bottom: var(--base-size-16);
        font-weight: var(--base-text-weight-semibold);
        line-height: 1.25;
      }

      :deep(.release-notes-markdown h1),
      :deep(.release-notes-markdown h2) {
        padding-bottom: 0.3em;
        border-bottom: 1px solid var(--borderColor-muted);
      }

      :deep(.release-notes-markdown h1) {
        margin: 0.67em 0;
        font-size: 2em;
      }

      :deep(.release-notes-markdown h2) {
        font-size: 1.5em;
      }

      :deep(.release-notes-markdown h3) {
        font-size: 1.25em;
      }

      :deep(.release-notes-markdown h4) {
        font-size: 1em;
      }

      :deep(.release-notes-markdown h5) {
        font-size: 0.875em;
      }

      :deep(.release-notes-markdown h6) {
        color: var(--fgColor-muted);
        font-size: 0.85em;
      }

      :deep(.release-notes-markdown p),
      :deep(.release-notes-markdown blockquote),
      :deep(.release-notes-markdown ul),
      :deep(.release-notes-markdown ol),
      :deep(.release-notes-markdown dl),
      :deep(.release-notes-markdown table),
      :deep(.release-notes-markdown pre),
      :deep(.release-notes-markdown details) {
        margin-top: 0;
        margin-bottom: var(--base-size-16);
      }

      :deep(.release-notes-markdown p) {
        margin-top: 0;
        margin-bottom: 10px;
      }

      :deep(.release-notes-markdown a) {
        color: var(--fgColor-accent);
        text-decoration: none;
        background-color: transparent;
      }

      :deep(.release-notes-markdown a:hover) {
        text-decoration: underline;
      }

      :deep(.release-notes-markdown strong) {
        font-weight: var(--base-text-weight-semibold);
      }

      :deep(.release-notes-markdown mark) {
        color: var(--fgColor-default);
        background-color: var(--bgColor-attention-muted);
      }

      :deep(.release-notes-markdown small) {
        font-size: 90%;
      }

      :deep(.release-notes-markdown sub),
      :deep(.release-notes-markdown sup) {
        position: relative;
        font-size: 75%;
        line-height: 0;
        vertical-align: baseline;
      }

      :deep(.release-notes-markdown sub) {
        bottom: -0.25em;
      }

      :deep(.release-notes-markdown sup) {
        top: -0.5em;
      }

      :deep(.release-notes-markdown ul),
      :deep(.release-notes-markdown ol) {
        margin-top: 0;
        margin-bottom: 0;
        padding-left: 2em;
      }

      :deep(.release-notes-markdown ol ol),
      :deep(.release-notes-markdown ul ol) {
        list-style-type: lower-roman;
      }

      :deep(.release-notes-markdown ul ul ol),
      :deep(.release-notes-markdown ul ol ol),
      :deep(.release-notes-markdown ol ul ol),
      :deep(.release-notes-markdown ol ol ol) {
        list-style-type: lower-alpha;
      }

      :deep(.release-notes-markdown ul ul),
      :deep(.release-notes-markdown ul ol),
      :deep(.release-notes-markdown ol ol),
      :deep(.release-notes-markdown ol ul) {
        margin-top: 0;
        margin-bottom: 0;
      }

      :deep(.release-notes-markdown li + li) {
        margin-top: 0.25em;
      }

      :deep(.release-notes-markdown li > p) {
        margin-top: var(--base-size-16);
      }

      :deep(.release-notes-markdown blockquote) {
        margin: 0;
        padding: 0 1em;
        color: var(--fgColor-muted);
        border-left: 0.25em solid var(--borderColor-default);
      }

      :deep(.release-notes-markdown blockquote > :first-child) {
        margin-top: 0;
      }

      :deep(.release-notes-markdown blockquote > :last-child) {
        margin-bottom: 0;
      }

      :deep(.release-notes-markdown hr) {
        box-sizing: content-box;
        height: 0.25em;
        padding: 0;
        margin: var(--base-size-24) 0;
        overflow: hidden;
        background-color: var(--borderColor-default);
        border: 0;
      }

      :deep(.release-notes-markdown kbd) {
        display: inline-block;
        padding: var(--base-size-4);
        color: var(--fgColor-default);
        font: 11px
          var(
            --fontStack-monospace,
            ui-monospace,
            SFMono-Regular,
            "SF Mono",
            Menlo,
            Consolas,
            "Liberation Mono",
            monospace
          );
        line-height: 10px;
        vertical-align: middle;
        background-color: var(--bgColor-muted);
        border: solid 1px var(--borderColor-neutral-muted);
        border-radius: 6px;
        box-shadow: inset 0 -1px 0 var(--borderColor-neutral-muted);
      }

      :deep(.release-notes-markdown code),
      :deep(.release-notes-markdown tt),
      :deep(.release-notes-markdown samp) {
        padding: 0.2em 0.4em;
        margin: 0;
        font-family: var(--fontStack-monospace);
        font-size: 85%;
        white-space: break-spaces;
        background-color: var(--bgColor-neutral-muted);
        border-radius: 6px;
      }

      :deep(.release-notes-markdown pre),
      :deep(.release-notes-markdown code),
      :deep(.release-notes-markdown kbd),
      :deep(.release-notes-markdown samp),
      :deep(.release-notes-markdown tt) {
        font-family: var(--fontStack-monospace);
      }

      :deep(.release-notes-markdown pre) {
        padding: var(--base-size-16);
        margin-top: 0;
        margin-bottom: 0;
        overflow: auto;
        color: var(--fgColor-default);
        font-size: 85%;
        line-height: 1.45;
        word-wrap: normal;
        background-color: var(--bgColor-muted);
        border-radius: 6px;
      }

      :deep(.release-notes-markdown pre code) {
        display: inline;
        max-width: none;
        padding: 0;
        margin: 0;
        overflow: visible;
        font-size: 100%;
        line-height: inherit;
        white-space: pre;
        word-break: normal;
        background: transparent;
        background-color: transparent;
        border: 0;
      }

      :deep(.release-notes-markdown table) {
        display: block;
        width: max-content;
        max-width: 100%;
        overflow: auto;
        border-spacing: 0;
        border-collapse: collapse;
      }

      :deep(.release-notes-markdown tr) {
        background-color: var(--bgColor-default);
        border-top: 1px solid var(--borderColor-muted);
      }

      :deep(.release-notes-markdown tr:nth-child(2n)) {
        background-color: var(--bgColor-muted);
      }

      :deep(.release-notes-markdown th),
      :deep(.release-notes-markdown td) {
        padding: 6px 13px;
        border: 1px solid var(--borderColor-default);
      }

      :deep(.release-notes-markdown th) {
        font-weight: var(--base-text-weight-semibold);
      }

      :deep(.release-notes-markdown img) {
        max-width: 100%;
        box-sizing: content-box;
        border-style: none;
      }

      :deep(.release-notes-markdown img[align="right"]) {
        padding-left: 20px;
      }

      :deep(.release-notes-markdown img[align="left"]) {
        padding-right: 20px;
      }
    }
  }
}

@media (max-width: 640px) {
  .settings-app-update {
    .top {
      flex-direction: column;
      align-items: flex-start;
    }

    .actions {
      width: 100%;
    }
  }
}

@keyframes updater-spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}
</style>
