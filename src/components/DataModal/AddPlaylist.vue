<template>
  <GlassSheet v-model:show="addToPlaylistModal" :label="$t('menu.add')">
    <template #header>
      <div class="sheet-song add-playlist-head">
        <div class="sheet-song-text">
          <div class="sheet-song-name">{{ $t("menu.add") }}</div>
        </div>
        <n-button size="small" round strong secondary type="primary" @click="openCreate">
          <template #icon>
            <n-icon :component="AddRound" />
          </template>
          {{ $t("general.dialog.create") }}
        </n-button>
      </div>
      <div class="add-playlist-filter">
        <n-input
          v-model:value="keyword"
          clearable
          size="small"
          round
          :placeholder="$t('general.name.filterInList')"
        >
          <template #prefix>
            <n-icon :component="Filter" />
          </template>
        </n-input>
      </div>
    </template>

    <section class="sheet-group">
      <template v-if="user.getUserPlayLists.has">
        <button
          v-for="item in visibleLists"
          :key="item.id"
          type="button"
          class="sheet-row playlist-row"
          :disabled="pendingId !== null"
          @click="addToPlayList(item, item.id === likedId)"
        >
          <img class="playlist-cover" :src="coverUrl(item.cover, 64)" alt="" />
          <span class="row-label">
            <span class="playlist-name text-hidden">{{ item.name }}</span>
            <span class="row-meta">
              {{ $t("general.name.songSize", { size: item.trackCount ?? 0 }) }}
            </span>
          </span>
          <n-spin v-if="pendingId === item.id" :size="16" />
          <n-icon
            v-else-if="item.id === likedId"
            class="row-icon"
            size="20"
            :component="songLiked ? FavoriteRound : FavoriteBorderRound"
          />
        </button>
        <div v-if="!visibleLists.length" class="sheet-row playlist-empty">
          {{ $t("general.name.noPlaylistResult") }}
        </div>
      </template>
      <template v-else>
        <div v-for="index in 4" :key="index" class="sheet-row playlist-row">
          <n-skeleton class="playlist-cover" />
          <n-skeleton text style="width: 50%" />
        </div>
      </template>
    </section>
  </GlassSheet>
  <!-- 新建歌单 -->
  <CreatePlaylist ref="createPlaylistRef" />
</template>

<script setup>
import { AddRound, FavoriteBorderRound, FavoriteRound } from "@vicons/material";
import { Filter } from "@icon-park/vue-next";
import { addSongToPlayList } from "@/api/playlist";
import { notifyTrackInPlaylist, likedPlaylistId } from "@/utils/library/playlistMutations";
import { musicStore, userStore } from "@/store";
import { useI18n } from "vue-i18n";
import { coverUrl } from "@/utils/ncm/coverUrl";
import { fuzzyScore } from "@/utils/ui/fuzzySearch";
import getLanguageData from "@/utils/app/languageData";
import GlassSheet from "@/components/Common/GlassSheet.vue";
import CreatePlaylist from "./CreatePlaylist.vue";

const { t } = useI18n();
const user = userStore();
const music = musicStore();
const createPlaylistRef = ref(null);

const addToPlaylistModal = ref(false);
const addToPlaylistId = ref(null);
const keyword = ref("");
const pendingId = ref(null);

// 自建歌单的第一个就是「我喜欢的音乐」。它不走 /playlist/tracks，而是走 /like：
// 这样心形、喜欢列表和通知栏的收藏状态是同一次写入，不会各说各的。
const likedId = computed(() => likedPlaylistId());
const songLiked = computed(
  () => addToPlaylistId.value !== null && music.getSongIsLike(addToPlaylistId.value),
);

const visibleLists = computed(() => {
  const own = user.getUserPlayLists.own;
  const query = keyword.value.trim();
  if (!query) return own;
  return own
    .map((item) => ({ item, score: fuzzyScore(item.name, query) }))
    .filter((entry) => entry.score !== null)
    .sort((a, b) => b.score - a.score)
    .map((entry) => entry.item);
});

const addToPlayList = async (item, isLiked) => {
  const id = addToPlaylistId.value;
  if (pendingId.value !== null || id === null) return;
  if (isLiked) {
    // changeLikeList 自己提示成功 / 已收藏 / 失败，并通知打开着的歌单页。
    if (!songLiked.value) {
      pendingId.value = item.id;
      await music.changeLikeList(id, true);
      pendingId.value = null;
    } else {
      $message.info(getLanguageData("loveSongRepeat"));
    }
    closeAddToPlaylist();
    return;
  }
  pendingId.value = item.id;
  try {
    const res = await addSongToPlayList(item.id, id);
    if (res.status === 200) {
      $message.success(t("general.message.addSuccess"));
      closeAddToPlaylist();
      // 曲目数就地 +1：网易对 /playlist/tracks 写后读不一致，紧接着重拉歌单列表
      // 很可能拿回旧的 trackCount。开着的歌单页由 playlistMutations 自己安静地对账。
      notifyTrackInPlaylist(item.id, id, true);
    } else {
      $message.error(res.body?.message || t("general.message.addFailure"));
    }
  } catch (err) {
    console.error(t("general.message.addFailure"), err);
    $message.error(t("general.message.addFailure"));
  } finally {
    pendingId.value = null;
  }
};

const openCreate = () => createPlaylistRef.value?.openCreatePlaylist();

// 开启收藏到歌单
const openAddToPlaylist = (id) => {
  if (!user.userLogin) {
    $message.error(t("general.message.needLogin"));
    return false;
  }
  if (!user.getUserPlayLists.has && !user.getUserPlayLists.isLoading) {
    user.setUserPlayLists();
  }
  keyword.value = "";
  addToPlaylistId.value = id;
  addToPlaylistModal.value = true;
};

const closeAddToPlaylist = () => {
  addToPlaylistModal.value = false;
};

defineExpose({
  openAddToPlaylist,
});
</script>

<style lang="scss" scoped>
.add-playlist-head {
  padding-top: 0;
  padding-bottom: 10px;
}

.add-playlist-filter {
  flex: 0 0 auto;
  padding: 0 16px 12px;
}

.playlist-row {
  --sheet-row-inset: 70px;
  min-height: 64px;
  padding-top: 8px;
  padding-bottom: 8px;
}

.playlist-cover {
  width: 42px;
  height: 42px;
  flex-shrink: 0;
  border-radius: var(--radius-sm);
  object-fit: cover;
  background: var(--sheet-fill);
}

.row-label {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.playlist-empty {
  justify-content: center;
  color: var(--text-secondary);
}
</style>
