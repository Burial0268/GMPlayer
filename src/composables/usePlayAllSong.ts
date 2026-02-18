import { musicStore } from "@/store";
import { soundStop } from "@/utils/AudioContext";

export const usePlayAllSong = () => {
  const music = musicStore();

  const playAllSong = (songList: any[]) => {
    if (!songList?.length) return;
    // If the first song is already playing, just resume
    if (music.getPlaySongData?.id === songList[0].id) {
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
