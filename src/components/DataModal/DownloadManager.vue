<template>
  <n-modal
    class="download-manager s-modal"
    v-model:show="show"
    preset="card"
    :bordered="false"
    style="max-width: 620px"
  >
    <template #header>{{ $t("download.manager") }}</template>

    <n-space vertical :size="16">
      <div class="destination">
        <div class="text">
          <n-text class="key">{{ $t("download.destination") }}</n-text>
          <n-text class="value" :depth="3">{{ destination }}</n-text>
        </div>
        <n-space :size="8">
          <n-button v-if="revealable" quaternary size="small" @click="reveal">
            <template #icon>
              <n-icon :component="FolderOpen" />
            </template>
            {{ $t("download.reveal") }}
          </n-button>
          <n-button strong secondary round size="small" @click="pick">
            {{ $t("download.change") }}
          </n-button>
        </n-space>
      </div>

      <n-empty v-if="!download.tasks.length" :description="$t('download.empty')" />
      <template v-else>
        <n-list class="task-list" hoverable>
          <n-list-item v-for="task in download.tasks" :key="task.id">
            <n-thing>
              <template #header>
                <n-space :size="8" align="center">
                  <n-text>{{ task.title || $t("general.name.unknownSong") }}</n-text>
                  <n-tag size="small" :type="tagType(task)" :bordered="false">
                    {{ $t(`download.status.${task.status}`) }}
                  </n-tag>
                </n-space>
              </template>
              <template #description>
                <n-text class="meta" :depth="3">
                  {{ task.artist || $t("general.name.unknownArtist") }}
                  <template v-if="task.fileName"> · {{ task.fileName }}</template>
                </n-text>
                <!--
                  Indeterminate when the server declared no size: a chunked
                  response has no total, and a bar pinned at 0% reads as stuck.
                -->
                <n-progress
                  v-if="inFlight(task)"
                  class="bar"
                  type="line"
                  :percentage="percentage(task)"
                  :indeterminate="!task.total"
                  :processing="task.status === 'downloading'"
                  :height="4"
                  :show-indicator="!!task.total"
                />
                <n-text v-if="task.error" class="meta error" :depth="3">{{ task.error }}</n-text>
              </template>
            </n-thing>
            <template #suffix>
              <n-space :size="4">
                <n-button
                  v-if="task.status === 'downloading' || task.status === 'queued'"
                  quaternary
                  circle
                  :title="$t('download.pause')"
                  @click="download.pause(task.id)"
                >
                  <template #icon>
                    <n-icon :component="PauseOne" />
                  </template>
                </n-button>
                <n-button
                  v-else-if="task.status === 'paused'"
                  quaternary
                  circle
                  :title="$t('download.resume')"
                  @click="download.resume(task.id)"
                >
                  <template #icon>
                    <n-icon :component="PlayOne" />
                  </template>
                </n-button>
                <n-button
                  v-else-if="task.status === 'failed'"
                  quaternary
                  circle
                  :title="$t('download.retry')"
                  @click="download.retry(task.id)"
                >
                  <template #icon>
                    <n-icon :component="Refresh" />
                  </template>
                </n-button>
                <n-button
                  quaternary
                  circle
                  :title="$t('download.remove')"
                  @click="download.remove(task.id)"
                >
                  <template #icon>
                    <n-icon :component="DeleteFour" />
                  </template>
                </n-button>
              </n-space>
            </template>
          </n-list-item>
        </n-list>
        <n-space justify="end">
          <n-button text :disabled="!download.finished.length" @click="download.clearFinished()">
            {{ $t("download.clearFinished") }}
          </n-button>
        </n-space>
      </template>
    </n-space>
  </n-modal>
</template>

<script setup lang="ts">
import { DeleteFour, FolderOpen, PauseOne, PlayOne, Refresh } from "@icon-park/vue-next";
import { useI18n } from "vue-i18n";
import { useDownloadStore } from "@/store";
import { dirIsRevealable, dirLabelOf, type DownloadTask } from "@/utils/download";

const { t } = useI18n();
const download = useDownloadStore();

const show = ref(false);

const open = () => {
  show.value = true;
  download.hydrate();
};

defineExpose({ open });

const destination = computed(() => dirLabelOf(download.settings) || t("download.noDestination"));
const revealable = computed(() => dirIsRevealable(download.settings));

const inFlight = (task: DownloadTask): boolean =>
  task.status === "downloading" || task.status === "paused" || task.status === "resolving";

const percentage = (task: DownloadTask): number => {
  if (!task.total) return 0;
  return Math.min(100, Math.round((task.received / task.total) * 100));
};

const tagType = (task: DownloadTask): "default" | "success" | "warning" | "error" | "info" => {
  switch (task.status) {
    case "done":
      return "success";
    case "skipped":
      return "info";
    case "failed":
      return "error";
    case "paused":
      return "warning";
    default:
      return "default";
  }
};

const pick = async () => {
  try {
    await download.pick();
  } catch (error) {
    $message.error(String(error));
  }
};

/**
 * 只在桌面可用。Android 的下载目录是一份 SAF 授权，`content://` 文档没有可以交给
 * 系统文件管理器的路径 —— 和 `DataLists.vue` 里「在文件夹中显示」同一条限制。
 */
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
</script>

<style lang="scss" scoped>
.download-manager {
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
  .task-list {
    background-color: transparent;
    max-height: 46vh;
    overflow-y: auto;
    .meta {
      display: block;
      font-size: 12px;
      word-break: break-all;
      &.error {
        color: var(--n-color-error);
      }
    }
    .bar {
      margin-top: 6px;
    }
  }
}
</style>
