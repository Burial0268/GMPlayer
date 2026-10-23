import useSettingDataStore from "./settingData";
import useMusicDataStore from "./musicData";
import useUserDataStore from "./userData";
import useSiteDataStore from "./siteData";
import useListenTogetherStore from "./listenTogether";
import useLocalLibraryStore from "./localLibrary";
import useDownloadStore from "./download";

export const settingStore = () => useSettingDataStore();
export const musicStore = () => useMusicDataStore();
export const userStore = () => useUserDataStore();
export const siteStore = () => useSiteDataStore();
export const listenTogetherStore = () => useListenTogetherStore();
export const localLibraryStore = () => useLocalLibraryStore();
export const downloadStore = () => useDownloadStore();

// Re-export stores for direct import
export {
  useSettingDataStore,
  useMusicDataStore,
  useUserDataStore,
  useSiteDataStore,
  useListenTogetherStore,
  useLocalLibraryStore,
  useDownloadStore,
};
