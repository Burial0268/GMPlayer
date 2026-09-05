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
  SoundManager,
  cancelNativeQueuePrefill,
  publishNativeManifest,
  clearNativeManifest,
  reseedRandomTraversal,
  publishSessionControls,
} from "@/utils/AudioContext";
import {
  NativeRustSound,
  isAudioBackendRuntimeAvailable,
} from "@/utils/tauri/audio/nativeRustSound";
import getLanguageData from "@/utils/getLanguageData";
import {
  likedPlaylistId,
  notifyPlaylistNeedsReconcile,
  notifyTrackInPlaylist,
} from "@/utils/playlistMutations";
import type { SongLyric } from "@/utils/LyricsProcessor";
import useMusicLyricStore from "./musicLyric";
import useMusicPersistedDataStore from "./musicPersistedData";
import useMusicPlaybackDataStore from "./musicPlaybackData";
import useMusicPlaybackResumeStore from "./musicPlaybackResume";
import useLocalLibraryStore from "./localLibrary";
import {
  createDefaultPlaySongTime,
  type PersistData,
  type PlaySongTime,
  type SongData,
} from "./musicTypes";
import { asRawEntries, asRawEntry } from "@/utils/rawEntry";
import { hintTracks } from "@/utils/ncmPrefetch";

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
  isLoadingSong: boolean;
  loadingStage: "idle" | "resolving" | "buffering" | "stalled" | "error";
  preloadedSongIds: Set<number>;
  // persistData.likeList 的查询索引。数组本身仍是持久化的唯一真源，
  // 这里只是派生索引：Array.includes 是 O(n)，而且会把整个数组登记为依赖，
  // 导致收藏任意一首歌就让所有列表行重新渲染。
  likeSet: Set<number>;
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
      isLoadingSong: false,
      loadingStage: "idle",
      preloadedSongIds: new Set(),
      likeSet: new Set(persistedStore.persistData.likeList),
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
                // 集合只用来避免重复预取；上限防止长会话中无限增长，
                // 清空后重新预取会直接命中浏览器 HTTP 缓存，代价可忽略。
                if (this.preloadedSongIds.size >= 500) this.preloadedSongIds.clear();
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
      // 个人 FM 的下一首由服务端决定，后端 planner 无法预知：
      // 进入时必须撤销 manifest，退出时重新发布，否则后台会按旧列表推进。
      if (value) {
        clearNativeManifest();
      }
      if (value) {
        if (typeof $player !== "undefined") soundStop($player);
        if ((this.persistData.personalFmData as SongData)?.id) {
          this.persistData.playlists = [];
          this.persistData.playlists.push(this.persistData.personalFmData as SongData);
          this.commitPlaySongIndex(0);
        } else {
          this.setPersonalFmData();
        }
      } else {
        publishNativeManifest({ force: true });
      }
    },

    setPersonalFmData() {
      try {
        const songName = (this.getPersonalFmData as SongData)?.name;
        getPersonalFm().then((res: any) => {
          if (res.data[0]) {
            const data = res.data[2] || res.data[0];
            const fmData: SongData = asRawEntry({
              id: data.id,
              name: data.name,
              artist: data.artists,
              album: data.album,
              alia: data.alias,
              time: getSongTime(data.duration),
              fee: data.fee,
              pc: data.pc ? data.pc : null,
              mv: data.mvid,
            });
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
            // Trashing an FM track also takes it out of 我喜欢的音乐 when it was
            // in there. Reconcile rather than patch: the exact effect is
            // Netease's to decide, and a guessed count would stay wrong until
            // the next navigation.
            notifyPlaylistNeedsReconcile(likedPlaylistId());
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
          this.likeSet = new Set(res.ids);
        });
      }
    },

    getSongIsLike(target: number | SongData): boolean {
      // A local file's like state is its own set, keyed on the file's locator.
      // It cannot live in `likeList`: login replaces that array wholesale from
      // `/likelist`, so a local entry would be erased at every sign-in — and the
      // symptom ("my local favourites disappeared after logging in") points at
      // the login code rather than at storage.
      const local = this.localKeyOf(target);
      if (local) return useLocalLibraryStore().isFavourite(local);
      return this.likeSet.has(this.songIdOf(target));
    },

    /**
     * The locator for a track, when it is a local file.
     *
     * Prefers a whole row, because a *library* row is not in the queue and an id
     * alone cannot be resolved to a path — hearts in the local song list would
     * all read "not liked" and a press would take the Netease path and demand a
     * login. Callers that only have an id (the player chrome, which is always
     * showing the current track) still work through the queue lookup.
     */
    localKeyOf(target: number | SongData | null | undefined): string | null {
      if (target && typeof target === "object") return target.local?.uri ?? null;
      return this.localSongRef(Number(target));
    },

    songIdOf(target: number | SongData): number {
      return Number(target && typeof target === "object" ? target.id : target);
    },

    /**
     * The locator for `id`, when it names a local file.
     *
     * Looks in the queue and the currently playing track, which is where every
     * id-only caller gets its ids from. A miss just means "treat it as a Netease
     * id", which is the correct default.
     */
    localSongRef(id: number): string | null {
      if (this.playingSongId === id) {
        const uri = (this.getPlaySongData as SongData | undefined)?.local?.uri;
        if (uri) return uri;
      }
      // Only local rows can have a non-positive id, so the scan is skipped
      // entirely for the overwhelmingly common Netease case.
      if (id > 0) return null;
      const match = this.persistData.playlists.find((song) => Number(song?.id) === id);
      return match?.local?.uri ?? null;
    },

    /**
     * Record a like state the account already has, without calling the API.
     *
     * Used when the *backend* performed the `/like` — the notification's heart
     * works with no WebView alive, so by the time the frontend hears about it the
     * call is done. Refetching the likelist to learn what we were just told would
     * be a second round trip; `changeLikeList` remains the path for a change the
     * frontend originates.
     */
    applyLikeState(id: number, like: boolean) {
      const exists = this.likeSet.has(id);
      if (like === exists) return;
      const list = this.persistData.likeList;
      if (like) {
        list.push(id);
        this.likeSet.add(id);
      } else {
        const index = list.indexOf(id);
        if (index !== -1) list.splice(index, 1);
        this.likeSet.delete(id);
      }
      // 我喜欢的音乐 gained or lost a track. The account write already happened —
      // in Rust, with the page possibly not even alive at the time — so this is
      // the only moment the rest of the UI can hear about it.
      notifyTrackInPlaylist(likedPlaylistId(), this.knownSongById(id) ?? id, like);
    },

    /**
     * A full song row for `id`, if the store happens to hold one.
     *
     * Only used to make a patched row appear complete straight away. A miss is
     * not a failure: the quiet reconcile that follows fetches the real row, so
     * the id alone is enough to get the count and the refetch right.
     */
    knownSongById(id: number): SongData | undefined {
      if (this.playingSongId === id && this.getPlaySongData?.id === id) {
        return this.getPlaySongData as SongData;
      }
      return this.persistData.playlists.find((song) => song.id === id);
    },

    async changeLikeList(target: number | SongData, like: boolean = true) {
      const id = this.songIdOf(target);
      // Local files first, and without a login check: a file on disk is not the
      // account's to authorize. Nothing here touches `persistData.likeList` or
      // sends a request — the set lives in Rust next to the library index, which
      // is why it survives logout and an account switch.
      const localKey = this.localKeyOf(target);
      if (localKey) {
        const local = useLocalLibraryStore();
        const changed = await local.setFavourite(localKey, like);
        if (changed) {
          $message.info(getLanguageData(like ? "loveSong" : "loveSongRemove"));
        } else if (like) {
          $message.info(getLanguageData("loveSongRepeat"));
        }
        // The OS session draws a heart for the playing track and derives it from
        // the same set, so it has to hear about the change.
        publishSessionControls();
        return;
      }

      const user = userStore();
      const list = this.persistData.likeList;
      const exists = this.likeSet.has(id);
      if (!user.userLogin) {
        $message.error(getLanguageData("needLogin"));
        return;
      }
      // Captured before the list is edited: on an un-like the row is about to be
      // dropped from the queue lookup this reads.
      const song = this.knownSongById(id);
      let changed = false;
      try {
        const res = await setLikeSong(id, like);
        if (res.code === 200) {
          if (like && !exists) {
            list.push(id);
            this.likeSet.add(id);
            changed = true;
            $message.info(getLanguageData("loveSong"));
          } else if (!like && exists) {
            list.splice(list.indexOf(id), 1);
            this.likeSet.delete(id);
            changed = true;
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
      // The OS session renders a heart for the playing track, so it has to be
      // told when the list it reflects changed under it.
      publishSessionControls();
      // Reported only for a change the account actually accepted — a refused
      // write must not move a row or a count. Gated on `changed` rather than on
      // `res.code` so a no-op ("already liked") stays silent too.
      if (changed) notifyTrackInPlaylist(likedPlaylistId(), song ?? id, like);
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
      if (value.length === 0) {
        this.clearPlaylists();
        return;
      }
      const autoMix = getAutoMixEngine();
      if (autoMix.isHandoffActive()) autoMix.cancelCrossfade();
      cancelNativeQueuePrefill();
      // asRawEntries 顺带完成 slice 的拷贝职责：新数组 + 逐项标记为 raw。
      this.persistData.playlists = asRawEntries(value);
      this.persistData.playSongIndex = Math.min(
        Math.max(0, this.persistData.playSongIndex),
        Math.max(0, this.persistData.playlists.length - 1),
      );
      this.resetPlaySongTime();
      this.preloadedSongIds.clear();
      getAudioPreloader().cleanup();
      // 整表替换视为一次新的随机序：重新播种，避免沿用旧列表的排列。
      reseedRandomTraversal();
      publishNativeManifest();
      // 切换播放列表时，清空旧歌词，等待新歌曲歌词加载
      this.resetSongLyricState();
    },

    /**
     * 采纳一起听房间的共享播放列表。
     *
     * 一起听的语义是「一份列表，两个消费者」：任何一方加歌，两边都加。所以本地
     * 要跟随房间的列表，但**跟随不等于重来** —— 别人在我前面插了一首歌，不该让
     * 我正在听的这首从头开始。
     *
     * 与 setPlaylists 的区别，逐条都是为了这个：
     * - 按 playingSongId 重新锚定索引，而不是按位置 clamp（别人插队会让位置整体后移）
     * - 不 resetPlaySongTime：进度是本地播放状态，与列表内容无关
     * - 不 resetSongLyricState：歌没变，歌词不该闪
     * - 不 reseedRandomTraversal：重新洗牌就是 rebase，不是 merge
     */
    adoptSharedPlaylist(value: SongData[]): boolean {
      if (!value?.length) return false;
      const autoMix = getAutoMixEngine();
      if (autoMix.isHandoffActive()) autoMix.cancelCrossfade();
      cancelNativeQueuePrefill();

      const activeSongId = this.playingSongId ?? this.getPlaySongData?.id ?? null;
      this.persistData.playlists = asRawEntries(value);

      const nextIndex =
        activeSongId === null
          ? -1
          : this.persistData.playlists.findIndex((song) => song.id === activeSongId);
      if (nextIndex >= 0) {
        this.persistData.playSongIndex = nextIndex;
        this.checkpointPlaySongTime(true);
      } else {
        // 正在放的歌被移出了共享列表：夹回范围内，让 Player 的 watcher 接手换歌。
        this.persistData.playSongIndex = Math.min(
          Math.max(0, this.persistData.playSongIndex),
          this.persistData.playlists.length - 1,
        );
      }

      this.preloadedSongIds.clear();
      getAudioPreloader().cleanup();
      publishNativeManifest();
      return true;
    },

    setDailySongs(value: any[], date = getDailySongsDate()) {
      if (value) {
        this.dailySongsData = [];
        this.dailySongsDate = date;
        value.forEach((v) => {
          this.dailySongsData.push(
            asRawEntry({
              id: v.id,
              name: v.name,
              artist: v.ar,
              album: v.al,
              alia: v.alia,
              time: getSongTime(v.dt),
              fee: v.fee,
              pc: v.pc ? v.pc : null,
              mv: v.mv ? v.mv : null,
            }),
          );
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
      useMusicPlaybackResumeStore().saveSession(songId, index, playbackStore.playSongTime);
      this.hintUpcomingSongs(index);
      return true;
    },

    /**
     * 告诉内嵌协议层：接下来大概会用到哪几首歌的歌词和详情。
     *
     * 每次切歌都要取歌词，而歌词是永远不变的——所以在当前这首还在播的时候就先取
     * 好，切过去就是 0 请求。这里只发提示，不等结果：Rust 侧对提示做了限流（并发
     * 有上限、被网易限速时直接丢弃、排不上就不排队），所以猜错了也不会让用户正在
     * 等的请求变慢。
     *
     * **只提示后面的歌，不提示当前这首。** 当前这首的歌词/详情马上就会有真实请求
     * 发出去，提示和它是同一瞬间，`inflight` 会把两者合成一个请求——也就是说提示
     * 当前这首不省任何东西，只是白占一个并发额度，把真正有用的提示挤掉。
     *
     * 播放链路的 URL 不在这里预取——它是带签名会过期的，那是
     * `NativeQueuePrefill` 的事。
     *
     * 随机模式下不猜下一首：顺序由 Rust planner 的 permutation 决定，这里按下标
     * 往后数会猜到无关的歌，白花请求。
     */
    hintUpcomingSongs(currentIndex: number) {
      const playlist = this.persistData.playlists;
      if (playlist.length < 2) return;
      // 私人 FM 的下一首要请求才知道；随机模式的顺序不在前端手里。
      if (this.persistData.playSongMode !== "normal" || this.persistData.personalFmMode) return;

      const AHEAD = 2;
      const ids: Array<number | string> = [];
      for (let i = 1; i <= AHEAD; i++) {
        const song = playlist[(currentIndex + i) % playlist.length];
        // 本地曲目没有网易 id，提示它等于让协议层去查一首不存在的歌。
        if (song?.local?.uri) continue;
        if (song?.id !== undefined) ids.push(song.id);
      }
      if (ids.length) hintTracks(ids);
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
      // 进入随机模式时重新播种，让本次随机与上一轮不同；
      // 然后重发 manifest，把新的遍历顺序交给后端 planner。
      if (this.persistData.playSongMode === "random") {
        reseedRandomTraversal();
      }
      publishNativeManifest({ force: true });
      // The OS session shows this too, and the backend's planner has to honour
      // it on the next hop. One writer: see NativeSessionControlsSync.
      publishSessionControls();
      $message.info(getLanguageData(value!), {
        icon: () =>
          h(NIcon, null, {
            default: () => h(modeObj[this.persistData.playSongMode]),
          }),
      });
    },

    setPlaySongIndex(type: "next" | "prev") {
      // Cancel AutoMix crossfade on manual skip
      const autoMix = getAutoMixEngine();
      if (autoMix.isHandoffActive()) {
        autoMix.cancelCrossfade();
      }
      if (this.persistData.personalFmMode) {
        if (typeof $player !== "undefined") soundStop($player);
        this.setPersonalFmData();
        return true;
      } else {
        const listLength = this.persistData.playlists.length;
        if (listLength === 0) {
          this.clearPlaylists();
          return false;
        }

        const activePlayer = typeof $player !== "undefined" ? $player : undefined;
        const listMode = this.persistData.playSongMode;
        let nextIndex = this.persistData.playSongIndex;
        if (listMode === "normal") {
          nextIndex += type === "next" ? 1 : -1;
        } else if (listMode === "random") {
          nextIndex = Math.floor(Math.random() * listLength);
        } else if (listMode === "single") {
          console.log("单曲循环模式");
          const currentSong = this.persistData.playlists[this.persistData.playSongIndex];
          if (activePlayer && SoundManager.getSongId(activePlayer) === Number(currentSong?.id)) {
            soundStop(activePlayer);
            fadePlayOrPause(activePlayer, "play", this.persistData.playVolume);
            this.isLoadingSong = false;
            this.setPlayState(true);
            return true;
          }
          this.isLoadingSong = true;
          this.setPlayState(true);
          if (currentSong && typeof window.$getPlaySongData === "function") {
            void window.$getPlaySongData(currentSong);
          }
          return !!currentSong;
        } else {
          $message.error(getLanguageData("playError"));
          return false;
        }

        if (nextIndex < 0) {
          nextIndex = listLength - 1;
        } else if (nextIndex >= listLength) {
          nextIndex = 0;
        }

        const currentSong = this.persistData.playlists[this.persistData.playSongIndex];
        if (nextIndex === this.persistData.playSongIndex) {
          const activeSongId = activePlayer ? SoundManager.getSongId(activePlayer) : null;
          if (activePlayer && activeSongId === Number(currentSong?.id)) {
            soundStop(activePlayer);
            fadePlayOrPause(activePlayer, "play", this.persistData.playVolume);
            this.isLoadingSong = false;
            this.setPlayState(true);
            return true;
          }
          this.isLoadingSong = true;
          this.setPlayState(true);
          if (currentSong && typeof window.$getPlaySongData === "function") {
            void window.$getPlaySongData(currentSong);
          }
          return !!currentSong;
        }

        if (activePlayer) soundStop(activePlayer);
        this.isLoadingSong = true;
        if (!this.commitPlaySongIndex(nextIndex)) {
          this.isLoadingSong = false;
          this.setPlayState(false);
          return false;
        }
        // 已经切换到下一首/上一首歌曲，先清空旧歌词，等待新歌词加载
        this.resetSongLyricState();
        nextTick().then(() => {
          if (this.persistData.playlists.length > 0) this.setPlayState(true);
        });
        return true;
      }
    },

    selectPlaySongByIndex(index: number) {
      const autoMix = getAutoMixEngine();
      if (autoMix.isHandoffActive()) autoMix.cancelCrossfade();
      if (
        !Number.isInteger(index) ||
        index < 0 ||
        index >= this.persistData.playlists.length ||
        (index === this.persistData.playSongIndex &&
          SoundManager.getSongId(window.$player) ===
            Number(this.persistData.playlists[index]?.id) &&
          this.loadingStage !== "error")
      ) {
        return;
      }
      if (index === this.persistData.playSongIndex) {
        if (typeof $player !== "undefined") soundStop($player);
        this.isLoadingSong = true;
        this.setPlayState(true);
        const song = this.persistData.playlists[index];
        if (song && typeof window.$getPlaySongData === "function") {
          void window.$getPlaySongData(song);
        }
        return;
      }
      if (typeof $player !== "undefined") soundStop($player);
      this.commitPlaySongIndex(index);
      this.resetSongLyricState();
      this.isLoadingSong = true;
      this.setPlayState(true);
    },

    addSongToPlaylists(value: SongData, play: boolean = true) {
      const autoMix = getAutoMixEngine();
      if (autoMix.isHandoffActive()) autoMix.cancelCrossfade();
      cancelNativeQueuePrefill();
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
        this.persistData.playlists.push(asRawEntry(value));
        this.commitPlaySongIndex(this.persistData.playlists.length - 1);
      }
      publishNativeManifest();
      if (play) this.setPlayState(true);
    },

    addSongToNext(value: SongData) {
      cancelNativeQueuePrefill();
      this.persistData.playSongMode = "normal";
      const autoMix = getAutoMixEngine();
      // 与 addSongToPlaylists 一致：以 playingSongId 为「正在发声的歌」的唯一判据。
      // playSongIndex 可能因 setPlaylists 的 clamp 而落在别的歌上。
      const activeSongId = this.playingSongId ?? this.getPlaySongData?.id;
      const activeIndex = this.persistData.playlists.findIndex((o) => o.id === activeSongId);
      let insertAfterIndex = activeIndex >= 0 ? activeIndex : this.persistData.playSongIndex;
      // 只有真正在交叉淡入（incoming 已发声）时才锚定到 incoming 歌曲。
      // 原生 AutoMix 在开播即 prepare，_activeTransition 会存在一整首歌，
      // 此时发声的仍是 from 歌曲，用 toSongId 当锚点会整体后移一位。
      if (autoMix.isCrossfading()) {
        const autoMixTargetIndex = autoMix.resolveActiveTransitionTargetIndex(insertAfterIndex);
        if (autoMixTargetIndex >= 0) {
          insertAfterIndex = autoMixTargetIndex;
        }
      }

      const index = this.persistData.playlists.findIndex((o) => o.id === value.id);
      if (index !== -1) {
        // 已经是正在播放的歌、或已经排在下一位：无需移动。
        if (value.id === activeSongId || index === insertAfterIndex) return true;
        if (index < this.persistData.playSongIndex) this.persistData.playSongIndex--;
        const arr = this.persistData.playlists.splice(index, 1)[0];
        if (index < insertAfterIndex) insertAfterIndex--;
        const insertIndex = insertAfterIndex + 1;
        this.persistData.playlists.splice(insertIndex, 0, arr);
        if (insertIndex <= this.persistData.playSongIndex) this.persistData.playSongIndex++;
      } else {
        const insertIndex = insertAfterIndex + 1;
        this.persistData.playlists.splice(insertIndex, 0, asRawEntry(value));
        if (insertIndex <= this.persistData.playSongIndex) this.persistData.playSongIndex++;
      }
      // 已 prepare 但尚未发声的过渡指向的是旧的「下一首」，插队后必须作废，
      // 否则后端仍会切到原来那首。已在发声的 crossfade 不打断。
      if (!autoMix.isCrossfading() && autoMix.isHandoffActive()) {
        autoMix.cancelCrossfade();
      }
      publishNativeManifest();
      this.checkpointPlaySongTime(true);
      $message.success(value.name + " " + getLanguageData("addSongToNext"));
    },

    removeSong(index: number) {
      if (index < 0 || index >= this.persistData.playlists.length) return false;
      cancelNativeQueuePrefill();
      const songId = this.persistData.playlists[index].id;
      const name = this.persistData.playlists[index].name;
      const removedCurrentSong = index === this.persistData.playSongIndex;
      if (index < this.persistData.playSongIndex) {
        this.persistData.playSongIndex--;
      } else if (index === this.persistData.playSongIndex) {
        if (typeof $player !== "undefined") soundStop($player);
      }
      $message.success(name + " " + getLanguageData("removeSong"));
      this.persistData.playlists.splice(index, 1);
      this.preloadedSongIds.delete(songId);
      if (this.persistData.playlists.length === 0) {
        this.clearPlaylists();
        return true;
      }
      // Next index may have changed after removal
      getAudioPreloader().cleanup();
      if (this.persistData.playSongIndex >= this.persistData.playlists.length) {
        this.persistData.playSongIndex = 0;
        if (typeof $player !== "undefined") soundStop($player);
      }
      publishNativeManifest();
      if (removedCurrentSong) {
        // 索引现在指向后继歌曲（由 watcher 接手加载）；实际播放身份等待 load/adopt 提交。
        this.playingSongId = null;
        this.resetPlaySongTime();
      } else this.checkpointPlaySongTime(true);
      return true;
    },

    clearPlaylists() {
      const autoMix = getAutoMixEngine();
      if (autoMix.isHandoffActive()) {
        autoMix.cancelCrossfade();
      }
      cancelNativeQueuePrefill();
      // 强语义：清空必须在拆掉 sound 之前先撤销后端 manifest，
      // 否则 planner 仍持有整表并会继续推进。
      clearNativeManifest();
      getAudioPreloader().cleanup();

      const activePlayer = typeof window !== "undefined" ? window.$player : undefined;
      if (activePlayer instanceof NativeRustSound) {
        activePlayer.clearPlaybackQueue();
      }
      if (SoundManager.getCurrentSound()) {
        SoundManager.unload();
      } else if (activePlayer) {
        activePlayer.unload?.();
        window.$player = undefined;
      }

      this.persistData.personalFmMode = false;
      this.persistData.playlists = [];
      this.persistData.playSongIndex = 0;
      this.playingSongId = null;
      this.playState = false;
      this.isLoadingSong = false;
      this.loadingStage = "idle";
      this.preloadedSongIds.clear();
      this.resetPlaySongTime();
      this.resetSongLyricState();
      return true;
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
        this.persistData.playHistory.unshift(asRawEntry(data));
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
