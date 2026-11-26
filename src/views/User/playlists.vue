<template>
  <div class="playlists">
    <div class="toolbar">
      <n-button
        type="primary"
        strong
        secondary
        round
        @click="createPlaylistRef.openCreatePlaylist()"
      >
        <template #icon>
          <n-icon :component="AddCircleRound" />
        </template>
        {{ $t("menu.create") }}
      </n-button>
      <!-- 新建歌单 -->
      <CreatePlaylist ref="createPlaylistRef" />
    </div>
    <CoverLists :listData="user.getUserPlayLists.own" :loading="!user.getUserPlayLists.has" />
  </div>
</template>

<script setup lang="ts">
import { AddCircleRound } from "@vicons/material";
import { userStore } from "@/store";
import { useI18n } from "vue-i18n";
import CoverLists from "@/components/DataList/CoverLists.vue";
import CreatePlaylist from "@/components/DataModal/CreatePlaylist.vue";

const { t } = useI18n();
const user = userStore();
const createPlaylistRef = ref(null);

onMounted(() => {
  $setSiteTitle(t("nav.user") + " - " + t("nav.userChildren.playlist"));
  if (!user.getUserPlayLists.has && !user.getUserPlayLists.isLoading) user.setUserPlayLists();
});
</script>

<style lang="scss" scoped>
.playlists {
  .toolbar {
    display: flex;
    align-items: center;
    margin-bottom: 16px;
  }
}
</style>
