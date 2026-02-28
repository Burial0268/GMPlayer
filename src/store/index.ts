import useSettingDataStore from "./settingData";
import useMusicDataStore from "./musicData";
import useUserDataStore from "./userData";
import useSiteDataStore from "./siteData";
import useListenTogetherStore from "./listenTogether";

export const settingStore = () => useSettingDataStore();
export const musicStore = () => useMusicDataStore();
export const userStore = () => useUserDataStore();
export const siteStore = () => useSiteDataStore();
export const listenTogetherStore = () => useListenTogetherStore();

// Re-export stores for direct import
export { useSettingDataStore, useMusicDataStore, useUserDataStore, useSiteDataStore, useListenTogetherStore };
