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
      songPicColor: "rgb(128,128,128)",
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
    },
  ],
});

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useSiteDataStore, import.meta.hot));
}

export default useSiteDataStore;
