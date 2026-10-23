<template>
  <div class="lyric-pane">
    <div class="status">
      <n-tag round :bordered="false" :type="sourceTagType">{{ sourceLabel }}</n-tag>
      <n-text v-if="detail.lyric.imported" class="origin" :depth="3">
        {{ detail.lyric.imported.originalName || detail.lyric.imported.file }}
      </n-text>
      <n-text v-else-if="detail.lyric.hasSidecar" class="origin" :depth="3">
        {{ $t("local.detail.lyric.sidecarFound") }}
      </n-text>
    </div>

    <n-space class="actions">
      <n-button strong secondary round type="primary" @click="openSearch">
        <template #icon>
          <n-icon :component="Search" />
        </template>
        {{ $t("local.detail.lyric.searchAmll") }}
      </n-button>
      <n-button v-if="canPickFile" strong secondary round :loading="busy" @click="importFile">
        <template #icon>
          <n-icon :component="FolderOpen" />
        </template>
        {{ $t("local.detail.lyric.importFile") }}
      </n-button>
      <n-button strong secondary round @click="pasteShow = true">
        <template #icon>
          <n-icon :component="Clipboard" />
        </template>
        {{ $t("local.detail.lyric.paste") }}
      </n-button>
      <n-button
        strong
        secondary
        round
        :disabled="!detail.lyric.imported || busy"
        @click="clearImport"
      >
        <template #icon>
          <n-icon :component="Delete" />
        </template>
        {{ $t("local.detail.lyric.clear") }}
      </n-button>
    </n-space>

    <n-text class="formats" :depth="3">{{ $t("local.detail.lyric.formats") }}</n-text>

    <div class="preview">
      <n-spin v-if="loading" :size="16" />
      <n-empty v-else-if="!preview" :description="$t('local.detail.lyric.none')" size="small" />
      <n-scrollbar v-else class="scroll">
        <pre class="text">{{ preview }}</pre>
      </n-scrollbar>
    </div>

    <!-- 粘贴导入。跟 DataModal 下的弹窗一个风格（`s-modal`）。 -->
    <n-modal
      class="s-modal"
      v-model:show="pasteShow"
      preset="card"
      :title="$t('local.detail.lyric.paste')"
      :bordered="false"
    >
      <n-input
        v-model:value="pasteText"
        type="textarea"
        :rows="12"
        :placeholder="$t('local.detail.lyric.pastePlaceholder')"
      />
      <template #footer>
        <n-space justify="end">
          <n-button strong secondary round @click="pasteShow = false">
            {{ $t("general.dialog.cancel") }}
          </n-button>
          <n-button
            strong
            secondary
            round
            type="primary"
            :disabled="!pasteText.trim()"
            :loading="busy"
            @click="importText"
          >
            {{ $t("local.detail.lyric.importPasted") }}
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- AMLL 词库搜索。本地文件没有网易云 id，只能按标签找，所以标题/艺术家都可改。 -->
    <n-modal
      class="s-modal"
      v-model:show="searchShow"
      preset="card"
      :title="$t('local.detail.lyric.searchAmll')"
      :bordered="false"
    >
      <div class="search-form">
        <n-input
          v-model:value="searchTitle"
          :placeholder="$t('local.detail.lyric.searchKeywordTitle')"
          clearable
          @update:value="scheduleSearch"
        />
        <n-input
          v-model:value="searchArtist"
          :placeholder="$t('local.detail.lyric.searchKeywordArtist')"
          clearable
          @update:value="scheduleSearch"
        />
        <n-button
          strong
          secondary
          round
          :loading="autoBusy"
          :disabled="searching"
          @click="autoMatch"
        >
          {{ $t("local.detail.lyric.autoMatch") }}
        </n-button>
      </div>

      <div class="results">
        <n-spin v-if="searching" :size="16" />
        <n-empty v-else-if="!results.length" :description="emptyText" size="small" />
        <n-scrollbar v-else class="scroll">
          <div
            v-for="row in results"
            :key="row.id"
            class="row"
            :class="{ busy: importingId !== null }"
            @click="importFromAmll(row)"
          >
            <div class="name">{{ row.musicNames.join(" / ") }}</div>
            <div class="meta">{{ rowMeta(row) }}</div>
            <div v-if="row.authorUsernames.length" class="authors">
              {{
                $t("local.detail.lyric.contributedBy", { names: row.authorUsernames.join("、") })
              }}
            </div>
          </div>
        </n-scrollbar>
      </div>

      <template #footer>
        <n-space justify="end">
          <n-text v-if="totalText" class="total" :depth="3">{{ totalText }}</n-text>
          <n-button strong secondary round @click="searchShow = false">
            {{ $t("general.dialog.cancel") }}
          </n-button>
        </n-space>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { Clipboard, Delete, FolderOpen, Search } from "@icon-park/vue-next";
import { useI18n } from "vue-i18n";
import {
  amllPrimaryArtist,
  getAmllLyricById,
  searchAmllLyrics,
  type AmllFailure,
  type AmllOutcome,
  type AmllSearchResult,
  type AmllSongItem,
} from "@/utils/amllTtmlApi";
import {
  localLyricClear,
  localLyricFor,
  localLyricImportFile,
  localLyricImportText,
  type LocalLyric,
  type LocalTrackDetail,
} from "@/utils/localLibrary";
import { isTauri } from "@/utils/tauri/core/runtime";

/**
 * The lyric pane: where this track's words come from, and how to replace them.
 *
 * Two import routes, because the platforms differ in what they can offer. A file
 * picker is the obvious one; pasting is not a fallback — it is the better path
 * when the lyric came from a web page, and it is the only one that works
 * everywhere. What gets stored is the text either way, under `$APPDATA`, so
 * nothing is ever written next to the user's music.
 *
 * An import outranks the file's own embedded tag and any sibling `.lrc`, which is
 * the order the backend resolves in: a person who went and found a lyric did so
 * *because* what the file carried was wrong or missing.
 */
const props = defineProps<{ detail: LocalTrackDetail }>();
const emit = defineEmits<{ updated: [] }>();

const { t } = useI18n();

const preview = ref("");
const loading = ref(false);
const busy = ref(false);
const pasteShow = ref(false);
const pasteText = ref("");
const resolved = ref<LocalLyric | null>(null);

// ── AMLL 词库搜索 ────────────────────────────────────────────────
const searchShow = ref(false);
const searchTitle = ref("");
const searchArtist = ref("");
const searching = ref(false);
const autoBusy = ref(false);
const results = ref<AmllSongItem[]>([]);
const total = ref(0);
const importingId = ref<number | null>(null);
/** What the last finished search ran into, so the empty state can say which. */
const lastFailure = ref<AmllFailure | null>(null);

/**
 * Own debounce handle, deliberately not `@/utils/debounce`: that module keeps a
 * single module-level timer shared app-wide, so anything else debouncing at the
 * same moment would cancel this box's pending search.
 */
let searchTimer: ReturnType<typeof setTimeout> | null = null;
/** Only the newest search may write `results`; a slow earlier one is dropped. */
let searchGeneration = 0;

const rowMeta = (row: AmllSongItem): string =>
  [row.artistNames.join(" / "), row.albumNames.join(" / ")].filter(Boolean).join(" · ");

const emptyText = computed(() => {
  if (lastFailure.value === "failed") return t("local.detail.lyric.searchFailed");
  if (!searchTitle.value.trim() && !searchArtist.value.trim())
    return t("local.detail.lyric.searchPrompt");
  return t("local.detail.lyric.searchNoResults");
});

const totalText = computed(() =>
  total.value > results.value.length
    ? t("local.detail.lyric.searchTotal", { shown: results.value.length, total: total.value })
    : "",
);

/**
 * Whether a native file picker is reachable.
 *
 * Desktop opens the dialog plugin; Android goes through SAF, both from Rust. iOS
 * has neither wired, so the button is hidden rather than left to fail — the paste
 * route works there.
 */
const canPickFile = computed(() => isTauri() && !isIos());

const isIos = (): boolean =>
  typeof navigator !== "undefined" && /iPad|iPhone|iPod/i.test(navigator.userAgent);

const sourceLabel = computed(() => {
  switch (resolved.value?.source) {
    case "imported":
      return t("local.detail.lyric.fromImport");
    case "embedded":
      return t("local.detail.lyric.fromEmbedded");
    case "sidecar":
      return t("local.detail.lyric.fromSidecar");
    default:
      return t("local.detail.lyric.none");
  }
});

const sourceTagType = computed(() => (resolved.value ? "success" : "default"));

const load = async () => {
  loading.value = true;
  const key = props.detail.view.key;
  try {
    const lyric = await localLyricFor(key);
    // The pane may have been handed a different track while this was in flight.
    if (key !== props.detail.view.key) return;
    resolved.value = lyric;
    preview.value = lyric?.text?.trim() ?? "";
  } catch (err) {
    console.error("[LocalSong] could not read the lyric:", err);
    resolved.value = null;
    preview.value = "";
  } finally {
    if (key === props.detail.view.key) loading.value = false;
  }
};

const importFile = async () => {
  busy.value = true;
  try {
    const entry = await localLyricImportFile(props.detail.view.key);
    // `null` is a cancelled picker, which is not a failure worth a message.
    if (!entry) return;
    $message.success(t("local.detail.lyric.imported"));
    await load();
    emit("updated");
  } catch (err) {
    console.error("[LocalSong] lyric import failed:", err);
    $message.error(t("local.detail.lyric.importFailed"));
  } finally {
    busy.value = false;
  }
};

const importText = async () => {
  const text = pasteText.value.trim();
  if (!text) return;
  busy.value = true;
  try {
    // No extension to go on: the backend recognises TTML by its first character
    // and the frontend detects the word-timed dialect from the content, so a
    // paste needs no format question asked of the user.
    await localLyricImportText(props.detail.view.key, text);
    $message.success(t("local.detail.lyric.imported"));
    pasteShow.value = false;
    pasteText.value = "";
    await load();
    emit("updated");
  } catch (err) {
    console.error("[LocalSong] lyric paste failed:", err);
    $message.error(t("local.detail.lyric.importFailed"));
  } finally {
    busy.value = false;
  }
};

const clearImport = async () => {
  busy.value = true;
  try {
    await localLyricClear(props.detail.view.key);
    $message.success(t("local.detail.lyric.cleared"));
    await load();
    emit("updated");
  } catch (err) {
    console.error("[LocalSong] could not clear the lyric:", err);
    $message.error(t("local.detail.lyric.importFailed"));
  } finally {
    busy.value = false;
  }
};

/** How many rows one search asks for. */
const PAGE_SIZE = 30;

/**
 * One search, plus the title-only retry.
 *
 * `artistName` is an AND term, so a tag that spells the artist differently from
 * the library — a collaboration, a romanisation, a trailing "(CV. …)" — turns a
 * findable track into zero results. Dropping it is what a person does by hand at
 * this point, and `droppedArtist` lets the caller empty the box so the list on
 * screen matches the query behind it.
 */
const searchWithFallback = async (
  musicName: string,
  artistName: string,
  fallbackToTitle: boolean,
): Promise<{ outcome: AmllOutcome<AmllSearchResult>; droppedArtist: boolean }> => {
  const first = await searchAmllLyrics({ musicName, artistName, pageSize: PAGE_SIZE });
  const worthRetrying =
    fallbackToTitle &&
    !!musicName &&
    !!artistName &&
    first.result === "ok" &&
    !first.data.items.length;
  if (!worthRetrying) return { outcome: first, droppedArtist: false };

  const retry = await searchAmllLyrics({ musicName, pageSize: PAGE_SIZE });
  // Keep the artist-scoped answer when the wider one adds nothing: a failure
  // there says nothing about the library, and reporting it would be misleading.
  if (retry.result === "ok" && retry.data.items.length) {
    return { outcome: retry, droppedArtist: true };
  }
  return { outcome: first, droppedArtist: false };
};

/**
 * Run whatever the two inputs currently describe.
 *
 * Only the prefilled paths ask for the fallback: while the user is typing,
 * silently dropping a term they can see would be a lie.
 *
 * Answers whether it actually wrote `results`, which `autoMatch` needs: a search
 * abandoned mid-flight leaves someone else's rows in place, and importing one of
 * those would put the wrong words on the track.
 */
const runSearch = async (options: { fallbackToTitle?: boolean } = {}): Promise<boolean> => {
  const generation = ++searchGeneration;
  const musicName = searchTitle.value.trim();
  const artistName = searchArtist.value.trim();

  if (!musicName && !artistName) {
    results.value = [];
    total.value = 0;
    lastFailure.value = null;
    return true;
  }

  searching.value = true;
  try {
    const { outcome, droppedArtist } = await searchWithFallback(
      musicName,
      artistName,
      options.fallbackToTitle === true,
    );
    // A newer keystroke already started its own search; this answer is stale.
    if (generation !== searchGeneration) return false;
    if (droppedArtist) searchArtist.value = "";

    if (outcome.result === "ok") {
      results.value = outcome.data.items;
      total.value = outcome.data.pagination.total;
      lastFailure.value = outcome.data.items.length ? null : "missing";
    } else {
      results.value = [];
      total.value = 0;
      lastFailure.value = outcome.result;
    }
    return true;
  } finally {
    if (generation === searchGeneration) searching.value = false;
  }
};

const scheduleSearch = () => {
  if (searchTimer !== null) clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {
    searchTimer = null;
    runSearch();
  }, 350);
};

/**
 * Open prefilled from the file's own tags, which is the answer most of the time.
 *
 * Only the *first* artist is sent — see `amllPrimaryArtist`.
 */
const openSearch = () => {
  searchTitle.value = props.detail.view.title || props.detail.view.displayName;
  searchArtist.value = amllPrimaryArtist(props.detail.view.artist);
  results.value = [];
  total.value = 0;
  lastFailure.value = null;
  searchShow.value = true;
  runSearch({ fallbackToTitle: true });
};

const importFromAmll = async (row: AmllSongItem) => {
  if (importingId.value !== null) return;
  importingId.value = row.id;
  const key = props.detail.view.key;
  try {
    const detail = await getAmllLyricById(row.id);
    if (detail.result !== "ok" || !detail.data.lyrics) {
      $message.error(t("local.detail.lyric.searchFailed"));
      return;
    }
    // The pane may have been handed a different track while the fetch was out.
    if (key !== props.detail.view.key) return;
    // Stored as an import, which outranks the embedded tag and any sibling file.
    // `filename` becomes `originalName`, so the status line names the library
    // file the words came from.
    await localLyricImportText(key, detail.data.lyrics, "ttml", detail.data.filename);
    $message.success(t("local.detail.lyric.imported"));
    searchShow.value = false;
    await load();
    emit("updated");
  } catch (err) {
    console.error("[LocalSong] AMLL lyric import failed:", err);
    $message.error(t("local.detail.lyric.importFailed"));
  } finally {
    importingId.value = null;
  }
};

/**
 * Re-run from the file's tags and take the answer if there is only one.
 *
 * Several hits for one title are routine — the library publishes every revision
 * as its own file — and an import outranks the embedded lyric, so guessing among
 * them could hide words that were already right. Ambiguity is handed back.
 */
const autoMatch = async () => {
  const musicName = (props.detail.view.title || props.detail.view.displayName).trim();
  const artistName = amllPrimaryArtist(props.detail.view.artist);
  if (!musicName && !artistName) {
    $message.info(t("local.detail.lyric.autoMatchNone"));
    return;
  }

  autoBusy.value = true;
  try {
    // Reset the boxes to the tags and go through the one search path, so the
    // artist fallback and the staleness guard apply here too.
    searchTitle.value = musicName;
    searchArtist.value = artistName;
    if (searchTimer !== null) {
      clearTimeout(searchTimer);
      searchTimer = null;
    }
    const applied = await runSearch({ fallbackToTitle: true });

    if (!applied) return; // Someone typed while this was out; those rows are theirs.
    if (lastFailure.value === "failed") {
      $message.error(t("local.detail.lyric.searchFailed"));
      return;
    }

    const items = results.value;
    if (items.length === 1) {
      await importFromAmll(items[0]);
    } else if (items.length === 0) {
      $message.info(t("local.detail.lyric.autoMatchNone"));
    } else {
      $message.info(t("local.detail.lyric.autoMatchAmbiguous", { count: items.length }));
    }
  } finally {
    autoBusy.value = false;
  }
};

onMounted(load);

onUnmounted(() => {
  if (searchTimer !== null) clearTimeout(searchTimer);
});

watch(
  () => props.detail.view.key,
  () => {
    // The dialog belongs to the track it was opened for: its keywords and results
    // are that track's, and a click after the pane moved on would import into the
    // wrong file. `importFromAmll` guards that too, but closing is the honest
    // answer rather than leaving a stale list on screen.
    if (searchTimer !== null) {
      clearTimeout(searchTimer);
      searchTimer = null;
    }
    searchGeneration += 1;
    searchShow.value = false;
    results.value = [];
    total.value = 0;
    lastFailure.value = null;
    load();
  },
);
</script>

<style lang="scss" scoped>
.lyric-pane {
  display: flex;
  flex-direction: column;
  max-width: 760px;

  .status {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 12px;

    .origin {
      font-size: 12px;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
  }

  .formats {
    margin-top: 10px;
    font-size: 12px;
  }

  .preview {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 120px;
    margin-top: 12px;
    padding: 10px 12px;
    border-radius: var(--radius-md);
    background-color: color-mix(in srgb, var(--n-text-color) 4%, transparent);

    .scroll {
      max-height: 320px;
      width: 100%;
    }

    .text {
      margin: 0;
      font-family: inherit;
      font-size: 12.5px;
      line-height: 1.7;
      white-space: pre-wrap;
      overflow-wrap: anywhere;
      // 歌词是这一页唯一值得复制走的内容。
      -webkit-user-select: text;
      user-select: text;
    }
  }
}
// 弹窗被 teleport 到 body，所以这些选择器必须是顶层的，不能嵌在 .lyric-pane 里
// —— 和 ListenTogetherModal 一个写法。
.search-form {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;

  .n-input {
    flex: 1 1 180px;
  }
}

.results {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 160px;
  margin-top: 12px;

  .scroll {
    max-height: 320px;
    width: 100%;
  }

  .row {
    padding: 8px 10px;
    border-radius: var(--radius-md);
    cursor: pointer;
    transition: background-color 0.2s;

    &:hover {
      background-color: color-mix(in srgb, var(--n-text-color) 6%, transparent);
    }

    &.busy {
      pointer-events: none;
      opacity: 0.5;
    }

    .name {
      font-size: 14px;
      font-weight: bold;
    }

    .meta,
    .authors {
      font-size: 12px;
      opacity: 0.6;
    }
  }
}

.total {
  margin-right: auto;
  align-self: center;
  font-size: 12px;
}
</style>
