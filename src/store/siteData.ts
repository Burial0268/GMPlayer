import { defineStore, acceptHMRUpdate } from "pinia";

interface SiteDataState {
  siteTitle: string;
  songPicColor: string;
  songPicGradient: string;
  searchInputActive: boolean;
}

/**
 * store 级修复：老版本把 songPicColor 存成 `rgb(r, g, b)`，现在统一存
 * `"r, g, b"`。与来源无关、幂等（转换后不再含 `rgb(`）。
 *
 * 两条 hydrate 路径都要跑：Tauri store 的 `$patch` 发生在 persistedstate 的
 * `afterHydrate` 之后，只挂前者的话旧格式会被重新灌进来。
 */
export function repairSiteData(store: any): void {
  const match = String(store.songPicColor).match(/rgb\(([^)]+)\)/);
  if (!match) return;
  store.songPicColor = match[1]
    .split(",")
    .map((channel) => String(Number(channel.trim()) || 0))
    .join(", ");
}

const useSiteDataStore = defineStore("siteData", {
  state: (): SiteDataState => {
    return {
      siteTitle: import.meta.env.VITE_SITE_TITLE as string,
      songPicColor: "128, 128, 128",
      songPicGradient: "linear-gradient(-45deg, #666, #fff)",
      searchInputActive: false,
    };
  },
  getters: {},
  actions: {},
  persist: [
    {
      storage: localStorage,
      pick: ["siteTitle", "songPicColor", "songPicGradient"],
      afterHydrate(ctx: { store: any }) {
        repairSiteData(ctx.store);
      },
    },
  ],
  // Tauri 层：落盘到 store 文件 + 跨窗口同步。localStorage 那份仍然保留，
  // 它才是同步 hydrate 的来源。见 utils/tauri/store/piniaPersistence。
  tauri: {
    save: true,
    sync: true,
    // searchInputActive 是纯 UI 瞬时态，同步过去只会让别的窗口跟着抖。
    filterKeys: ["siteTitle", "songPicColor", "songPicGradient"],
    filterKeysStrategy: "pick",
  },
});

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useSiteDataStore, import.meta.hot));
}

export default useSiteDataStore;
