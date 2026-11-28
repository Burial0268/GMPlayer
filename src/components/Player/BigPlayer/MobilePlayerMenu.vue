<template>
  <GlassSheet v-model:show="show" :label="$t('player.controls.more')">
    <template #header>
      <div v-if="song" class="sheet-song">
        <img class="sheet-cover" :src="coverUrl(song.album?.picUrl, 120)" alt="" />
        <div class="sheet-song-text">
          <div class="sheet-song-name text-hidden">{{ song.name }}</div>
          <div class="sheet-song-artists text-hidden">{{ artistNames }}</div>
        </div>
      </div>
    </template>
    <section class="sheet-group">
      <div class="sheet-row">
        <n-icon class="row-icon" size="22" :component="AvTimerRound" />
        <span class="row-label">{{ $t("setting.lyricOffset") }}</span>
        <div class="stepper">
          <button
            type="button"
            :aria-label="$t('setting.lyricOffsetDelay')"
            @click="adjustOffset(-LYRIC_OFFSET_STEP)"
          >
            <n-icon size="18" :component="RemoveRound" />
          </button>
          <button
            type="button"
            :class="['stepper-value', { changed: offset !== 0 }]"
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
      <div v-if="languageOptions.length" class="sheet-row language-row">
        <n-icon class="row-icon" size="22" :component="TranslateRound" />
        <span class="row-label">{{ $t("setting.lyricTranslationLanguage") }}</span>
        <div class="chips">
          <button
            v-for="option in languageOptions"
            :key="option.value"
            type="button"
            :class="['chip', { active: selectedLanguage === option.value }]"
            :aria-pressed="selectedLanguage === option.value"
            @click="selectedLanguage = option.value"
          >
            {{ option.label }}
          </button>
        </div>
      </div>
    </section>

    <section v-if="actions.length" class="sheet-group">
      <button
        v-for="action in actions"
        :key="action.key"
        type="button"
        class="sheet-row"
        @click="action.run"
      >
        <n-icon class="row-icon" size="22" :component="action.icon" />
        <span class="row-label text-hidden">{{ action.label }}</span>
        <n-icon v-if="action.page" class="row-chevron" size="20" :component="ChevronRightRound" />
      </button>
    </section>
  </GlassSheet>
  <!-- Loaded on first use, as in DataLists: both open through a method on their ref. -->
  <component :is="AddPlaylistComponent" v-if="AddPlaylistComponent" ref="addPlaylistRef" />
  <component :is="DownloadSongComponent" v-if="DownloadSongComponent" ref="downloadSongRef" />
</template>

<script setup lang="ts">
/**
 * 大播放器（移动端）的「更多」面板：歌词偏移 / 翻译语言 + 当前歌曲的操作。
 *
 * 面板外壳、手势、Escape / 系统返回与行样式都来自 GlassSheet，这里只有内容。
 */
import { computed, nextTick, ref, shallowRef, type Component } from "vue";
import type { RouteLocationRaw } from "vue-router";
import { useI18n } from "vue-i18n";
import {
  AddRound,
  AlbumRound,
  AvTimerRound,
  ChevronRightRound,
  DownloadRound,
  InfoRound,
  LinkRound,
  PersonRound,
  PlaylistAddRound,
  RemoveRound,
  SmartDisplayRound,
  TranslateRound,
} from "@vicons/material";
import { musicStore } from "@/store";
import { coverUrl } from "@/utils/ncm/coverUrl";
import { LYRIC_OFFSET_STEP, useLyricControls } from "@/composables/useLyricControls";
import GlassSheet from "@/components/Common/GlassSheet.vue";

const show = defineModel<boolean>("show", { required: true });
const emit = defineEmits<{ openPage: [to: RouteLocationRaw] }>();

const { t } = useI18n();
const music = musicStore();
const { languageOptions, selectedLanguage, offset, offsetDisplay, adjustOffset, resetOffset } =
  useLyricControls();

const song = computed(() => music.getPlaySongData);
const artistNames = computed(() =>
  (song.value?.artist ?? []).map((artist) => artist?.name).join(" / "),
);

// ── 歌曲操作 ──
type Action = { key: string; icon: Component; label: string; run: () => void; page?: boolean };

const go = (to: RouteLocationRaw) => {
  show.value = false;
  emit("openPage", to);
};
const now = (action: (target: NonNullable<typeof song.value>) => void) => () => {
  const target = song.value;
  show.value = false;
  if (target) action(target);
};

const actions = computed<Action[]>(() => {
  const target = song.value;
  if (!target) return [];
  const local = Boolean(target.local?.uri);
  const list: Action[] = [];
  if (!local) {
    list.push(
      { key: "add", icon: PlaylistAddRound, label: t("menu.add"), run: now(addToPlaylist) },
      { key: "download", icon: DownloadRound, label: t("menu.download"), run: now(download) },
    );
  }
  list.push({
    key: "detail",
    icon: InfoRound,
    label: t("menu.songDetail"),
    page: true,
    run: () =>
      go(
        local
          ? { path: "/local/song", query: { key: String(target.local.uri) } }
          : { path: "/song", query: { id: target.id } },
      ),
  });
  if (target.mv) {
    list.push({
      key: "mv",
      icon: SmartDisplayRound,
      label: t("menu.mv"),
      page: true,
      run: () => go({ path: "/video", query: { id: target.mv } }),
    });
  }
  if (!local) {
    list.push({
      key: "copy",
      icon: LinkRound,
      label: t("menu.copy", { name: t("general.name.song"), other: t("general.name.link") }),
      run: now(copyLink),
    });
  }
  // 本地曲目的歌手和专辑来自文件标签：id 是 0，没有可去的网易页面。
  for (const artist of target.artist ?? []) {
    if (!artist || !(Number(artist.id) > 0)) continue;
    list.push({
      key: `artist-${artist.id}`,
      icon: PersonRound,
      label: artist.name,
      page: true,
      run: () => go({ path: "/artist/songs", query: { id: artist.id } }),
    });
  }
  if (Number(target.album?.id) > 0) {
    list.push({
      key: "album",
      icon: AlbumRound,
      label: target.album.name,
      page: true,
      run: () => go({ path: "/album", query: { id: target.album.id } }),
    });
  }
  return list;
});

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
// 外壳与行样式在 GlassSheet；这里只有歌词偏移步进器与翻译语言选项。
.stepper {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  border-radius: var(--radius-pill);
  background: var(--sheet-fill);

  button {
    appearance: none;
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 36px;
    height: 32px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-pill);
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;

    &:active {
      background: var(--sheet-separator);
    }
  }

  .stepper-value {
    min-width: 56px;
    font-variant-numeric: tabular-nums;
    color: var(--text-secondary);

    &.changed {
      color: var(--main-color);
      font-weight: 600;
    }
  }
}

// 语言可能有好几种：标签换到下一行，选项横向排开，不挤占标题。
.language-row {
  flex-wrap: wrap;
  row-gap: 8px;
  padding-top: 12px;
  padding-bottom: 12px;
}

.chips {
  flex-basis: 100%;
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  padding-left: 36px;
}

.chip {
  appearance: none;
  height: 30px;
  padding: 0 12px;
  border: 0;
  border-radius: var(--radius-pill);
  background: var(--sheet-fill);
  color: inherit;
  font: inherit;
  font-size: 0.82rem;
  cursor: pointer;

  &.active {
    background: var(--main-color);
    color: #fff;
  }
}
</style>
