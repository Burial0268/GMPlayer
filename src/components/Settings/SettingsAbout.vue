<template>
  <div class="settings-about">
    <div class="identity">
      <img class="logo" :src="siteLogo" :alt="siteTitle" />
      <div class="meta">
        <div class="title-row">
          <span class="name">{{ siteTitle }}</span>
          <n-tag round size="small" :bordered="false" type="primary">
            v{{ runtime?.appVersion ?? appInfo.version }}
          </n-tag>
          <n-tag round size="small" :bordered="false">{{ runtimeLabel }}</n-tag>
        </div>
        <span class="desc">{{ siteDesc }}</span>
      </div>
    </div>

    <n-descriptions class="runtime-info" bordered size="small" label-placement="left" :column="1">
      <n-descriptions-item v-for="row in infoRows" :key="row.key" :label="row.label">
        <span class="value">{{ row.value }}</span>
      </n-descriptions-item>
    </n-descriptions>

    <div class="actions">
      <n-button strong secondary @click="jumpUrl(appInfo.github)">
        <template #icon>
          <n-icon :component="CodeRound" />
        </template>
        GitHub
      </n-button>
      <n-button strong secondary @click="jumpUrl(appInfo.home)">
        <template #icon>
          <n-icon :component="LanguageRound" />
        </template>
        {{ t("setting.aboutHomepage") }}
      </n-button>
      <n-button strong secondary @click="copyRuntimeInfo">
        <template #icon>
          <n-icon :component="ContentCopyRound" />
        </template>
        {{ t("setting.aboutCopyInfo") }}
      </n-button>
    </div>

    <div class="copyright">
      <span>
        Copyright&nbsp;©&nbsp;2020 - {{ currentYear }}
        <n-a :href="appInfo.home" target="_blank">{{ appInfo.author }}</n-a>
      </span>
      <span class="point">·</span>
      <span>{{ t("setting.aboutLicense") }}</span>
      <template v-if="icp">
        <span class="point">·</span>
        <n-a class="beian" href="https://beian.miit.gov.cn/" target="_blank" v-html="icp" />
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { CodeRound, ContentCopyRound, LanguageRound } from "@vicons/material";
import { appInfo } from "@/utils/appInfo";
import { openExternalUrl } from "@/utils/openLink";
import { getRuntimeInfo, type RuntimeInfo } from "@/utils/systemInfo";

declare const $message: any;

const { t } = useI18n();

const siteTitle = import.meta.env.VITE_SITE_TITLE;
const siteDesc = import.meta.env.VITE_SITE_DES;
const siteLogo = (import.meta.env.VITE_SITE_LOGO as string) || "/favicon.ico";
const icp = import.meta.env.VITE_ICP || null;
const currentYear = new Date().getFullYear();

const runtime = ref<RuntimeInfo | null>(null);

const runtimeLabel = computed(() => {
  if (!runtime.value) return t("setting.aboutRuntimeWeb");
  if (runtime.value.kind === "desktop") return t("setting.aboutRuntimeDesktop");
  if (runtime.value.kind === "mobile") return t("setting.aboutRuntimeMobile");
  return t("setting.aboutRuntimeWeb");
});

const unknown = computed(() => t("setting.aboutUnknown"));

const runtimeDetail = computed(() => {
  const info = runtime.value;
  if (!info?.tauriVersion) return runtimeLabel.value;
  return `${runtimeLabel.value} · Tauri ${info.tauriVersion}`;
});

const infoRows = computed(() => {
  const info = runtime.value;
  const rows = [
    {
      key: "version",
      label: t("setting.aboutVersion"),
      value: info?.appVersion ?? appInfo.version,
    },
    { key: "runtime", label: t("setting.aboutRuntime"), value: runtimeDetail.value },
    { key: "os", label: t("setting.aboutSystem"), value: info?.os || unknown.value },
    { key: "engine", label: t("setting.aboutEngine"), value: info?.engine || unknown.value },
  ];
  if (info?.arch) {
    rows.splice(3, 0, { key: "arch", label: t("setting.aboutArch"), value: info.arch });
  }
  return rows;
});

const jumpUrl = (url: string) => {
  if (url) void openExternalUrl(url);
};

const copyRuntimeInfo = async () => {
  const text = [`${siteTitle} v${runtime.value?.appVersion ?? appInfo.version}`]
    .concat(infoRows.value.slice(1).map((row) => `${row.label}: ${row.value}`))
    .join("\n");
  try {
    await navigator.clipboard.writeText(text);
    $message?.success(t("setting.aboutCopied"));
  } catch {
    $message?.error(t("setting.aboutCopyFailed"));
  }
};

onMounted(async () => {
  runtime.value = await getRuntimeInfo();
});
</script>

<style lang="scss" scoped>
.settings-about {
  display: flex;
  flex-direction: column;
  gap: 14px;
  width: 100%;
}

.identity {
  display: flex;
  align-items: center;
  gap: 14px;

  .logo {
    flex: 0 0 auto;
    width: 54px;
    height: 54px;
    border-radius: var(--radius-md);
    object-fit: cover;
  }

  .meta {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .title-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;

    .name {
      font-size: 20px;
      font-weight: 700;
      line-height: 1.2;
    }
  }

  .desc {
    font-size: 12px;
    line-height: 1.5;
    opacity: 0.68;
  }
}

.runtime-info {
  width: 100%;

  .value {
    font-variant-numeric: tabular-nums;
  }

  :deep(.n-descriptions-table-wrapper) {
    border-radius: var(--radius-sm);
    overflow: hidden;
  }
}

.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.copyright {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 2px 4px;
  font-size: 12px;
  opacity: 0.66;

  a {
    text-decoration: none;
  }

  .point {
    opacity: 0.5;
  }
}

@media (max-width: 640px) {
  .identity {
    align-items: flex-start;

    .logo {
      width: 46px;
      height: 46px;
    }
  }

  .actions {
    .n-button {
      flex: 1 1 auto;
    }
  }
}
</style>
