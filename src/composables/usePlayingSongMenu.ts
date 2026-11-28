import { computed, h, nextTick, ref, shallowRef, type Component } from "vue";
import { useI18n } from "vue-i18n";
import { NIcon, type DropdownOption } from "naive-ui";
import { DownloadRound, PlaylistAddRound } from "@vicons/material";
import { musicStore } from "@/store";
import { downloadAvailable } from "@/utils/library/download";
import { isLocalSong } from "@/utils/library/localLibrary";
import type { SongData } from "@/store/musicTypes";

/** A sheet loaded on first use that opens through a method on its instance. */
const lazySheet = <T>(load: () => Promise<{ default: Component }>) => {
  const component = shallowRef<Component>();
  const instance = ref<T>();
  const withInstance = async (run: (sheet: T) => void) => {
    if (!component.value) {
      component.value = (await load()).default;
      await nextTick();
    }
    if (instance.value) run(instance.value as T);
  };
  return { component, instance, withInstance };
};

/**
 * The playing song's items for the desktop big player's "more" menu — add to a
 * playlist and download, as in the mobile sheet (`MobilePlayerMenu`).
 *
 * Bind the sheets in the template, as DataLists does (`defineAsyncComponent`
 * gives no moment at which the instance exists to call into):
 * `<component :is="AddPlaylistSheet" v-if="AddPlaylistSheet" ref="addPlaylistSheetRef" />`
 * `<component :is="DownloadSheet" v-if="DownloadSheet" ref="downloadSheetRef" />`
 */
export const usePlayingSongMenu = () => {
  const { t } = useI18n();
  const music = musicStore();

  const addPlaylist = lazySheet<{ openAddToPlaylist: (id: SongData["id"]) => void }>(
    () => import("@/components/DataModal/AddPlaylist.vue"),
  );
  const download = lazySheet<{ openDownloadModal: (song: SongData) => void }>(
    () => import("@/components/DataModal/DownloadSong.vue"),
  );

  const icon = (component: Component) => () =>
    h(NIcon, { size: 18 }, { default: () => h(component) });

  // A local track has no Netease id: nothing to add to a playlist or download.
  const songOptions = computed<DropdownOption[]>(() => {
    const song = music.getPlaySongData;
    if (!song || isLocalSong(song)) return [];
    const options: DropdownOption[] = [
      { label: t("menu.add"), key: "addToPlaylist", icon: icon(PlaylistAddRound) },
    ];
    if (downloadAvailable()) {
      options.push({ label: t("menu.download"), key: "download", icon: icon(DownloadRound) });
    }
    return options;
  });

  /** Handles `key` if it is one of `songOptions`; returns whether it did. */
  const selectSongOption = (key: string): boolean => {
    const song = music.getPlaySongData;
    if (!song) return false;
    if (key === "addToPlaylist")
      addPlaylist.withInstance((sheet) => sheet.openAddToPlaylist(song.id));
    else if (key === "download") download.withInstance((sheet) => sheet.openDownloadModal(song));
    else return false;
    return true;
  };

  /** `songOptions`, a divider, then `rest` — the divider only when both sides have items. */
  const withSongOptions = (rest: DropdownOption[]): DropdownOption[] =>
    songOptions.value.length && rest.length
      ? [...songOptions.value, { type: "divider", key: "song-options-divider" }, ...rest]
      : [...songOptions.value, ...rest];

  return {
    AddPlaylistSheet: addPlaylist.component,
    addPlaylistSheetRef: addPlaylist.instance,
    DownloadSheet: download.component,
    downloadSheetRef: download.instance,
    withSongOptions,
    selectSongOption,
  };
};
