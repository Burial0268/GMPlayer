<template>
  <div class="set-other">
    <n-card
      v-if="supported"
      class="set-item updater-card"
      :content-style="{
        flexDirection: 'column',
        alignItems: 'flex-start',
      }"
    >
      <div class="top">
        <div class="name">
          <div class="dev">
            {{ $t("setting.appUpdate") }}
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
            {{ $t("setting.checkUpdate") }}
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
            <span class="version-label">{{ $t("setting.versionCurrent") }}</span>
            <span class="version-value">v{{ updaterState.update.currentVersion }}</span>
          </div>
          <n-icon class="version-arrow" :component="ArrowForwardRound" />
          <div class="version-item is-latest">
            <span class="version-label">{{ $t("setting.versionLatest") }}</span>
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
              {{ $t("setting.updateEta", { time: etaText }) }}
            </span>
            <span v-if="isInstalling" class="meta-item">{{ $t("setting.installingUpdate") }}…</span>
          </div>
        </div>

        <n-alert v-if="updaterState.status === 'installed'" type="success" :bordered="false">
          {{ $t("setting.updateInstalledTip", { version: updaterState.installedVersion }) }}
        </n-alert>
        <n-alert v-else-if="updaterState.status === 'error'" type="error" :bordered="false">
          {{ updaterState.error || $t("setting.updateFailed") }}
        </n-alert>

        <div v-if="updaterState.update?.body" class="release-notes">
          <div class="release-notes-title">
            <n-icon :component="DescriptionRound" />
            {{ $t("setting.releaseNotesTitle") }}
          </div>
          <n-scrollbar class="release-notes-body">{{ updaterState.update.body }}</n-scrollbar>
        </div>
      </div>
    </n-card>
    <n-card class="set-item">
      <div class="name">
        {{ $t("setting.resetApp") }}
        <span class="tip">{{ $t("setting.resetAppTip") }}</span>
      </div>
      <n-button strong secondary type="error" @click="resetApp">
        {{ $t("general.name.restore") }}
      </n-button>
    </n-card>
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

const { t } = useI18n();
const themeVars = useThemeVars();
const {
  updaterState,
  supported,
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

// State-driven icon and color for the compact update status line.
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

// 程序重置
const resetApp = () => {
  const cleanAll = () => {
    if ($message) {
      $message.success(t("other.cleanAll"));
    } else {
      alert(t("other.cleanAll"));
    }
    localStorage.clear();
    window.location.href = "/";
  };
  $dialog.warning({
    class: "s-dialog",
    title: t("setting.resetApp"),
    content: t("setting.resetAppWarning"),
    positiveText: t("setting.resetApp"),
    negativeText: t("general.dialog.cancel"),
    onPositiveClick: () => {
      if ($cleanAll) {
        $cleanAll();
      } else {
        cleanAll();
      }
    },
  });
};
</script>

<style lang="scss" scoped>
.updater-card {
  .top {
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    width: 100%;
  }

  .actions {
    display: flex;
    flex-shrink: 0;
    align-items: center;
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
    display: flex;
    flex-direction: column;
    gap: 12px;
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
      max-height: 140px;
      font-size: 12px;
      line-height: 1.6;
      white-space: pre-wrap;
      opacity: 0.82;
    }
  }

  @media (max-width: 768px) {
    .top {
      flex-direction: column;
      align-items: flex-start;
      gap: 12px;
    }

    .actions {
      width: 100%;
      justify-content: flex-end;
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
