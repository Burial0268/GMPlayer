<template>
  <n-drawer
    v-model:show="show"
    class="data-list-action-drawer"
    placement="bottom"
    height="70vh"
    :z-index="2200"
  >
    <n-drawer-content
      v-if="song"
      :native-scrollbar="false"
      header-class="data-list-action-drawer-header"
      body-content-class="data-list-action-drawer-body"
      body-content-style="padding: 0"
      closable
    >
      <template #header>
        <SmallSongData :songData="song" notJump />
      </template>
      <div class="drawer-menu">
        <div class="item lyric-item">
          <n-icon size="20"><Time theme="filled" /></n-icon>
          <n-text>{{ $t("setting.lyricOffset") }}</n-text>
          <div class="offset-stepper">
            <button
              type="button"
              :aria-label="$t('setting.lyricOffsetDelay')"
              @click="adjustOffset(-LYRIC_OFFSET_STEP)"
            >
              <n-icon size="18" :component="RemoveRound" />
            </button>
            <button
              type="button"
              class="offset-value"
              :aria-label="$t('setting.lyricOffsetReset')"
              @click="resetOffset"
            >
              {{ offsetDisplay }}
            </button>
            <button
              type="button"
              :aria-label="$t('setting.lyricOffsetAdvance')"
              @click="adjustOffset(LYRIC_OFFSET_STEP)"
            >
              <n-icon size="18" :component="AddRound" />
            </button>
          </div>
        </div>
        <div v-if="languageOptions.length" class="item lyric-item">
          <n-icon size="20"><Translate theme="filled" /></n-icon>
          <n-text>{{ $t("setting.lyricTranslationLanguage") }}</n-text>
          <n-select
            v-model:value="selectedLanguage"
            class="language-select"
            size="small"
            :options="languageOptions"
            :consistent-menu-width="false"
          />
        </div>
        <div class="drawer-menu-divider" />
        <div v-if="!isLocal" class="item action-item" @click="run(addToPlaylist)">
          <n-icon size="20"><ListAdd theme="filled" /></n-icon>
          <n-text>{{ $t("menu.add") }}</n-text>
        </div>
        <div v-if="!isLocal" class="item action-item" @click="run(download)">
          <n-icon size="20"><DownloadFour theme="filled" /></n-icon>
          <n-text>{{ $t("menu.download") }}</n-text>
        </div>
        <div class="item action-item" @click="go(detailRoute)">
          <n-icon size="20"><FileMusic theme="filled" /></n-icon>
          <n-text>{{ $t("menu.songDetail") }}</n-text>
        </div>
        <div
          v-if="song.mv"
          class="item action-item"
          @click="go({ path: '/video', query: { id: song.mv } })"
        >
          <n-icon size="20"><Video theme="filled" /></n-icon>
          <n-text>{{ $t("menu.mv") }}</n-text>
        </div>
        <div v-if="!isLocal" class="item action-item" @click="run(copyLink)">
          <n-icon size="20"><LinkTwo theme="filled" /></n-icon>
          <n-text>{{
            $t("menu.copy", { name: $t("general.name.song"), other: $t("general.name.link") })
          }}</n-text>
        </div>
        <template v-if="artists.length || albumId">
          <div class="drawer-menu-divider" />
          <div
            v-for="artist in artists"
            :key="artist.id"
            class="item action-item"
            @click="go({ path: '/artist/songs', query: { id: artist.id } })"
          >
            <n-icon size="20"><Voice theme="filled" /></n-icon>
            <n-text class="text-hidden">{{ $t("general.name.artists") }}: {{ artist.name }}</n-text>
          </div>
          <div
            v-if="albumId"
            class="item action-item"
            @click="go({ path: '/album', query: { id: albumId } })"
          >
            <n-icon size="20"><RecordDisc theme="filled" /></n-icon>
            <n-text class="text-hidden"
              >{{ $t("general.name.album") }}: {{ song.album.name }}</n-text
            >
          </div>
        </template>
      </div>
    </n-drawer-content>
  </n-drawer>
  <!-- Loaded on first use, as in DataLists: both open through a method on their ref. -->
  <component :is="AddPlaylistComponent" v-if="AddPlaylistComponent" ref="addPlaylistRef" />
  <component :is="DownloadSongComponent" v-if="DownloadSongComponent" ref="downloadSongRef" />
</template>

<script setup lang="ts">
import { computed, nextTick, ref, shallowRef } from "vue";
import type { RouteLocationRaw } from "vue-router";
import { useI18n } from "vue-i18n";
import { AddRound, RemoveRound } from "@vicons/material";
import {
  DownloadFour,
  FileMusic,
  LinkTwo,
  ListAdd,
  RecordDisc,
  Time,
  Translate,
  Video,
  Voice,
} from "@icon-park/vue-next";
import { musicStore } from "@/store";
import SmallSongData from "@/components/DataList/SmallSongData.vue";
import { LYRIC_OFFSET_STEP, useLyricControls } from "@/composables/useLyricControls";

const show = defineModel<boolean>("show", { required: true });
const emit = defineEmits<{ openPage: [to: RouteLocationRaw] }>();

const { t } = useI18n();
const music = musicStore();
const { languageOptions, selectedLanguage, offsetDisplay, adjustOffset, resetOffset } =
  useLyricControls();

const song = computed(() => music.getPlaySongData);
const isLocal = computed(() => Boolean(song.value?.local?.uri));
// A local track's artists and album come from file tags: id 0, no Netease page to open.
const artists = computed(() =>
  (song.value?.artist ?? []).filter((artist) => artist && Number(artist.id) > 0),
);
const albumId = computed(() => (Number(song.value?.album?.id) > 0 ? song.value.album.id : null));
const detailRoute = computed<RouteLocationRaw>(() =>
  isLocal.value
    ? { path: "/local/song", query: { key: String(song.value.local.uri) } }
    : { path: "/song", query: { id: song.value.id } },
);

const run = (action: (target: typeof song.value) => void) => {
  const target = song.value;
  show.value = false;
  action(target);
};
const go = (to: RouteLocationRaw) => {
  show.value = false;
  emit("openPage", to);
};

const addPlaylistRef = ref();
const downloadSongRef = ref();
const AddPlaylistComponent = shallowRef();
const DownloadSongComponent = shallowRef();
const openLazy = async (holder, load, instance, invoke) => {
  if (!holder.value) {
    holder.value = (await load()).default;
    await nextTick();
  }
  invoke(instance.value);
};
const addToPlaylist = (target) =>
  openLazy(
    AddPlaylistComponent,
    () => import("@/components/DataModal/AddPlaylist.vue"),
    addPlaylistRef,
    (modal) => modal?.openAddToPlaylist(target.id),
  );
const download = (target) =>
  openLazy(
    DownloadSongComponent,
    () => import("@/components/DataModal/DownloadSong.vue"),
    downloadSongRef,
    (modal) => modal?.openDownloadModal(target),
  );
const copyLink = async (target) => {
  try {
    await navigator.clipboard.writeText(`https://music.163.com/#/song?id=${target.id}`);
    $message.success(t("general.message.copySuccess"));
  } catch (err) {
    console.error(t("general.message.copyFailure"), err);
    $message.error(t("general.message.copyFailure"));
  }
};
</script>

<style lang="scss" scoped>
.drawer-menu .item.lyric-item {
  cursor: default;

  &:active {
    color: inherit;
    background-color: transparent;
    transform: none;
  }

  .n-text {
    flex: 1;
  }
}

.offset-stepper {
  display: flex;
  align-items: center;
  gap: 4px;

  button {
    appearance: none;
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 36px;
    height: 32px;
    padding: 0 8px;
    border: none;
    border-radius: var(--radius-md);
    background: var(--drawer-item-hover);
    color: inherit;
    font: inherit;
    cursor: pointer;

    &:active {
      transform: scale(0.94);
    }
  }

  .offset-value {
    min-width: 64px;
    font-variant-numeric: tabular-nums;
  }
}

.language-select {
  width: 9rem;
}
</style>
