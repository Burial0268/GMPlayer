<template>
  <div class="tags-pane">
    <n-alert class="notice" type="info" :bordered="false" :show-icon="false">
      {{ $t("local.detail.overrideNotice") }}
    </n-alert>

    <div class="cover-row">
      <n-image class="thumb" preview-disabled :src="coverSrc" :fallback-src="DEFAULT_COVER" />
      <div class="cover-meta">
        <div class="cover-status">
          <n-tag round :bordered="false" :type="coverTagType">{{ coverLabel }}</n-tag>
          <n-text v-if="detail.cover.imported" class="origin" :depth="3">
            {{ detail.cover.imported.originalName || detail.cover.imported.file }}
          </n-text>
        </div>
        <n-space :size="8">
          <n-button strong secondary round size="small" :loading="coverBusy" @click="pickCover">
            <template #icon>
              <n-icon :component="Pic" />
            </template>
            {{ $t("local.detail.cover.replace") }}
          </n-button>
          <n-button
            strong
            secondary
            round
            size="small"
            :disabled="!detail.cover.imported || coverBusy"
            @click="clearCover"
          >
            {{ $t("local.detail.cover.clear") }}
          </n-button>
        </n-space>
        <!-- Inline rather than behind a confirm dialog: it has to qualify both
             buttons above, and a checkbox the user can see before pressing is
             plainer than one that appears after. -->
        <n-checkbox v-if="albumSiblings > 0" v-model:checked="applyToAlbum" size="small">
          {{ $t("local.detail.cover.applyToAlbum", { count: albumSiblings }) }}
        </n-checkbox>
      </div>
    </div>

    <n-form class="form" label-placement="left" :label-width="92" size="small">
      <n-form-item :label="$t('local.detail.field.title')">
        <n-input v-model:value="form.title" clearable :placeholder="scanned.title || '—'" />
      </n-form-item>
      <n-form-item :label="$t('local.detail.field.artist')">
        <n-input v-model:value="form.artist" clearable :placeholder="scanned.artist || '—'" />
      </n-form-item>
      <n-form-item :label="$t('local.detail.field.album')">
        <n-input v-model:value="form.album" clearable :placeholder="scanned.album || '—'" />
      </n-form-item>
      <n-form-item :label="$t('local.detail.field.albumArtist')">
        <n-input
          v-model:value="form.albumArtist"
          clearable
          :placeholder="scanned.albumArtist || '—'"
        />
      </n-form-item>
      <div class="numbers">
        <n-form-item :label="$t('local.detail.field.trackNo')">
          <n-input-number v-model:value="form.trackNo" clearable :min="0" :precision="0" />
        </n-form-item>
        <n-form-item :label="$t('local.detail.field.discNo')">
          <n-input-number v-model:value="form.discNo" clearable :min="0" :precision="0" />
        </n-form-item>
        <n-form-item :label="$t('local.detail.field.year')">
          <n-input-number v-model:value="form.year" clearable :min="0" :precision="0" />
        </n-form-item>
      </div>
    </n-form>

    <n-space class="actions">
      <n-button strong secondary round type="primary" :loading="saving" @click="save">
        {{ $t("local.detail.save") }}
      </n-button>
      <n-button strong secondary round :disabled="!detail.patch || saving" @click="revert">
        {{ $t("local.detail.revert") }}
      </n-button>
      <n-button strong secondary round :loading="reprobing" @click="reprobe">
        <template #icon>
          <n-icon :component="Refresh" />
        </template>
        {{ $t("local.detail.reread") }}
      </n-button>
    </n-space>

    <n-text class="hint" :depth="3">{{ $t("local.detail.rereadHint") }}</n-text>
  </div>
</template>

<script setup lang="ts">
import { Pic, Refresh } from "@icon-park/vue-next";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useI18n } from "vue-i18n";
import { DEFAULT_COVER } from "@/utils/coverUrl";
import {
  localCoverClear,
  localCoverImport,
  localTrackOverrideSet,
  localTrackReprobe,
  type LocalTrackDetail,
  type TrackOverride,
} from "@/utils/localLibrary";

/**
 * The tag-correction pane.
 *
 * **Nothing here writes to the audio file.** A correction is an override stored
 * beside the playlists and favourites, layered over the scanned row on the way
 * out — so it survives a re-scan, cannot corrupt the only copy of a song, and
 * needs no write permission (Android grants us `READ` on a SAF tree and nothing
 * more). The trade is that another player will not see it, which the notice at
 * the top says out loud.
 *
 * The form holds the *override*, not the effective value: an empty box means "use
 * the file's value" and shows what that is as its placeholder. That is what makes
 * clearing a field and reverting the same gesture, with no third state to explain.
 */
const props = defineProps<{ detail: LocalTrackDetail }>();
const emit = defineEmits<{ updated: [] }>();

const { t } = useI18n();

const scanned = computed(() => props.detail.scanned);

interface FormState {
  title: string;
  artist: string;
  album: string;
  albumArtist: string;
  trackNo: number | null;
  discNo: number | null;
  year: number | null;
}

const form = ref<FormState>(fromPatch(props.detail.patch));
const saving = ref(false);
const reprobing = ref(false);

// ── 封面 ─────────────────────────────────────────────────────────
const coverBusy = ref(false);
const applyToAlbum = ref(false);

/** Other tracks the bulk apply would reach. */
const albumSiblings = computed(() => Math.max(0, props.detail.cover.albumTrackCount - 1));

/**
 * The cover as it is *now* — the import if there is one, otherwise the file's own.
 *
 * Read off the view, because Rust already layered the import onto `coverKey`;
 * this pane never has to decide which one wins.
 */
const coverSrc = computed(() =>
  props.detail.view.coverPath ? convertFileSrc(props.detail.view.coverPath) : DEFAULT_COVER,
);

const coverLabel = computed(() => {
  if (props.detail.cover.imported) return t("local.detail.cover.fromImport");
  if (props.detail.cover.hasEmbedded) return t("local.detail.cover.fromEmbedded");
  return t("local.detail.cover.none");
});

const coverTagType = computed(() =>
  props.detail.cover.imported || props.detail.cover.hasEmbedded ? "success" : "default",
);

/** Matches Rust's `MAX_COVER_UPLOAD_BYTES`, so the message can name the limit. */
const MAX_COVER_BYTES = 12 * 1024 * 1024;

function fromPatch(patch: TrackOverride | null): FormState {
  return {
    title: patch?.title ?? "",
    artist: patch?.artist ?? "",
    album: patch?.album ?? "",
    albumArtist: patch?.albumArtist ?? "",
    trackNo: patch?.trackNo ?? null,
    discNo: patch?.discNo ?? null,
    year: patch?.year ?? null,
  };
}

// Re-seed when the parent reloads (a save, a re-read, or a different track).
watch(
  () => props.detail,
  (next) => {
    form.value = fromPatch(next.patch);
  },
);

/** Blank means "no override". Rust normalizes too, but keeping the wire clean
 * means the stored object matches what the form shows. */
const toPatch = (): TrackOverride => {
  const trimmed = (value: string) => {
    const text = value.trim();
    return text ? text : null;
  };
  return {
    title: trimmed(form.value.title),
    artist: trimmed(form.value.artist),
    album: trimmed(form.value.album),
    albumArtist: trimmed(form.value.albumArtist),
    trackNo: form.value.trackNo ?? null,
    discNo: form.value.discNo ?? null,
    year: form.value.year ?? null,
  };
};

const save = async () => {
  saving.value = true;
  try {
    await localTrackOverrideSet(props.detail.view.key, toPatch());
    $message.success(t("local.detail.saved"));
    emit("updated");
  } catch (err) {
    console.error("[LocalSong] could not save the override:", err);
    $message.error(t("local.detail.saveFailed"));
  } finally {
    saving.value = false;
  }
};

const revert = async () => {
  saving.value = true;
  try {
    // An empty patch *is* the revert — see the module comment.
    await localTrackOverrideSet(props.detail.view.key, {});
    $message.success(t("local.detail.reverted"));
    emit("updated");
  } catch (err) {
    console.error("[LocalSong] could not revert the override:", err);
    $message.error(t("local.detail.saveFailed"));
  } finally {
    saving.value = false;
  }
};

/**
 * Read a picked file as the base64 payload Rust expects.
 *
 * `readAsDataURL` rather than `readAsArrayBuffer` because the alternative is
 * hand-rolling base64 over a `Uint8Array`, and the data URL already carries
 * exactly the encoding the command takes — minus its `data:…;base64,` prefix.
 */
const readAsBase64 = (file: File): Promise<string> =>
  new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error ?? new Error("could not read the picture"));
    reader.onload = () => {
      const result = String(reader.result);
      const comma = result.indexOf(",");
      resolve(comma >= 0 ? result.slice(comma + 1) : result);
    };
    reader.readAsDataURL(file);
  });

/**
 * Pick a picture and store it as this track's cover.
 *
 * A plain `<input type="file">` rather than the Rust picker: the vendored
 * `local-files` plugin only offers a text picker and an audio one, so an image
 * entry there would mean a Kotlin change, while this works unchanged on desktop
 * WebView2, the Android WebView and the web build.
 */
const pickCover = () => {
  const input = document.createElement("input");
  input.type = "file";
  input.accept = "image/*";
  input.onchange = async () => {
    const file = input.files?.[0];
    if (!file) return;
    if (file.size > MAX_COVER_BYTES) {
      $message.error(t("local.detail.cover.tooLarge"));
      return;
    }

    coverBusy.value = true;
    const key = props.detail.view.key;
    try {
      const dataBase64 = await readAsBase64(file);
      // The pane may have been handed a different track while the read was out.
      if (key !== props.detail.view.key) return;
      const result = await localCoverImport(
        key,
        dataBase64,
        file.type,
        file.name,
        applyToAlbum.value,
      );
      $message.success(
        result.applied.length > 1
          ? t("local.detail.cover.importedMany", { count: result.applied.length })
          : t("local.detail.cover.imported"),
      );
      emit("updated");
    } catch (err) {
      console.error("[LocalSong] cover import failed:", err);
      $message.error(t("local.detail.cover.importFailed"));
    } finally {
      coverBusy.value = false;
    }
  };
  input.click();
};

const clearCover = async () => {
  coverBusy.value = true;
  try {
    const cleared = await localCoverClear(props.detail.view.key, applyToAlbum.value);
    if (!cleared.length) return;
    $message.success(t("local.detail.cover.cleared"));
    emit("updated");
  } catch (err) {
    console.error("[LocalSong] could not clear the cover:", err);
    $message.error(t("local.detail.cover.importFailed"));
  } finally {
    coverBusy.value = false;
  }
};

const reprobe = async () => {
  reprobing.value = true;
  try {
    const row = await localTrackReprobe(props.detail.view.key);
    if (!row) {
      $message.error(t("local.detail.rereadFailed"));
      return;
    }
    $message.success(t("local.detail.rereadDone"));
    emit("updated");
  } catch (err) {
    console.error("[LocalSong] could not re-read the file:", err);
    $message.error(t("local.detail.rereadFailed"));
  } finally {
    reprobing.value = false;
  }
};
</script>

<style lang="scss" scoped>
.tags-pane {
  display: flex;
  flex-direction: column;
  max-width: 620px;

  .notice {
    margin-bottom: 14px;
    border-radius: var(--radius-md);
  }

  .cover-row {
    display: flex;
    align-items: flex-start;
    gap: 14px;
    margin-bottom: 16px;

    .thumb {
      flex: 0 0 auto;
      width: 92px;
      height: 92px;
      border-radius: var(--radius-md);
      overflow: hidden;
      background-color: color-mix(in srgb, var(--n-text-color) 4%, transparent);

      :deep(img) {
        width: 100%;
        height: 100%;
        object-fit: cover;
      }
    }

    .cover-meta {
      display: flex;
      flex-direction: column;
      align-items: flex-start;
      gap: 8px;
      min-width: 0;
    }

    .cover-status {
      display: flex;
      align-items: center;
      gap: 8px;
      min-width: 0;

      .origin {
        font-size: 12px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
    }
  }

  .form {
    :deep(.n-form-item) {
      margin-bottom: 4px;
    }
  }

  .numbers {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 0 14px;
  }

  .actions {
    margin-top: 10px;
  }

  .hint {
    margin-top: 10px;
    font-size: 12px;
  }
}
</style>
