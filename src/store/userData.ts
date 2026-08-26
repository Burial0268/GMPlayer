import { defineStore, acceptHMRUpdate } from "pinia";
import {
  userLogOut as apiUserLogOut,
  getUserLevel,
  getUserSubcount,
  getUserPlaylist,
  getUserArtistlist,
  getUserAlbum,
} from "@/api/user";
import { formatNumber, getLongTime } from "@/utils/timeTools";
import getLanguageData from "@/utils/getLanguageData";
import { asRawEntry } from "@/utils/rawEntry";

declare const $message: any;

interface UserData {
  userId?: number;
  [key: string]: any;
}

interface UserOtherData {
  level?: any;
  subcount?: any;
  [key: string]: any;
}

interface PlaylistItem {
  id: number;
  cover: string;
  name: string;
  artist: any;
  desc?: string;
  tags?: string[];
  playCount: string | number;
  trackCount?: number;
}

interface UserPlayLists {
  isLoading: boolean;
  has: boolean;
  own: PlaylistItem[];
  like: PlaylistItem[];
}

interface AlbumItem {
  id: number;
  cover: string;
  name: string;
  artist: any;
  time: string;
}

interface UserAlbum {
  isLoading: boolean;
  has: boolean;
  list: AlbumItem[];
}

interface ArtistItem {
  id: number;
  name: string;
  cover: string;
  size: number;
}

interface UserArtistLists {
  isLoading: boolean;
  has: boolean;
  list: ArtistItem[];
}

interface UserDataState {
  userLogin: boolean;
  cookie: string | null;
  userData: UserData;
  userOtherData: UserOtherData;
  userPlayLists: UserPlayLists;
  userAlbum: UserAlbum;
  userArtistLists: UserArtistLists;
}

/** 收藏专辑分页大小 */
const ALBUM_PAGE_SIZE = 30;
/** 收藏专辑分页并发上限，避免一次性打满接口 */
const ALBUM_PAGE_CONCURRENCY = 4;
/** 歌单请求的最小 limit，与旧实现保持一致 */
const PLAYLIST_MIN_LIMIT = 30;
/** 基于缓存订阅数推算 limit 时预留的余量（应对新建/收藏后计数滞后） */
const PLAYLIST_LIMIT_HEADROOM = 10;

/**
 * 进行中的请求（不放进 state，避免 Promise 被 Pinia 深度响应式代理）。
 * Sidebar / CoverLists / AddPlaylist / PaLikeSongs 等组件会在同一时刻各自触发拉取，
 * 复用同一个 Promise 可以把 N 次重复往返收敛成 1 次，同时保证每个调用方的 callback 都能执行。
 */
let playListsInFlight: Promise<boolean> | null = null;
let artistListsInFlight: Promise<boolean> | null = null;
let albumListsInFlight: Promise<boolean> | null = null;
let otherDataInFlight: Promise<void> | null = null;

const clearInFlight = () => {
  playListsInFlight = null;
  artistListsInFlight = null;
  albumListsInFlight = null;
  otherDataInFlight = null;
};

/** 有并发上限的批量任务，保持结果顺序 */
const runWithLimit = async <T>(
  count: number,
  limit: number,
  task: (index: number) => Promise<T>,
): Promise<T[]> => {
  const results = Array.from({ length: count }) as T[];
  let cursor = 0;
  const workers = Array.from({ length: Math.min(limit, count) }, async () => {
    while (cursor < count) {
      const index = cursor++;
      results[index] = await task(index);
    }
  });
  await Promise.all(workers);
  return results;
};

const toIdSet = (list: { id: number }[]): Set<number> => {
  const ids = new Set<number>();
  for (let i = 0; i < list.length; i++) ids.add(list[i].id);
  return ids;
};

const useUserDataStore = defineStore("userData", {
  state: (): UserDataState => {
    return {
      userLogin: false,
      cookie: null,
      userData: {},
      userOtherData: {},
      userPlayLists: {
        isLoading: false,
        has: false,
        own: [],
        like: [],
      },
      userAlbum: {
        isLoading: false,
        has: false,
        list: [],
      },
      userArtistLists: {
        isLoading: false,
        has: false,
        list: [],
      },
    };
  },
  getters: {
    getCookie(state): string | null {
      return state.cookie;
    },
    getUserData(state): UserData {
      return state.userData;
    },
    getUserOtherData(state): UserOtherData {
      return state.userOtherData;
    },
    getUserPlayLists(state): UserPlayLists {
      return state.userPlayLists;
    },
    getUserArtistLists(state): UserArtistLists {
      return state.userArtistLists;
    },
    getUserAlbumLists(state): UserAlbum {
      return state.userAlbum;
    },
    // 以下 4 个 id 索引为缓存 getter，仅在对应列表整体替换时重建，
    // 供「是否已收藏 / 是否可删除」等判断使用，避免每次都做 O(n) 线性查找
    getOwnPlayListIds(state): Set<number> {
      return toIdSet(state.userPlayLists.own);
    },
    getLikedPlayListIds(state): Set<number> {
      return toIdSet(state.userPlayLists.like);
    },
    getUserAlbumIds(state): Set<number> {
      return toIdSet(state.userAlbum.list);
    },
    getUserArtistIds(state): Set<number> {
      return toIdSet(state.userArtistLists.list);
    },
  },
  actions: {
    setCookie(value: string) {
      window.localStorage.setItem("cookie", value);
      this.cookie = value;
    },
    setUserData(value: UserData) {
      this.userData = value;
    },
    setUserOtherData(): Promise<void> {
      if (!this.userLogin) return Promise.resolve();
      if (otherDataInFlight) return otherDataInFlight;
      otherDataInFlight = Promise.all([getUserLevel(), getUserSubcount()])
        .then(([level, subcount]) => {
          // 一次性整体赋值，只触发一次响应式更新
          this.userOtherData = { level: level?.data, subcount };
        })
        .catch((err) => {
          console.error(getLanguageData("getDataError"), err);
          $message.error(getLanguageData("getDataError"));
        })
        .finally(() => {
          otherDataInFlight = null;
        });
      return otherDataInFlight;
    },
    userLogOut() {
      this.userLogin = false;
      this.cookie = null;
      this.userData = {};
      this.userOtherData = {};
      // 清空上一个账号的列表数据，避免退出后仍展示/参与计算旧数据
      this.userPlayLists = { isLoading: false, has: false, own: [], like: [] };
      this.userAlbum = { isLoading: false, has: false, list: [] };
      this.userArtistLists = { isLoading: false, has: false, list: [] };
      clearInFlight();
      localStorage.removeItem("cookie");
      apiUserLogOut();
    },
    async setUserPlayLists(callback?: () => void) {
      if (!this.userLogin) {
        $message.error(getLanguageData("needLogin"));
        return;
      }
      if (!playListsInFlight) {
        playListsInFlight = this.loadUserPlayLists().finally(() => {
          playListsInFlight = null;
        });
      }
      if ((await playListsInFlight) && typeof callback === "function") callback();
    },
    /**
     * 歌单曲目数 ±delta。歌单写入后立即生效，无需重新拉取整份列表。
     *
     * 条目是 markRaw 的（见 `utils/rawEntry`）：写 `entry.trackCount` 不会触发任何
     * 响应式更新，所以这里整体替换数组槽位，而不是就地改字段。侧边栏与各处卡片
     * 因此能跟着开着的歌单页一起动。
     *
     * 只有自建歌单带 `trackCount`，收藏的歌单没有这个字段，所以只找 `own`。
     * 由 `utils/playlistMutations` 统一调用 —— 每个写入点各自记得改计数是行不通的，
     * 漏掉了也看不出来，直到数字对不上。
     */
    applyPlaylistTrackDelta(playlistId: number, delta: number) {
      if (!delta) return;
      const own = this.userPlayLists.own;
      const index = own.findIndex((item) => Number(item.id) === Number(playlistId));
      if (index === -1) return;
      const entry = own[index];
      if (typeof entry.trackCount !== "number") return;
      own[index] = asRawEntry({
        ...entry,
        trackCount: Math.max(0, entry.trackCount + delta),
      });
    },
    async setUserArtistLists(callback?: () => void) {
      if (!this.userLogin) {
        $message.error(getLanguageData("needLogin"));
        return;
      }
      if (!artistListsInFlight) {
        artistListsInFlight = this.loadUserArtistLists().finally(() => {
          artistListsInFlight = null;
        });
      }
      if ((await artistListsInFlight) && typeof callback === "function") callback();
    },
    async setUserAlbumLists(callback?: () => void) {
      if (!this.userLogin) {
        $message.error(getLanguageData("needLogin"));
        return;
      }
      if (!albumListsInFlight) {
        albumListsInFlight = this.loadUserAlbumLists().finally(() => {
          albumListsInFlight = null;
        });
      }
      if ((await albumListsInFlight) && typeof callback === "function") callback();
    },
    /** 依据缓存的订阅数推算歌单 limit；返回 0 表示无缓存、需要问接口 */
    resolvePlayListLimit(): number {
      const subcount = this.userOtherData?.subcount;
      const total = (subcount?.createdPlaylistCount ?? 0) + (subcount?.subPlaylistCount ?? 0);
      return total > 0 ? Math.max(PLAYLIST_MIN_LIMIT, total + PLAYLIST_LIMIT_HEADROOM) : 0;
    },
    /** @internal 由 setUserPlayLists 调度，勿直接调用 */
    async loadUserPlayLists(): Promise<boolean> {
      this.userPlayLists.isLoading = true;
      try {
        const userId = this.userData.userId!;
        const cachedLimit = this.resolvePlayListLimit();
        // 有缓存订阅数时直接省掉 subcount 请求；没有时也让它与首次歌单请求并发，
        // 而不是像旧实现那样串行等待。失败不阻断歌单本身
        const subcountRequest = cachedLimit ? null : getUserSubcount().catch(() => null);
        const limit = cachedLimit || PLAYLIST_MIN_LIMIT;
        let res = await getUserPlaylist(userId, limit);
        const maybeTruncated = res?.playlist?.length >= limit;
        // 缓存计数可能落后于真实数量（新建/收藏歌单后），取满时用权威 subcount 补齐一次
        let subcount = subcountRequest ? await subcountRequest : null;
        if (!subcount && maybeTruncated) subcount = await getUserSubcount().catch(() => null);
        if (subcount) {
          this.userOtherData = { ...this.userOtherData, subcount };
          const total = (subcount.createdPlaylistCount ?? 0) + (subcount.subPlaylistCount ?? 0);
          if (maybeTruncated && total > limit) {
            res = await getUserPlaylist(userId, total + PLAYLIST_LIMIT_HEADROOM);
          }
        }
        if (!res?.playlist) {
          $message.info(getLanguageData("getDaraEmpty"));
          return false;
        }
        // 先在普通数组上构建，最后整体赋值：把 O(n) 次响应式写入降为 1 次
        const playlist: any[] = res.playlist;
        const own: PlaylistItem[] = [];
        const like: PlaylistItem[] = [];
        for (let i = 0; i < playlist.length; i++) {
          const v = playlist[i];
          if (v.creator?.userId === userId) {
            own.push(
              asRawEntry({
                id: v.id,
                cover: v.coverImgUrl,
                name: v.name,
                artist: v.creator,
                desc: v.description,
                tags: v.tags,
                playCount: formatNumber(v.playCount),
                trackCount: v.trackCount,
              }),
            );
          } else {
            like.push(
              asRawEntry({
                id: v.id,
                cover: v.coverImgUrl,
                name: v.name,
                artist: v.creator,
                playCount: formatNumber(v.playCount),
              }),
            );
          }
        }
        this.userPlayLists = { isLoading: false, has: true, own, like };
        return true;
      } catch (err) {
        if (this.userLogin) {
          console.error(getLanguageData("getDataError"), err);
          $message.error(getLanguageData("getDataError"));
        }
        return false;
      } finally {
        this.userPlayLists.isLoading = false;
      }
    },
    /** @internal 由 setUserArtistLists 调度，勿直接调用 */
    async loadUserArtistLists(): Promise<boolean> {
      this.userArtistLists.isLoading = true;
      try {
        const res = await getUserArtistlist();
        if (!res?.data) {
          $message.info(getLanguageData("getDaraEmpty"));
          return false;
        }
        const source: any[] = res.data;
        const list: ArtistItem[] = [];
        for (let i = 0; i < source.length; i++) {
          const v = source[i];
          list.push(asRawEntry({ id: v.id, name: v.name, cover: v.img1v1Url, size: v.musicSize }));
        }
        this.userArtistLists = { isLoading: false, has: true, list };
        return true;
      } catch (err) {
        if (this.userLogin) {
          console.error(getLanguageData("getDataError"), err);
          $message.error(getLanguageData("getDataError"));
        }
        return false;
      } finally {
        this.userArtistLists.isLoading = false;
      }
    },
    /** @internal 由 setUserAlbumLists 调度，勿直接调用 */
    async loadUserAlbumLists(): Promise<boolean> {
      this.userAlbum.isLoading = true;
      try {
        // 首页返回 count，据此并发拉取剩余分页，替代原来的串行 while 循环
        const first = await getUserAlbum(ALBUM_PAGE_SIZE, 0);
        const total = Number(first?.count) || 0;
        const restPages = Math.max(0, Math.ceil(total / ALBUM_PAGE_SIZE) - 1);
        const rest = restPages
          ? await runWithLimit(restPages, ALBUM_PAGE_CONCURRENCY, (index) =>
              getUserAlbum(ALBUM_PAGE_SIZE, (index + 1) * ALBUM_PAGE_SIZE),
            )
          : [];
        const list: AlbumItem[] = [];
        for (let page = -1; page < rest.length; page++) {
          const source: any[] = (page < 0 ? first : rest[page])?.data;
          if (!source?.length) continue;
          for (let i = 0; i < source.length; i++) {
            const v = source[i];
            list.push(
              asRawEntry({
                id: v.id,
                cover: v.picUrl,
                name: v.name,
                artist: v.artists,
                time: getLongTime(v.subTime),
              }),
            );
          }
        }
        this.userAlbum = { isLoading: false, has: true, list };
        return true;
      } catch (err) {
        if (this.userLogin) {
          console.error(getLanguageData("getDataError"), err);
          $message.error(getLanguageData("getDataError"));
        }
        return false;
      } finally {
        this.userAlbum.isLoading = false;
      }
    },
  },
  persist: [
    {
      storage: localStorage,
      pick: ["userLogin", "cookie", "userData", "userOtherData"],
    },
  ],
  // Tauri 层：cookie / 登录态落到真实文件，不再只躺在 WebView 的 localStorage
  // 里；同时让设置窗、迷你播放器等从窗口共享同一份登录态。
  // 拉取到的歌单 / 专辑列表是可重新请求的缓存，不进 Tauri store。
  tauri: {
    save: true,
    sync: true,
    filterKeys: ["userLogin", "cookie", "userData", "userOtherData"],
    filterKeysStrategy: "pick",
  },
});

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useUserDataStore, import.meta.hot));
}

export default useUserDataStore;
