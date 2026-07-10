import { acceptHMRUpdate, defineStore } from "pinia";
import useMusicPersistedDataStore from "./musicPersistedData";
import {
  createDefaultPlaybackSnapshot,
  createDefaultPlaySongTime,
  type PlaySongTime,
} from "./musicTypes";

export const useMusicPlaybackResumeStore = defineStore("musicPlaybackResume", () => {
  const persistedStore = useMusicPersistedDataStore();
  const legacy = persistedStore.persistData;
  const legacySong = legacy.playlists[legacy.playSongIndex];
  const session =
    legacy.playbackSnapshot ?? (legacy.playbackSnapshot = createDefaultPlaybackSnapshot());

  if (session.songId === null && legacySong) {
    session.songId = legacySong.id;
    session.playSongIndex = legacy.playSongIndex;
    session.playSongTime = {
      ...createDefaultPlaySongTime(),
      ...legacy.playSongTime,
    };
  }

  function saveSession(songId: number | null, playSongIndex: number, time: PlaySongTime) {
    persistedStore.$patch(() => {
      legacy.playSongIndex = playSongIndex;
      legacy.playSongTime = { ...time };
      session.version = 1;
      session.revision++;
      session.songId = songId;
      session.playSongIndex = playSongIndex;
      session.playSongTime = { ...time };
      session.updatedAt = Date.now();
    });
  }

  function clearSessionTime() {
    persistedStore.$patch(() => {
      session.revision++;
      session.playSongTime = createDefaultPlaySongTime();
      session.updatedAt = Date.now();
    });
  }

  return { session, saveSession, clearSessionTime };
});

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useMusicPlaybackResumeStore, import.meta.hot));
}

export default useMusicPlaybackResumeStore;
