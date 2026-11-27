import { nextTick, ref, shallowRef, type Component } from "vue";
import type { SongData } from "@/store/musicTypes";

/**
 * The single-song download sheet, loaded on first use.
 *
 * Bind both refs in the template —
 * `<component :is="DownloadSheet" v-if="DownloadSheet" ref="downloadSheetRef" />` —
 * as DataLists does: the sheet opens through a method on its instance, and
 * `defineAsyncComponent` gives no moment at which that instance exists.
 */
export const useSongDownloadSheet = () => {
  const DownloadSheet = shallowRef<Component>();
  const downloadSheetRef = ref<{ openDownloadModal: (song: SongData) => void }>();

  const openDownload = async (song: SongData | null | undefined) => {
    if (!song) return;
    if (!DownloadSheet.value) {
      DownloadSheet.value = (await import("@/components/DataModal/DownloadSong.vue")).default;
      await nextTick();
    }
    downloadSheetRef.value?.openDownloadModal(song);
  };

  return { DownloadSheet, downloadSheetRef, openDownload };
};
