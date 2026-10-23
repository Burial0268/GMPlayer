<template>
  <n-modal
    class="import-local-music s-modal"
    v-model:show="show"
    preset="card"
    :bordered="false"
    style="max-width: 560px"
  >
    <template #header>{{ $t("local.manageSources") }}</template>

    <n-space vertical :size="16">
      <n-space :size="10">
        <n-button
          strong
          secondary
          round
          type="primary"
          :disabled="local.scan.active"
          @click="addFolder"
        >
          <template #icon>
            <n-icon :component="FolderOpen" />
          </template>
          {{ $t("local.addFolder") }}
        </n-button>
        <!--
          Deliberately equal in weight to "add folder": importing one song is a
          first-class action, not a fallback. It is the *directory* that costs
          less on Android (one persisted grant covers a whole tree), which is a
          platform detail and not a reason to bury single-file import.
        -->
        <n-button strong secondary round :disabled="local.scan.active" @click="addFiles">
          <template #icon>
            <n-icon :component="FileMusic" />
          </template>
          {{ $t("local.addFiles") }}
        </n-button>
      </n-space>

      <!--
        Progress is shown for the *tagging* phase, which is the one that takes
        time; listing a tree reports a single indeterminate tick because neither
        `walkdir` nor SAF can say how many files there are before walking them.
      -->
      <div v-if="local.scan.active" class="progress">
        <n-progress
          type="line"
          :percentage="percentage"
          :indeterminate="local.scan.total === 0"
          :processing="true"
        />
        <div class="progress-meta">
          <n-text class="current" :depth="3">
            {{ local.scan.current || $t("local.scanning") }}
          </n-text>
          <n-button text type="error" @click="local.cancelScan()">
            {{ $t("general.dialog.cancel") }}
          </n-button>
        </div>
      </div>

      <n-alert v-if="local.scan.error" type="error" :show-icon="false">
        {{ local.scan.error }}
      </n-alert>

      <n-empty v-if="!local.sources.length" :description="$t('local.noSources')" />
      <n-list v-else class="source-list" hoverable>
        <n-list-item v-for="source in local.sources" :key="source.id">
          <n-thing>
            <template #header>
              <n-space :size="8" align="center">
                <n-text>{{ sourceLabel(source) }}</n-text>
                <!--
                  The download folder is an ordinary directory source, so it shows
                  up here like any other — but deleting it does not stop downloads
                  going there, and the next one re-creates the row. Say so rather
                  than letting the button imply otherwise.
                -->
                <n-tag v-if="isDownloadDir(source)" size="small" type="info" :bordered="false">
                  {{ $t("local.downloadSource") }}
                </n-tag>
                <!--
                  A source whose grant or mount is gone is marked, never removed:
                  an SD card that was out at launch, or a revoked SAF permission,
                  comes back — and deleting the rows would take the playlists that
                  reference them with it.
                -->
                <n-tag v-if="!source.available" size="small" type="warning" :bordered="false">
                  {{ $t("local.unavailable") }}
                </n-tag>
              </n-space>
            </template>
            <template #description>
              <n-text class="locator" :depth="3">{{ displayLocator(source) }}</n-text>
              <n-text class="stats" :depth="3">
                {{ $t("local.trackCount", { count: source.trackCount }) }}
                <template v-if="source.lastScannedAt">
                  · {{ $t("local.lastScanned", { time: scannedAt(source.lastScannedAt) }) }}
                </template>
              </n-text>
            </template>
          </n-thing>
          <template #suffix>
            <n-space :size="4">
              <n-button
                quaternary
                circle
                :disabled="local.scan.active"
                :title="$t('local.rescan')"
                @click="rescan(source)"
              >
                <template #icon>
                  <n-icon :component="Refresh" />
                </template>
              </n-button>
              <n-button
                quaternary
                circle
                :disabled="local.scan.active"
                :title="$t('local.removeSource')"
                @click="confirmRemove(source)"
              >
                <template #icon>
                  <n-icon :component="DeleteFour" />
                </template>
              </n-button>
            </n-space>
          </template>
        </n-list-item>
      </n-list>
    </n-space>
  </n-modal>
</template>

<script setup lang="ts">
import { DeleteFour, FileMusic, FolderOpen, Refresh } from "@icon-park/vue-next";
import { useI18n } from "vue-i18n";
import { useDownloadStore, useLocalLibraryStore } from "@/store";
import type { LocalScanSummary, LocalSource } from "@/utils/localLibrary";

const { t } = useI18n();
const local = useLocalLibraryStore();
const download = useDownloadStore();

const show = ref(false);

const percentage = computed(() => {
  const { done, total } = local.scan;
  if (!total) return 0;
  return Math.min(100, Math.round((done / total) * 100));
});

const open = () => {
  show.value = true;
  local.hydrate();
  // Cheap and idempotent; needed only to know which row is the download folder.
  download.hydrate();
};

defineExpose({ open });

/** Whether this row is the folder the download queue writes into. */
const isDownloadDir = (source: LocalSource): boolean => {
  const dir = download.settings?.dir;
  return !!dir && source.locator === dir;
};

/**
 * The source's name.
 *
 * A `files` source is the synthetic bag that holds individually-picked songs; its
 * locator is the sentinel `local:picked-files`, which must never reach the screen.
 */
const sourceLabel = (source: LocalSource): string => {
  if (source.kind === "files") return t("local.pickedFiles");
  return source.displayName || source.locator;
};

/**
 * What to show as the source's location.
 *
 * A `content://` tree URI is a provider authority plus a percent-encoded document
 * id — noise on screen — and a `files` source has no single location at all, so
 * it reports how many songs it holds instead.
 */
const displayLocator = (source: LocalSource): string => {
  if (source.kind === "files") {
    return t("local.pickedFilesTip", { count: source.members.length });
  }
  return source.locator.startsWith("content://") ? t("local.safSource") : source.locator;
};

const scannedAt = (unixSeconds: number): string => new Date(unixSeconds * 1000).toLocaleString();

const report = (summary: LocalScanSummary | null) => {
  if (!summary) return;
  if (summary.truncated) $message.warning(t("local.scanTruncated"));
  $message.success(
    t("local.scanDone", {
      added: summary.added,
      updated: summary.updated,
      removed: summary.removed,
      failed: summary.failed,
    }),
  );
};

const addFolder = async () => {
  try {
    report(await local.importDirectory());
  } catch (err) {
    $message.error(String(err));
  }
};

const addFiles = async () => {
  try {
    report(await local.importFiles());
  } catch (err) {
    $message.error(String(err));
  }
};

const rescan = async (source: LocalSource) => {
  try {
    report(await local.rescan(source.id));
  } catch (err) {
    $message.error(String(err));
  }
};

const confirmRemove = (source: LocalSource) => {
  $dialog.warning({
    class: "s-dialog",
    title: t("local.removeSource"),
    // Spelled out because it is the one destructive action here, and the thing
    // people fear (losing their playlists) is exactly what does *not* happen:
    // playlists and favourites are keyed on file locators, so re-importing the
    // same folder restores them.
    content: t("local.removeSourceQuestion", {
      name: sourceLabel(source),
    }),
    positiveText: t("general.dialog.confirm"),
    negativeText: t("general.dialog.cancel"),
    onPositiveClick: async () => {
      try {
        await local.removeSource(source.id);
      } catch (err) {
        $message.error(String(err));
      }
    },
  });
};
</script>

<style lang="scss" scoped>
.import-local-music {
  .progress {
    .progress-meta {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      margin-top: 6px;
      .current {
        font-size: 12px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
    }
  }
  .source-list {
    background-color: transparent;
    max-height: 42vh;
    overflow-y: auto;
    .locator,
    .stats {
      display: block;
      font-size: 12px;
      word-break: break-all;
    }
  }
}
</style>
