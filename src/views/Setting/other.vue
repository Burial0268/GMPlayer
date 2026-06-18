<template>
  <div class="set-other">
    <n-card v-if="supported" class="set-item updater-card">
      <div class="name">
        {{ $t("setting.appUpdate") }}
        <span class="tip">{{ updaterTip }}</span>
      </div>
      <div class="set updater-actions">
        <n-space justify="end" align="center" :size="8">
          <n-tag v-if="hasUpdate && updaterState.update" type="warning" size="small" round>
            v{{ updaterState.update.version }}
          </n-tag>
          <n-button
            strong
            secondary
            :loading="updaterState.status === 'checking'"
            :disabled="isBusy"
            @click="handleCheckUpdate"
          >
            {{ $t("setting.checkUpdate") }}
          </n-button>
          <n-button
            v-if="
              hasUpdate ||
              updaterState.status === 'downloading' ||
              updaterState.status === 'installing'
            "
            strong
            secondary
            type="primary"
            :loading="updaterState.status === 'downloading' || updaterState.status === 'installing'"
            :disabled="updaterState.status === 'checking'"
            @click="handleInstallUpdate"
          >
            {{ installButtonText }}
          </n-button>
        </n-space>
      </div>
      <div v-if="showUpdaterDetails" class="updater-details">
        <div v-if="updaterState.update" class="updater-version">
          <span>{{
            $t("setting.currentVersion", { version: updaterState.update.currentVersion })
          }}</span>
          <span>{{ $t("setting.latestVersion", { version: updaterState.update.version }) }}</span>
        </div>
        <n-progress
          v-if="updaterState.status === 'downloading' || updaterState.status === 'installing'"
          type="line"
          :percentage="progressPercent ?? 0"
          :show-indicator="progressPercent !== null"
          processing
        />
        <div
          v-if="updaterState.status === 'downloading' && updaterState.downloadedBytes"
          class="updater-size"
        >
          {{ downloadedSizeText }}
        </div>
        <n-alert v-if="updaterState.status === 'installed'" type="success" :show-icon="false">
          {{ $t("setting.updateInstalledTip", { version: updaterState.installedVersion }) }}
        </n-alert>
        <n-alert v-else-if="updaterState.status === 'error'" type="error" :show-icon="false">
          {{ updaterState.error || $t("setting.updateFailed") }}
        </n-alert>
        <div v-if="updaterState.update?.body" class="release-notes">
          {{ updaterState.update.body }}
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
import { useAppUpdater } from "@/composables/useAppUpdater";

const { t } = useI18n();
const {
  updaterState,
  supported,
  hasUpdate,
  isBusy,
  progressPercent,
  checkForUpdate,
  installAvailableUpdate,
} = useAppUpdater();

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

const handleCheckUpdate = async () => {
  const update = await checkForUpdate();
  if (update) {
    $message?.success(t("setting.updateAvailableTip", { version: update.version }));
  } else if (updaterState.status === "error") {
    $message?.error(t("setting.updateFailed"));
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
        $message?.error(t("setting.updateFailed"));
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
  :deep(.n-card__content) {
    flex-wrap: wrap;
    gap: 12px;
    align-items: flex-start;
  }

  .updater-actions {
    width: auto;
    min-width: 240px;
    display: flex;
    justify-content: flex-end;

    @media (max-width: 768px) {
      width: 100%;
      min-width: 0;
      justify-content: flex-start;
    }
  }

  .updater-details {
    width: 100%;
    padding: 12px;
    border-radius: 8px;
    background-color: var(--n-border-color);
    box-sizing: border-box;
  }

  .updater-version {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 16px;
    margin-bottom: 10px;
    font-size: 12px;
    opacity: 0.82;
  }

  .updater-size {
    margin-top: 6px;
    font-size: 12px;
    opacity: 0.72;
  }

  .release-notes {
    margin-top: 10px;
    white-space: pre-wrap;
    font-size: 12px;
    line-height: 1.6;
    opacity: 0.82;
  }
}
</style>
