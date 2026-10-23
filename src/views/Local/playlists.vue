<template>
  <div class="local-playlists">
    <div class="toolbar">
      <n-button strong secondary round type="primary" @click="createShow = true">
        <template #icon>
          <n-icon :component="Plus" />
        </template>
        {{ $t("local.createPlaylist") }}
      </n-button>
      <n-button strong secondary round @click="openFavourites">
        <template #icon>
          <n-icon :component="Like" />
        </template>
        {{ $t("local.favourites") }}
      </n-button>
      <n-button v-if="!isMobilePlatform" strong secondary round @click="importM3u">
        <template #icon>
          <n-icon :component="FolderOpen" />
        </template>
        {{ $t("local.importM3u") }}
      </n-button>
    </div>

    <n-empty v-if="!local.playlists.length" :description="$t('local.noPlaylists')" size="large" />
    <n-list v-else class="playlist-list" hoverable clickable>
      <n-list-item
        v-for="playlist in local.playlists"
        :key="playlist.id"
        @click="open(playlist, $event)"
      >
        <template #prefix>
          <div class="badge">
            <n-icon :size="22" :component="MusicList" />
          </div>
        </template>
        <n-thing :title="playlist.name">
          <template #description>
            <n-text :depth="3">
              {{ $t("local.trackCount", { count: playlist.tracks.length }) }}
              <template v-if="playlist.importedFrom"> · {{ $t("local.imported") }}</template>
            </n-text>
          </template>
        </n-thing>
        <template #suffix>
          <n-space :size="4">
            <n-button
              quaternary
              circle
              :disabled="local.scan.active"
              :title="$t('local.addSongsToPlaylist')"
              @click.stop="addSongs(playlist)"
            >
              <template #icon>
                <n-icon :component="Plus" />
              </template>
            </n-button>
            <n-button quaternary circle @click.stop="confirmDelete(playlist)">
              <template #icon>
                <n-icon :component="DeleteFour" />
              </template>
            </n-button>
          </n-space>
        </template>
      </n-list-item>
    </n-list>

    <n-modal
      class="s-modal"
      v-model:show="createShow"
      preset="card"
      :title="$t('local.createPlaylist')"
      :bordered="false"
      style="max-width: 400px"
    >
      <n-input
        v-model:value="newName"
        clearable
        :placeholder="$t('local.playlistNamePlaceholder')"
        @keyup.enter="create"
      />
      <template #footer>
        <n-space justify="end">
          <n-button strong secondary round @click="createShow = false">
            {{ $t("general.dialog.cancel") }}
          </n-button>
          <n-button
            strong
            secondary
            round
            type="primary"
            :disabled="!newName.trim()"
            @click="create"
          >
            {{ $t("general.dialog.confirm") }}
          </n-button>
        </n-space>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { DeleteFour, FolderOpen, Like, MusicList, Plus } from "@icon-park/vue-next";
import { useLayerNavigation } from "@/utils/navigation";
import { useI18n } from "vue-i18n";
import { useLocalLibraryStore } from "@/store";
import {
  localPlaylistCreate,
  localPlaylistDelete,
  localPlaylistImportM3u,
  type LocalPlaylist,
} from "@/utils/localLibrary";
import { refToQuery } from "@/utils/playlistSource";
import { isMobile } from "@/utils/tauri/platform/mobile";

const { t } = useI18n();
const navigation = useLayerNavigation();
const local = useLocalLibraryStore();

const createShow = ref(false);
const newName = ref("");
/**
 * Import reads a file the user picks from anywhere on disk, which SAF cannot
 * express: a per-file grant cannot see the parent directory a playlist's
 * relative entries point at, so the import would match nothing. Hidden rather
 * than left to fail with an error nobody can act on.
 */
const isMobilePlatform = ref(false);

const open = (playlist: LocalPlaylist, origin: Event) => {
  navigation.openPage(
    {
      path: "/local/playlist",
      query: refToQuery({ kind: "local-playlist", id: playlist.id }),
    },
    { origin },
  );
};

const openFavourites = (origin: Event) => {
  navigation.openPage(
    { path: "/local/playlist", query: refToQuery({ kind: "local-favourites" }) },
    { origin },
  );
};

const create = async () => {
  const name = newName.value.trim();
  if (!name) return;
  try {
    await localPlaylistCreate(name);
    await local.refreshPlaylists();
    newName.value = "";
    createShow.value = false;
  } catch (err) {
    $message.error(String(err));
  }
};

/**
 * Pick individual audio files straight into `playlist`.
 *
 * One dialog, not two steps: importing a folder and then hunting the songs down
 * in the library is the flow this replaces. The files are indexed (so they get
 * tags, a cover and a stable id) *and* appended, in one command — see
 * `local_source_add_files`.
 */
const addSongs = async (playlist: LocalPlaylist) => {
  try {
    const summary = await local.importFiles(playlist.id);
    if (!summary) return;
    $message.success(
      t("local.scanDone", {
        added: summary.added,
        updated: summary.updated,
        removed: summary.removed,
        failed: summary.failed,
      }),
    );
  } catch (err) {
    $message.error(String(err));
  }
};

const importM3u = async () => {
  try {
    const result = await localPlaylistImportM3u();
    // `null` means the picker was dismissed.
    if (!result) return;
    await local.refreshPlaylists();
    // The missing count is reported rather than hidden: an import that silently
    // drops half a playlist looks like a parsing bug, and the usual cause is
    // simply that those files are not in the library yet.
    if (result.missing > 0) {
      $message.warning(
        t("local.importedPartial", { matched: result.matched, missing: result.missing }),
      );
    } else {
      $message.success(t("local.importedAll", { matched: result.matched }));
    }
  } catch (err) {
    $message.error(String(err));
  }
};

const confirmDelete = (playlist: LocalPlaylist) => {
  $dialog.warning({
    class: "s-dialog",
    title: t("general.dialog.delete"),
    content: t("local.deletePlaylistQuestion", { name: playlist.name }),
    positiveText: t("general.dialog.delete"),
    negativeText: t("general.dialog.cancel"),
    onPositiveClick: async () => {
      await localPlaylistDelete(playlist.id);
      await local.refreshPlaylists();
    },
  });
};

onMounted(async () => {
  $setSiteTitle(t("sidebar.localMusic") + " - " + t("local.tab.playlists"));
  isMobilePlatform.value = await isMobile();
  local.refreshPlaylists();
});
</script>

<style lang="scss" scoped>
.local-playlists {
  .toolbar {
    display: flex;
    gap: 10px;
    margin-bottom: 16px;
    flex-wrap: wrap;
  }
  .playlist-list {
    background-color: transparent;
    .badge {
      display: flex;
      align-items: center;
      justify-content: center;
      width: 44px;
      height: 44px;
      border-radius: 8px;
      background-color: rgba(var(--main-color), 0.14);
      color: rgb(var(--main-color));
    }
  }
}
</style>
