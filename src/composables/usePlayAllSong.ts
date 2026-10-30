import { musicStore } from "@/store";
import { soundStop } from "@/utils/AudioContext";

// Order-insensitive: random mode shuffles the persisted queue in place.
const isSameQueue = (queue: any[], songList: any[]): boolean => {
  if (queue.length !== songList.length) return false;
  const ids = new Set(songList.map((song) => song.id));
  return queue.every((song) => ids.has(song.id));
};

export const usePlayAllSong = () => {
  const music = musicStore();

  const playAllSong = (songList: any[]) => {
    if (!songList?.length) return;
    // If the first song is already playing from this very list, just resume.
    // A stale or partial queue (restored session, list hydrated since) must be
    // replaced, or the full list the caller supplied is silently dropped.
    if (music.getPlaySongData?.id === songList[0].id && isSameQueue(music.getPlaylists, songList)) {
      music.setPlayState(true);
      return;
    }
    // Exit personal FM mode
    if (music.getPersonalFmMode && typeof window.$player !== "undefined") {
      soundStop(window.$player);
      music.setPersonalFmMode(false);
    }
    music.setPlayState(true);
    music.setPlaylists(songList);
    music.setPlayListMode("list");
    music.addSongToPlaylists(songList[0]);
  };

  return { playAllSong };
};
