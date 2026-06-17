import { defineStore, acceptHMRUpdate } from "pinia";

interface SiteDataState {
  siteTitle: string;
  songPicColor: string;
  songPicGradient: string;
  searchInputActive: boolean;
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
        const match = String(ctx.store.songPicColor).match(/rgb\(([^)]+)\)/);
        if (match) {
          ctx.store.songPicColor = match[1]
            .split(",")
            .map((channel) => String(Number(channel.trim()) || 0))
            .join(", ");
        }
      },
    },
  ],
});

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useSiteDataStore, import.meta.hot));
}

export default useSiteDataStore;
