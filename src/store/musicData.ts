import { defineStore, acceptHMRUpdate, storeToRefs } from "pinia";
import { nextTick, h } from "vue";
import { getSongTime, getDailySongsDate } from "@/utils/timeTools";
import { getPersonalFm, setFmTrash } from "@/api/home";
import { getLikelist, setLikeSong } from "@/api/user";
import { getPlayListCatlist } from "@/api/playlist";
import { resolveSongUrl } from "@/utils/AudioContext/resolveSongUrl";
import { userStore } from "@/store";
import { NIcon } from "naive-ui";
import { PlayCycle, PlayOnce, ShuffleOne } from "@icon-park/vue-next";
import {
  soundStop,
  fadePlayOrPause,
  getAutoMixEngine,
  getAudioPreloader,
} from "@/utils/AudioContext";
import { isAudioBackendRuntimeAvailable } from "@/utils/tauri/NativeRustSound";
import getLanguageData from "@/utils/getLanguageData";
import type { SongLyric } from "@/utils/LyricsProcessor";
import useMusicLyricStore from "./musicLyric";
import useMusicPersistedDataStore from "./musicPersistedData";
import useMusicPlaybackDataStore from "./musicPlaybackData";
import useMusicPlaybackResumeStore from "./musicPlaybackResume";
import {
  createDefaultPlaySongTime,
  type PersistData,
  type PlaySongTime,
  type SongData,
} from "./musicTypes";

declare const $message: any;
declare const $player: any;

interface AutoMixStateData {
  phase: "idle" | "analyzing" | "waiting" | "crossfading" | "finishing";
  outroType: string | null;
  outroConfidence: number;
  crossfadeStartTime: number;
  crossfadeDuration: number;
  crossfadeProgress: number;
  incomingSongName: string | null;
  incomingSongId: number | null;
}

interface MusicDataState {
  showBigPlayer: boolean;
  showPlayBar: boolean;
  showPlayList: boolean;
  playState: boolean;
  songLyric: SongLyric;
  playSongLyricIndex: number;
  dailySongsData: SongData[];
  dailySongsDate: string;
  catList: Record<string, any>;
  highqualityCatList: any[];
  spectrumsData: number[];
  spectrumsScaleData: number;
  lowFreqVolume: number;
  isLoadingSong: boolean;
  loadingStage: "idle" | "resolving" | "buffering" | "stalled" | "error";
  preloadedSongIds: Set<number>;
  autoMixState: AutoMixStateData;
  playSongTime: PlaySongTime;
  persistData: PersistData;
  playingSongId: number | null;
}

const useMusicDataStore = defineStore("musicData", {
  state: (): MusicDataState => {
    const persistedStore = useMusicPersistedDataStore();
    const resumeStore = useMusicPlaybackResumeStore();
    const snapshot = resumeStore.session;
    if (snapshot.songId !== null) {
      const snapshotIndex = persistedStore.persistData.playlists.findIndex(
        (song) => song.id === snapshot.songId,
      );
      if (snapshotIndex >= 0) {
        if (
          persistedStore.persistData.playSongIndex !== snapshotIndex ||
          snapshot.playSongIndex !== snapshotIndex
        ) {
          resumeStore.saveSession(snapshot.songId, snapshotIndex, snapshot.playSongTime);
        }
      } else {
        const currentIndex = persistedStore.persistData.playSongIndex;
        const currentSong = persistedStore.persistData.playlists[currentIndex];
        resumeStore.saveSession(currentSong?.id ?? null, currentIndex, createDefaultPlaySongTime());
      }
    }
    const playbackStore = useMusicPlaybackDataStore();
    const lyricStore = useMusicLyricStore();
    const { playSongTime } = storeToRefs(playbackStore);
    const { songLyric, playSongLyricIndex } = storeToRefs(lyricStore);

    return {
      showBigPlayer: false,
      showPlayBar: true,
      showPlayList: false,
      playState: false,
      songLyric: songLyric as unknown as SongLyric,
      playSongLyricIndex: playSongLyricIndex as unknown as number,
      dailySongsData: [],
      dailySongsDate: "",
      catList: {},
      highqualityCatList: [],
      spectrumsData: [],
      spectrumsScaleData: 1,
      lowFreqVolume: 0,
      isLoadingSong: false,
      loadingStage: "idle",
      preloadedSongIds: new Set(),
      autoMixState: {
        phase: "idle",
        outroType: null,
        outroConfidence: 0,
        crossfadeStartTime: 0,
        crossfadeDuration: 0,
        crossfadeProgress: -1,
        incomingSongName: null,
        incomingSongId: null,
      },
      playSongTime: playSongTime as unknown as PlaySongTime,
      persistData: persistedStore.persistData,
      // 播放器当前实际加载的歌曲 ID。队列被 setPlaylists 整体替换时不会变，
      // 因此它是「是否切歌」的判定基准，而不是 playlists[playSongIndex]。
      playingSongId:
        persistedStore.persistData.playlists[persistedStore.persistData.playSongIndex]?.id ?? null,
    };
  },
  getters: {
    getPersonalFmMode(state): boolean {
      return state.persistData.personalFmMode;
    },
    getPersonalFmData(state): SongData | Record<string, never> {
      return state.persistData.personalFmData;
    },
    getLoadingState(state): boolean {
      return state.isLoadingSong;
    },
    getLoadingStage(state): "idle" | "resolving" | "buffering" | "stalled" | "error" {
      return state.loadingStage;
    },
    getDailySongs(state): SongData[] {
      return state.dailySongsData;
    },
    getDailySongsDate(state): string {
      return state.dailySongsDate;
    },
    getPlaylists(state): SongData[] {
      return state.persistData.playlists;
    },
    getSpectrumsData(state): number[] {
      return state.spectrumsData;
    },
    getLowFreqVolume(state): number {
      return state.lowFreqVolume;
    },
    getPlaySongMode(state): "normal" | "random" | "single" {
      return state.persistData.playSongMode;
    },
    getPlaySongData(state): SongData | undefined {
      return state.persistData.playlists[state.persistData.playSongIndex];
    },
    getPlaySongLyric(state): SongLyric {
      return state.songLyric;
    },
    getPlaySongLyricIndex(state): number {
      return state.playSongLyricIndex;
    },
    getPlaySongTime(state): PlaySongTime {
      return state.playSongTime;
    },
    getPlayState(state): boolean {
      return state.playState;
    },
    getLikeList(state): number[] {
      return state.persistData.likeList;
    },
    getPlayHistory(state): SongData[] {
      return state.persistData.playHistory;
    },
    getPlayListMode(state): string {
      return state.persistData.playListMode;
    },
    getSearchHistory(state): string[] {
      return state.persistData.searchHistory;
    },
  },
  actions: {
    /**
     * 重置当前歌曲歌词状态：
     * - 清空上一首歌的歌词与处理缓存
     * - 将歌词索引置为 -1
     * 在切换歌曲但新歌词尚未加载完成时调用，避免界面继续显示旧歌词。
     */
    resetSongLyricState() {
      useMusicLyricStore().resetSongLyricState();
    },

    preloadUpcomingSongs() {
      if (isAudioBackendRuntimeAvailable()) {
        console.log("预加载已跳过：audio-backend runtime 已接管播放");
        return;
      }
      const audioPreloader = getAudioPreloader();
      if (audioPreloader.isPreloading) {
        console.log("预加载已跳过：AudioPreloader 正在处理下一首");
        return;
      }
      if (!(this.preloadedSongIds instanceof Set)) {
        console.warn("preloadedSongIds 类型不正确，已重置。");
        this.preloadedSongIds = new Set();
      }
      if (this.persistData.personalFmMode) {
        console.log("预加载已跳过：私人 FM 模式");
        return;
      }
      const playlist = this.persistData.playlists;
      const listLength = playlist.length;
      if (listLength < 2 || this.persistData.playSongMode !== "normal") {
        console.log(
          `预加载已跳过：歌曲数 ${listLength} / 播放模式 ${this.persistData.playSongMode}`,
        );
        return;
      }

      const currentIndex = this.persistData.playSongIndex;
      const preloadCount = 5;
      const songsToPreload: SongData[] = [];

      for (let i = 0; i <= preloadCount; i++) {
        const nextIndex = (currentIndex + i) % listLength;
        const songData = playlist[nextIndex];
        if (songData && !this.preloadedSongIds.has(songData.id)) {
          songsToPreload.push(songData);
        }
      }

      if (!songsToPreload.length) {
        console.log("没有需要预加载的新歌曲");
        return;
      }

      console.log("即将并行预加载歌曲:", songsToPreload.map((s) => s.name).join(", "));

      const urlPromises = songsToPreload.map((songData) =>
        resolveSongUrl(songData)
          .then((result) => {
            if (!result) {
              console.warn(`${songData.name} 无法获取 URL，跳过预加载`);
              return null;
            }
            return {
              id: songData.id,
              name: songData.name,
              url: result.url,
            };
          })
          .catch((err: any) => {
            console.error(`获取 ${songData.name} URL 失败`, err);
            return null;
          }),
      );

      Promise.all(urlPromises).then((results) => {
        const validSongs = results.filter(Boolean) as { id: number; name: string; url: string }[];
        if (!validSongs.length) return;

        const fetchPromises = validSongs.map((song) =>
          fetch(song.url)
            .then((response) => {
              if (response.ok) {
                console.log(`歌曲 ${song.name} 预加载完成`);
                this.preloadedSongIds.add(song.id);
              } else {
                throw new Error(`Response status: ${response.status}`);
              }
            })
            .catch((err) => {
              console.warn(`歌曲 ${song.name} 预加载请求失败`, err);
            }),
        );

        Promise.all(fetchPromises).then(() => {
          console.log("本批次预加载任务全部结束");
        });
      });
    },

    setPersonalFmMode(value: boolean) {
      this.persistData.personalFmMode = value;
      if (value) {
        if (typeof $player !== "undefined") soundStop($player);
        if ((this.persistData.personalFmData as SongData)?.id) {
          this.persistData.playlists = [];
          this.persistData.playlists.push(this.persistData.personalFmData as SongData);
          this.commitPlaySongIndex(0);
        } else {
          this.setPersonalFmData();
        }
      }
    },

    setPersonalFmData() {
      try {
        const songName = (this.getPersonalFmData as SongData)?.name;
        getPersonalFm().then((res: any) => {
          if (res.data[0]) {
            const data = res.data[2] || res.data[0];
            const fmData: SongData = {
              id: data.id,
              name: data.name,
              artist: data.artists,
              album: data.album,
              alia: data.alias,
              time: getSongTime(data.duration),
              fee: data.fee,
              pc: data.pc ? data.pc : null,
              mv: data.mvid,
            };
            if (songName && songName === fmData.name) {
              this.setFmDislike(fmData.id);
            } else {
              this.persistData.personalFmData = fmData;
              if (this.persistData.personalFmMode) {
                if (typeof $player !== "undefined") soundStop($player);
                this.persistData.playlists = [];
                this.persistData.playlists.push(fmData);
                this.commitPlaySongIndex(0);
                this.setPlayState(true);
              }
            }
          } else {
            $message.error(getLanguageData("personalFmError"));
          }
        });
      } catch (err) {
        console.error(getLanguageData("personalFmError"), err);
        $message.error(getLanguageData("personalFmError"));
      }
    },

    setFmDislike(id: number) {
      const user = userStore();
      if (user.userLogin) {
        setFmTrash(id).then((res: any) => {
          if (res.code === 200) {
            this.persistData.personalFmMode = true;
            this.setPlaySongIndex("next");
          } else {
            $message.error(getLanguageData("fmTrashError"));
          }
        });
      } else {
        $message.error(getLanguageData("needLogin"));
      }
    },

    setLikeList() {
      const user = userStore();
      if (user.userLogin) {
        getLikelist(user.userData.id).then((res: any) => {
          this.persistData.likeList = res.ids;
        });
      }
    },

    getSongIsLike(id: number): boolean {
      return this.persistData.likeList.includes(id);
    },

    async changeLikeList(id: number, like: boolean = true) {
      const user = userStore();
      const list = this.persistData.likeList;
      const exists = list.includes(id);
      if (!user.userLogin) {
        $message.error(getLanguageData("needLogin"));
        return;
      }
      try {
        const res = await setLikeSong(id, like);
        if (res.code === 200) {
          if (like && !exists) {
            list.push(id);
            $message.info(getLanguageData("loveSong"));
          } else if (!like && exists) {
            list.splice(list.indexOf(id), 1);
            $message.info(getLanguageData("loveSongRemove"));
          } else if (like && exists) {
            $message.info(getLanguageData("loveSongRepeat"));
          }
        } else {
          if (like) {
            $message.error(getLanguageData("loveSongError"));
          } else {
            $message.error(getLanguageData("loveSongRemoveError"));
          }
        }
      } catch (error) {
        console.error(getLanguageData("loveSongError"), error);
        $message.error(getLanguageData("loveSongError"));
      }
    },

    setPlayState(value: boolean) {
      this.playState = value;
    },

    setBigPlayerState(value: boolean) {
      this.showBigPlayer = value;
    },

    setPlayBarState(value: boolean) {
      this.showPlayBar = value;
    },

    setPlayListMode(value: string) {
      this.persistData.playListMode = value;
    },

    setPlaylists(value: SongData[]) {
      this.persistData.playlists = value.slice();
      this.persistData.playSongIndex = Math.min(
        Math.max(0, this.persistData.playSongIndex),
        Math.max(0, this.persistData.playlists.length - 1),
      );
      this.resetPlaySongTime();
      this.preloadedSongIds.clear();
      getAudioPreloader().cleanup();
      // 切换播放列表时，清空旧歌词，等待新歌曲歌词加载
      this.resetSongLyricState();
    },

    setDailySongs(value: any[], date = getDailySongsDate()) {
      if (value) {
        this.dailySongsData = [];
        this.dailySongsDate = date;
        value.forEach((v) => {
          this.dailySongsData.push({
            id: v.id,
            name: v.name,
            artist: v.ar,
            album: v.al,
            alia: v.alia,
            time: getSongTime(v.dt),
            fee: v.fee,
            pc: v.pc ? v.pc : null,
            mv: v.mv ? v.mv : null,
          });
        });
      }
    },

    setPlaySongLyric(value: any) {
      useMusicLyricStore().setPlaySongLyric(value);
    },

    setPlaySongTime(value: { currentTime: number; duration: number; displayCurrentTime?: number }) {
      const playbackStore = useMusicPlaybackDataStore();
      playbackStore.setPlaySongTime(value);
      useMusicLyricStore().syncCurrentLyricIndex(playbackStore.getPlaySongPlaybackCurrentTime());
    },

    resetPlaySongTime({ checkpoint = true }: { checkpoint?: boolean } = {}) {
      useMusicPlaybackDataStore().resetPlaySongTime({ checkpoint: false });
      if (checkpoint) this.checkpointPlaySongTime(true);
    },

    checkpointPlaySongTime(force = false) {
      if (!force) return;
      const playbackStore = useMusicPlaybackDataStore();
      useMusicPlaybackResumeStore().saveSession(
        this.playingSongId ?? this.getPlaySongData?.id ?? null,
        this.persistData.playSongIndex,
        playbackStore.playSongTime,
      );
    },

    commitPlaySongIndex(
      index: number,
      time?: { currentTime: number; duration: number; displayCurrentTime?: number },
    ) {
      if (!Number.isInteger(index) || index < 0 || index >= this.persistData.playlists.length) {
        return false;
      }
      const playbackStore = useMusicPlaybackDataStore();
      if (time) playbackStore.setPlaySongTime(time);
      else playbackStore.resetPlaySongTime({ checkpoint: false });
      const songId = this.persistData.playlists[index]?.id ?? null;
      this.playingSongId = songId;
      useMusicPlaybackResumeStore().saveSession(songId, index, playbackStore.playSongTime);
      return true;
    },

    getPlaySongPlaybackCurrentTime(): number {
      return useMusicPlaybackDataStore().getPlaySongPlaybackCurrentTime();
    },

    setPlaySongMode(value: "normal" | "random" | "single" | null = null) {
      const modeObj = {
        normal: PlayCycle,
        random: ShuffleOne,
        single: PlayOnce,
      };
      if (value && value in modeObj) {
        this.persistData.playSongMode = value;
      } else {
        switch (this.persistData.playSongMode) {
          case "normal":
            this.persistData.playSongMode = "random";
            value = "random";
            break;
          case "random":
            this.persistData.playSongMode = "single";
            value = "single";
            break;
          default:
            this.persistData.playSongMode = "normal";
            value = "normal";
            break;
        }
      }
      // Clean up preloader when mode is not normal (can't predict next song)
      if (this.persistData.playSongMode !== "normal") {
        getAudioPreloader().cleanup();
      }
      $message.info(getLanguageData(value!), {
        icon: () =>
          h(NIcon, null, {
            default: () => h(modeObj[this.persistData.playSongMode]),
          }),
      });
    },

    setPlaySongIndex(type: "next" | "prev") {
      if (typeof $player === "undefined") return false;
      // Cancel AutoMix crossfade on manual skip
      const autoMix = getAutoMixEngine();
      if (autoMix.isHandoffActive()) {
        autoMix.cancelCrossfade();
      }
      soundStop($player);
      if (this.persistData.playSongMode !== "single") {
        this.isLoadingSong = true;
      }
      if (this.persistData.personalFmMode) {
        this.setPersonalFmData();
      } else {
        const listLength = this.persistData.playlists.length;
        const listMode = this.persistData.playSongMode;
        let nextIndex = this.persistData.playSongIndex;
        if (listMode === "normal") {
          nextIndex += type === "next" ? 1 : -1;
        } else if (listMode === "random") {
          nextIndex = Math.floor(Math.random() * listLength);
        } else if (listMode === "single") {
          console.log("单曲循环模式");
          fadePlayOrPause($player, "play", this.persistData.playVolume);
        } else {
          $message.error(getLanguageData("playError"));
        }
        if (listMode !== "single") {
          if (nextIndex < 0) {
            nextIndex = listLength - 1;
          } else if (nextIndex >= listLength) {
            nextIndex = 0;
            soundStop($player);
            fadePlayOrPause($player, "play", this.persistData.playVolume);
          }
          if (listLength > 1) {
            soundStop($player);
          }
          this.commitPlaySongIndex(nextIndex);
          // 已经切换到下一首/上一首歌曲，先清空旧歌词，等待新歌词加载
          this.resetSongLyricState();
          nextTick().then(() => {
            this.setPlayState(true);
          });
        }
      }
    },

    selectPlaySongByIndex(index: number) {
      if (
        !Number.isInteger(index) ||
        index < 0 ||
        index >= this.persistData.playlists.length ||
        index === this.persistData.playSongIndex
      ) {
        return;
      }
      if (typeof $player !== "undefined") soundStop($player);
      this.commitPlaySongIndex(index);
      this.resetSongLyricState();
      this.isLoadingSong = true;
      this.setPlayState(true);
    },

    addSongToPlaylists(value: SongData, play: boolean = true) {
      const index = this.persistData.playlists.findIndex((o) => o.id === value.id);
      // 与「播放器实际加载的歌曲」(playingSongId) 比较，而不是 playlists[playSongIndex]：
      // playSong / playAllSong 等调用方会先用 setPlaylists 替换队列，旧索引在新队列中
      // 指向的只是恰好同位置的歌，相同 index 不代表同一首歌。
      const activeSongId = this.playingSongId ?? this.getPlaySongData?.id;
      const identityChanged = value.id !== activeSongId;
      try {
        if (identityChanged) {
          console.log("Play a song that is not the same as the last one");
          if (typeof $player !== "undefined") soundStop($player);
          this.isLoadingSong = true;
          // 将要播放不同歌曲，立即清空旧歌词，等待新歌词加载
          this.resetSongLyricState();
        }
      } catch (error) {
        console.error("Error:" + error);
      }
      if (index !== -1) {
        if (identityChanged) {
          this.commitPlaySongIndex(index);
        } else if (index !== this.persistData.playSongIndex) {
          // 同一首歌，只是在新队列中的位置变了：仅对齐索引，
          // 不重置进度、不重新加载声音。
          this.persistData.playSongIndex = index;
          this.checkpointPlaySongTime(true);
        }
      } else {
        this.persistData.playlists.push(value);
        this.commitPlaySongIndex(this.persistData.playlists.length - 1);
      }
      if (play) this.setPlayState(true);
    },

    addSongToNext(value: SongData) {
      this.persistData.playSongMode = "normal";
      const autoMix = getAutoMixEngine();
      let insertAfterIndex = this.persistData.playSongIndex;
      if (autoMix.isHandoffActive()) {
        const autoMixTargetIndex = autoMix.resolveActiveTransitionTargetIndex(insertAfterIndex);
        if (autoMixTargetIndex >= 0) {
          insertAfterIndex = autoMixTargetIndex;
        }
      }

      const index = this.persistData.playlists.findIndex((o) => o.id === value.id);
      if (index !== -1) {
        console.log(index);
        if (index === this.persistData.playSongIndex || index === insertAfterIndex) return true;
        if (index < this.persistData.playSongIndex) this.persistData.playSongIndex--;
        const arr = this.persistData.playlists.splice(index, 1)[0];
        if (index < insertAfterIndex) insertAfterIndex--;
        const insertIndex = insertAfterIndex + 1;
        this.persistData.playlists.splice(insertIndex, 0, arr);
        if (insertIndex <= this.persistData.playSongIndex) this.persistData.playSongIndex++;
      } else {
        const insertIndex = insertAfterIndex + 1;
        this.persistData.playlists.splice(insertIndex, 0, value);
        if (insertIndex <= this.persistData.playSongIndex) this.persistData.playSongIndex++;
      }
      this.checkpointPlaySongTime(true);
      $message.success(value.name + " " + getLanguageData("addSongToNext"));
    },

    removeSong(index: number) {
      if (typeof $player === "undefined") return false;
      const songId = this.persistData.playlists[index].id;
      const name = this.persistData.playlists[index].name;
      const removedCurrentSong = index === this.persistData.playSongIndex;
      if (index < this.persistData.playSongIndex) {
        this.persistData.playSongIndex--;
      } else if (index === this.persistData.playSongIndex) {
        soundStop($player);
      }
      $message.success(name + " " + getLanguageData("removeSong"));
      this.persistData.playlists.splice(index, 1);
      this.preloadedSongIds.delete(songId);
      // Next index may have changed after removal
      getAudioPreloader().cleanup();
      if (this.persistData.playSongIndex >= this.persistData.playlists.length) {
        this.persistData.playSongIndex = 0;
        soundStop($player);
      }
      if (removedCurrentSong) {
        // 索引现在指向后继歌曲（由 watcher 接手加载），同步实际播放标识
        this.playingSongId = this.persistData.playlists[this.persistData.playSongIndex]?.id ?? null;
        this.resetPlaySongTime();
      } else this.checkpointPlaySongTime(true);
    },

    setCatList(highquality: boolean = false) {
      getPlayListCatlist().then((res: any) => {
        if (res.code === 200) {
          this.catList = res;
        } else {
          $message.error(getLanguageData("getDataError"));
        }
      });
      if (highquality) {
        getPlayListCatlist(true).then((res: any) => {
          if (res.code === 200) {
            this.highqualityCatList = res.tags;
          } else {
            $message.error(getLanguageData("getDataError"));
          }
        });
      }
    },

    setPlayHistory(data: SongData | null, clean: boolean = false) {
      if (clean) {
        this.persistData.playHistory = [];
      } else if (data) {
        const index = this.persistData.playHistory.findIndex((item) => item.id === data.id);
        if (index !== -1) {
          this.persistData.playHistory.splice(index, 1);
        }
        if (this.persistData.playHistory.length > 100) this.persistData.playHistory.pop();
        this.persistData.playHistory.unshift(data);
      }
    },

    setSearchHistory(name: string | null, clean: boolean = false) {
      if (clean) {
        this.persistData.searchHistory = [];
      } else if (name) {
        const index = this.persistData.searchHistory.indexOf(name);
        if (index !== -1) {
          this.persistData.searchHistory.splice(index, 1);
        }
        this.persistData.searchHistory.unshift(name);
        if (this.persistData.searchHistory.length > 30) {
          this.persistData.searchHistory.pop();
        }
      }
    },

    updateCurrentTime(time: number) {
      (this as any).currentTime = Math.floor(time * 1000);
    },

    setLoadingState(state: boolean) {
      this.isLoadingSong = state;
      if (!state) this.loadingStage = "idle";
    },
  },
});

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useMusicDataStore, import.meta.hot));
}

export default useMusicDataStore;
