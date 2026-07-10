import { acceptHMRUpdate, defineStore } from "pinia";
import { reactive } from "vue";
import { createDefaultPersistData, type PersistData } from "./musicTypes";

export const useMusicPersistedDataStore = defineStore(
  "musicPersistedData",
  () => {
    const persistData = reactive<PersistData>(createDefaultPersistData());

    return { persistData };
  },
  {
    persist: [
      {
        key: "musicData",
        storage: localStorage,
        pick: ["persistData"],
      },
    ],
  },
);

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useMusicPersistedDataStore, import.meta.hot));
}

export default useMusicPersistedDataStore;
