import { defineStore, acceptHMRUpdate } from "pinia";
import { nextTick, h } from "vue";
import { getSongTime, getSongPlayingTime } from "@/utils/timeTools";
import { getPersonalFm, setFmTrash } from "@/api/home";
import { getLikelist, setLikeSong } from "@/api/user";
import { getPlayListCatlist } from "@/api/playlist";
import { getMusicUrl, getMusicNumUrl } from "@/api/song";
import { userStore, settingStore } from "@/store";
import { NIcon } from "naive-ui";
import { PlayCycle, PlayOnce, ShuffleOne } from "@icon-park/vue-next";
import {
  soundStop,
  fadePlayOrPause,
  getAutoMixEngine,
  getAudioPreloader,
} from "@/utils/AudioContext";
import getLanguageData from "@/utils/getLanguageData";
import {
  preprocessLyrics,
  type SongLyric,
  type ParsedLrcLine,
  type ParsedYrcLine,
  type StoredLyricLine,
} from "@/utils/LyricsProcessor";

declare const $message: any;
declare const $player: any;

interface Artist {
  id: number;
  name: string;
  [key: string]: any;
}

interface Album {
  id: number;
  name: string;
  picUrl: string;
  [key: string]: any;
}

interface SongData {
  id: number;
  name: string;
  artist: Artist[];
  album: Album;
  alia?: string[];
  time: string;
  fee: number;
  pc?: any;
  mv?: number;
  [key: string]: any;
}

interface PlaySongTime {
  currentTime: number;
  duration: number;
  barMoveDistance: number;
  songTimePlayed: string;
  songTimeDuration: string;
}

interface PersistData {
  searchHistory: string[];
  personalFmMode: boolean;
  personalFmData: SongData | Record<string, never>;
  playListMode: string;
  likeList: number[];
  playlists: SongData[];
  playSongIndex: number;
  playSongMode: "normal" | "random" | "single";
  playSongTime: PlaySongTime;
  playVolume: number;
  playVolumeMute: number;
  playlistState: number;
  playHistory: SongData[];
}

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
  catList: Record<string, any>;
  highqualityCatList: any[];
  spectrumsData: number[];
  spectrumsScaleData: number;
  lowFreqVolume: number;
  isLoadingSong: boolean;
  preloadedSongIds: Set<number>;
  autoMixState: AutoMixStateData;
  persistData: PersistData;
}

const useMusicDataStore = defineStore("musicData", {
  state: (): MusicDataState => {
    return {
      showBigPlayer: false,
      showPlayBar: true,
      showPlayList: false,
      playState: false,
      songLyric: {
        hasLrcTran: true,
        hasLrcRoma: true,
        hasYrc: false,
        hasYrcTran: true,
        hasYrcRoma: true,
        hasTTML: false,
        lrc: [],
        yrc: [],
        ttml: [],
        lrcAMData: [],
        yrcAMData: [],
        formattedLrc: "",
        processedLyrics: [],
        settingsHash: "true-false-false",
      },
      playSongLyricIndex: -1,
      dailySongsData: [],
      catList: {},
      highqualityCatList: [],
      spectrumsData: [],
      spectrumsScaleData: 1,
      lowFreqVolume: 0,
      isLoadingSong: false,
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
      persistData: {
        searchHistory: [],
        personalFmMode: false,
        personalFmData: {},
        playListMode: "list",
        likeList: [],
        playlists: [],
        playSongIndex: 0,
        playSongMode: "normal",
        playSongTime: {
          currentTime: 0,
          duration: 0,
          barMoveDistance: 0,
          songTimePlayed: "00:00",
          songTimeDuration: "00:00",
        },
        playVolume: 0.7,
        playVolumeMute: 0,
        playlistState: 0,
        playHistory: [],
      },
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
    getDailySongs(state): SongData[] {
      return state.dailySongsData;
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
      return state.persistData.playSongTime;
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
      this.songLyric = {
        lrc: [],
        yrc: [],
        lrcAMData: [],
        yrcAMData: [],
        hasTTML: false,
        ttml: [],
        hasLrcTran: false,
        hasLrcRoma: false,
        hasYrc: false,
        hasYrcTran: false,
        hasYrcRoma: false,
        formattedLrc: "",
        processedLyrics: [],
        settingsHash: "",
      } as unknown as SongLyric;
      this.playSongLyricIndex = -1;
    },

    preloadUpcomingSongs() {
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
        getMusicUrl(songData.id)
          .then((res: any) => {
            const url = res.data[0]?.url?.replace(/^http:/, "https:");
            // 检测是否为无版权/灰化歌曲（无URL）或试听版本（jd-musicrep-ts）
            if (!url || url.includes("jd-musicrep-ts")) {
              if (!url) {
                console.log(`${songData.name} 无版权，尝试通过 UNM 解灰`);
              } else {
                console.log(`${songData.name} 是试听版，尝试通过 UNM 获取完整版`);
              }
              // 触发 UNM 逻辑
              return getMusicNumUrl(songData.id)
                .then((unmRes: any) => {
                  if (unmRes.data?.url) {
                    return {
                      id: songData.id,
                      name: songData.name,
                      url: unmRes.data.url.replace(/^http:/, "https:"),
                    };
                  }
                  // UNM 失败则降级
                  if (!url) {
                    console.warn(`${songData.name} UNM 解灰失败，跳过预加载`);
                    return null;
                  }
                  console.warn(`${songData.name} UNM 获取失败，使用试听版`);
                  return {
                    id: songData.id,
                    name: songData.name,
                    url,
                  };
                })
                .catch((unmErr: any) => {
                  console.warn(`${songData.name} UNM 请求失败:`, unmErr);
                  if (!url) {
                    return null;
                  }
                  return {
                    id: songData.id,
                    name: songData.name,
                    url,
                  };
                });
            }
            return {
              id: songData.id,
              name: songData.name,
              url,
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
          this.persistData.playSongIndex = 0;
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
                this.persistData.playSongIndex = 0;
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
      this.preloadedSongIds.clear();
      getAudioPreloader().cleanup();
      // 切换播放列表时，清空旧歌词，等待新歌曲歌词加载
      this.resetSongLyricState();
    },

    setDailySongs(value: any[]) {
      if (value) {
        this.dailySongsData = [];
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
      if (value) {
        try {
          if (!value.lrc || value.lrc.length === 0) {
            console.log("注意：歌词数据中缺少lrc数组，尝试从yrc创建");
            if (value.yrc && value.yrc.length > 0) {
              value.lrc = value.yrc.map((yrcLine: ParsedYrcLine) => ({
                time: yrcLine.time,
                content: yrcLine.TextContent,
              }));
              console.log("已从yrc数据创建lrc数组");
            } else {
              value.lrc = [
                { time: 0, content: "暂无歌词" },
                { time: 999, content: "No Lyrics Available" },
              ];
              console.log("创建了占位符lrc数组");
            }
          }

          if (!value.lrcAMData || value.lrcAMData.length === 0) {
            if (value.yrcAMData && value.yrcAMData.length > 0) {
              console.log("使用yrcAMData作为lrcAMData的备用");
              value.lrcAMData = [...value.yrcAMData];
            } else {
              console.log("创建基本的lrcAMData数组");
              value.lrcAMData = value.lrc.map((line: ParsedLrcLine) => ({
                startTime: line.time * 1000,
                endTime: (line.time + 5) * 1000,
                words: [
                  {
                    word: line.content,
                    startTime: line.time * 1000,
                    endTime: (line.time + 5) * 1000,
                  },
                ],
                translatedLyric: "",
                romanLyric: "",
                isBG: false,
                isDuet: false,
              }));
            }
          }

          if (value.hasTTML === undefined) {
            value.hasTTML = false;
          }
          if (value.ttml === undefined) {
            value.ttml = [];
          }

          console.time("预处理歌词");
          const settings = settingStore();
          try {
            preprocessLyrics(value, {
              showYrc: settings.showYrc,
              showRoma: settings.showRoma,
              showTransl: settings.showTransl,
            });
            console.log("歌词数据预处理完成");
          } catch (err) {
            console.warn("歌词预处理出错，将使用原始数据:", err);
          }
          console.timeEnd("预处理歌词");

          this.songLyric = value;
          console.log("歌词数据已存储到store:", this.songLyric);
        } catch (err) {
          $message.error(getLanguageData("getLrcError"));
          console.error(getLanguageData("getLrcError"), err);

          this.songLyric = {
            lrc: [
              { time: 0, content: "加载歌词时出错" },
              { time: 999, content: "Error loading lyrics" },
            ],
            yrc: [],
            lrcAMData: [
              {
                startTime: 0,
                endTime: 5000,
                words: [{ word: "加载歌词时出错", startTime: 0, endTime: 5000 }],
                translatedLyric: "",
                romanLyric: "",
                isBG: false,
                isDuet: false,
              },
            ],
            yrcAMData: [],
            hasTTML: false,
            ttml: [],
            hasLrcTran: false,
            hasLrcRoma: false,
            hasYrc: false,
            hasYrcTran: false,
            hasYrcRoma: false,
            formattedLrc: "",
            processedLyrics: [],
            settingsHash: "",
          };
        }
      } else {
        console.log("该歌曲暂无歌词");
        this.songLyric = {
          lrc: [],
          yrc: [],
          lrcAMData: [],
          yrcAMData: [],
          hasTTML: false,
          ttml: [],
          hasLrcTran: false,
          hasLrcRoma: false,
          hasYrc: false,
          hasYrcTran: false,
          hasYrcRoma: false,
          formattedLrc: "",
          processedLyrics: [],
          settingsHash: "",
        };
      }
    },

    setPlaySongTime(value: { currentTime: number; duration: number }) {
      this.persistData.playSongTime.currentTime = value.currentTime;
      this.persistData.playSongTime.duration = value.duration;
      if (value.duration === 0) {
        this.persistData.playSongTime.barMoveDistance = 0;
      } else {
        this.persistData.playSongTime.barMoveDistance = (value.currentTime / value.duration) * 100;
      }

      if (!Number.isNaN(this.persistData.playSongTime.barMoveDistance)) {
        this.persistData.playSongTime.songTimePlayed = getSongPlayingTime(
          (value.duration / 100) * this.persistData.playSongTime.barMoveDistance,
        );
        this.persistData.playSongTime.songTimeDuration = getSongPlayingTime(value.duration);
      }

      const setting = settingStore();
      const lrcType = !this.songLyric.hasYrc || !setting.showYrc;
      const lyrics = lrcType ? this.songLyric.lrc : this.songLyric.yrc;

      if (!lyrics || !lyrics.length) {
        this.playSongLyricIndex = -1;
        return;
      }

      let currentIndex = this.playSongLyricIndex;
      const offsetTime = value.currentTime + (setting.lyricTimeOffset ?? 0) / 1000;

      if (currentIndex > 0 && lyrics[currentIndex]?.time > offsetTime) {
        currentIndex = -1;
      }

      while (currentIndex < lyrics.length - 1 && lyrics[currentIndex + 1].time <= offsetTime) {
        currentIndex++;
      }

      this.playSongLyricIndex = currentIndex;
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
      if (autoMix.isCrossfading()) {
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
        if (listMode === "normal") {
          this.persistData.playSongIndex += type === "next" ? 1 : -1;
        } else if (listMode === "random") {
          this.persistData.playSongIndex = Math.floor(Math.random() * listLength);
        } else if (listMode === "single") {
          console.log("单曲循环模式");
          fadePlayOrPause($player, "play", this.persistData.playVolume);
        } else {
          $message.error(getLanguageData("playError"));
        }
        if (listMode !== "single") {
          if (this.persistData.playSongIndex < 0) {
            this.persistData.playSongIndex = listLength - 1;
          } else if (this.persistData.playSongIndex >= listLength) {
            this.persistData.playSongIndex = 0;
            soundStop($player);
            fadePlayOrPause($player, "play", this.persistData.playVolume);
          }
          if (listLength > 1) {
            soundStop($player);
          }
          // 已经切换到下一首/上一首歌曲，先清空旧歌词，等待新歌词加载
          this.resetSongLyricState();
          nextTick().then(() => {
            this.setPlayState(true);
          });
        }
      }
    },

    addSongToPlaylists(value: SongData, play: boolean = true) {
      if (typeof $player !== "undefined") soundStop($player);
      const index = this.persistData.playlists.findIndex((o) => o.id === value.id);
      try {
        if (value.id !== this.persistData.playlists[this.persistData.playSongIndex]?.id) {
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
        this.persistData.playSongIndex = index;
      } else {
        this.persistData.playlists.push(value);
        this.persistData.playSongIndex = this.persistData.playlists.length - 1;
      }
      play ? this.setPlayState(true) : null;
    },

    addSongToNext(value: SongData) {
      this.persistData.playSongMode = "normal";
      const index = this.persistData.playlists.findIndex((o) => o.id === value.id);
      if (index !== -1) {
        console.log(index);
        if (index === this.persistData.playSongIndex) return true;
        if (index < this.persistData.playSongIndex) this.persistData.playSongIndex--;
        const arr = this.persistData.playlists.splice(index, 1)[0];
        this.persistData.playlists.splice(this.persistData.playSongIndex + 1, 0, arr);
      } else {
        this.persistData.playlists.splice(this.persistData.playSongIndex + 1, 0, value);
      }
      $message.success(value.name + " " + getLanguageData("addSongToNext"));
    },

    removeSong(index: number) {
      if (typeof $player === "undefined") return false;
      const songId = this.persistData.playlists[index].id;
      const name = this.persistData.playlists[index].name;
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
    },
  },
  persist: [
    {
      storage: localStorage,
      pick: ["persistData"],
      omit: ["preloadedSongIds"],
      afterHydrate: (ctx: any) => {
        ctx.store.preloadedSongIds = new Set();
      },
    },
  ],
});

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useMusicDataStore, import.meta.hot));
}

export default useMusicDataStore;
