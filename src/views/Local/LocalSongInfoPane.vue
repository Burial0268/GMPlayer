<template>
  <div class="info-pane">
    <section class="group">
      <h4 class="group-title">{{ $t("local.detail.group.tags") }}</h4>
      <div class="rows">
        <div class="row" v-for="row in tagRows" :key="row.label">
          <n-text class="label" :depth="3">{{ row.label }}</n-text>
          <n-text class="value">{{ row.value || "—" }}</n-text>
          <!-- 只在这一项真的被覆盖时出现：整页一个「已编辑」徽标说不清是哪一栏。 -->
          <n-tag v-if="row.overridden" class="badge" size="small" round :bordered="false">
            {{ $t("local.detail.overridden") }}
          </n-tag>
        </div>
      </div>
    </section>

    <section class="group">
      <h4 class="group-title">{{ $t("local.detail.group.audio") }}</h4>
      <div class="rows">
        <div class="row" v-for="row in audioRows" :key="row.label">
          <n-text class="label" :depth="3">{{ row.label }}</n-text>
          <n-text class="value">{{ row.value || "—" }}</n-text>
        </div>
      </div>
    </section>

    <section class="group">
      <h4 class="group-title">{{ $t("local.detail.group.file") }}</h4>
      <div class="rows">
        <div class="row" v-for="row in fileRows" :key="row.label">
          <n-text class="label" :depth="3">{{ row.label }}</n-text>
          <!-- 路径可选中复制：这是这一页唯一一处用户真的想复制走的文本。 -->
          <n-text class="value selectable">{{ row.value || "—" }}</n-text>
        </div>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";
import type { LocalTrackDetail } from "@/utils/localLibrary";
import { getSongTime } from "@/utils/timeTools";

/**
 * Read-only "what is this file" pane.
 *
 * Three groups because the questions are three: what the tags say (and which of
 * them you changed), what the audio is, and where the file lives. The tag group
 * marks each overridden field individually — a single page-level badge cannot
 * answer "so which one did I edit".
 */
const props = defineProps<{ detail: LocalTrackDetail }>();

const { t } = useI18n();

const view = computed(() => props.detail.view);
const patch = computed(() => props.detail.patch);

const overridden = (field: keyof NonNullable<LocalTrackDetail["patch"]>): boolean => {
  const value = patch.value?.[field];
  return value !== undefined && value !== null;
};

const tagRows = computed(() => [
  {
    label: t("local.detail.field.title"),
    value: view.value.title,
    overridden: overridden("title"),
  },
  {
    label: t("local.detail.field.artist"),
    value: view.value.artist,
    overridden: overridden("artist"),
  },
  {
    label: t("local.detail.field.album"),
    value: view.value.album,
    overridden: overridden("album"),
  },
  {
    label: t("local.detail.field.albumArtist"),
    value: view.value.albumArtist,
    overridden: overridden("albumArtist"),
  },
  {
    label: t("local.detail.field.trackNo"),
    value: view.value.trackNo !== null ? String(view.value.trackNo) : "",
    overridden: overridden("trackNo"),
  },
  {
    label: t("local.detail.field.discNo"),
    value: view.value.discNo !== null ? String(view.value.discNo) : "",
    overridden: overridden("discNo"),
  },
  {
    label: t("local.detail.field.year"),
    value: view.value.year !== null ? String(view.value.year) : "",
    overridden: overridden("year"),
  },
]);

const audioRows = computed(() => {
  const row = view.value;
  return [
    { label: t("local.detail.field.duration"), value: getSongTime(row.durationMs) },
    { label: t("local.detail.field.codec"), value: row.codec?.toUpperCase() ?? "" },
    {
      label: t("local.detail.field.sampleRate"),
      value: row.sampleRate ? `${row.sampleRate} Hz` : "",
    },
    { label: t("local.detail.field.channels"), value: row.channels ? String(row.channels) : "" },
    {
      label: t("local.detail.field.bitrate"),
      value: row.bitrateBps ? `${Math.round(row.bitrateBps / 1000)} kbps` : "",
    },
    // Only shown when true: "no" is the normal case and saying so is noise.
    ...(row.needsCache
      ? [{ label: t("local.detail.field.needsCache"), value: t("local.detail.needsCacheYes") }]
      : []),
  ];
});

const fileRows = computed(() => {
  const row = view.value;
  return [
    { label: t("local.detail.field.fileName"), value: row.displayName },
    { label: t("local.detail.field.location"), value: row.key },
    { label: t("local.detail.field.source"), value: props.detail.sourceName },
    { label: t("local.detail.field.folder"), value: row.relativeDir },
    { label: t("local.detail.field.size"), value: formatSize(row.size) },
    { label: t("local.detail.field.modified"), value: formatTime(row.modifiedAt) },
  ];
});

const formatSize = (bytes: number | null): string => {
  if (!bytes) return "";
  const mb = bytes / 1024 / 1024;
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;
};

const formatTime = (seconds: number | null): string =>
  seconds ? new Date(seconds * 1000).toLocaleString() : "";
</script>

<style lang="scss" scoped>
.info-pane {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: 18px 26px;

  .group-title {
    margin: 0 0 10px;
    font-size: 14px;
    font-weight: 800;
    color: var(--n-text-color-2);
  }

  .rows {
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .row {
    display: flex;
    align-items: baseline;
    gap: 12px;
    padding: 9px 12px;
    font-size: 13px;

    &:nth-child(odd) {
      background-color: color-mix(in srgb, var(--n-text-color) 3%, transparent);
    }

    &:nth-child(even) {
      background-color: color-mix(in srgb, var(--n-text-color) 6%, transparent);
    }

    .label {
      flex: 0 0 88px;
      font-size: 12px;
    }

    .value {
      flex: 1;
      min-width: 0;
      overflow-wrap: anywhere;
    }

    .selectable {
      -webkit-user-select: text;
      user-select: text;
    }

    .badge {
      flex: 0 0 auto;
    }
  }
}
</style>
