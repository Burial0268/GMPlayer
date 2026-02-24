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
  playCount: string;
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
  },
  actions: {
    setCookie(value: string) {
      window.localStorage.setItem("cookie", value);
      this.cookie = value;
    },
    setUserData(value: UserData) {
      this.userData = value;
    },
    setUserOtherData() {
      if (this.userLogin) {
        const getOtherData = [getUserLevel(), getUserSubcount()];
        Promise.all(getOtherData)
          .then((res) => {
            console.log(res);
            this.userOtherData.level = res[0].data;
            this.userOtherData.subcount = res[1];
          })
          .catch((err) => {
            console.error(getLanguageData("getDataError"), err);
            $message.error(getLanguageData("getDataError"));
          });
      }
    },
    userLogOut() {
      this.userLogin = false;
      this.cookie = null;
      this.userData = {};
      this.userOtherData = {};
      localStorage.removeItem("cookie");
      apiUserLogOut();
    },
    async setUserPlayLists(callback?: () => void) {
      if (this.userLogin) {
        try {
          this.userPlayLists.isLoading = true;
          const { userId } = this.userData;
          const { createdPlaylistCount, subPlaylistCount } = await getUserSubcount();
          const number = createdPlaylistCount + subPlaylistCount || 30;
          const res = await getUserPlaylist(userId!, number);
          if (res.playlist) {
            this.userPlayLists = {
              isLoading: false,
              has: true,
              own: [],
              like: [],
            };
            res.playlist.forEach((v: any) => {
              if (v.creator.userId === this.getUserData.userId) {
                this.userPlayLists.own.push({
                  id: v.id,
                  cover: v.coverImgUrl,
                  name: v.name,
                  artist: v.creator,
                  desc: v.description,
                  tags: v.tags,
                  playCount: formatNumber(v.playCount),
                  trackCount: v.trackCount,
                });
              } else {
                this.userPlayLists.like.push({
                  id: v.id,
                  cover: v.coverImgUrl,
                  name: v.name,
                  artist: v.creator,
                  playCount: formatNumber(v.playCount),
                });
              }
            });
            if (callback && typeof callback === "function") {
              callback();
            }
            this.userPlayLists.isLoading = false;
          } else {
            this.userPlayLists.isLoading = false;
            $message.info(getLanguageData("getDaraEmpty"));
          }
        } catch (err) {
          this.userPlayLists.isLoading = false;
          if (this.userLogin) {
            console.error(getLanguageData("getDataError"), err);
            $message.error(getLanguageData("getDataError"));
          }
        }
      } else {
        $message.error(getLanguageData("needLogin"));
      }
    },
    async setUserArtistLists(callback?: () => void) {
      if (this.userLogin) {
        try {
          this.userArtistLists.isLoading = true;
          const res = await getUserArtistlist();
          if (res.data) {
            this.userArtistLists.list = [];
            this.userArtistLists.has = true;
            res.data.forEach((v: any) => {
              this.userArtistLists.list.push({
                id: v.id,
                name: v.name,
                cover: v.img1v1Url,
                size: v.musicSize,
              });
            });
            if (callback && typeof callback === "function") {
              callback();
            }
            this.userArtistLists.isLoading = false;
          } else {
            this.userArtistLists.isLoading = false;
            $message.info(getLanguageData("getDaraEmpty"));
          }
        } catch (err) {
          this.userArtistLists.isLoading = false;
          if (this.userLogin) {
            console.error(getLanguageData("getDataError"), err);
            $message.error(getLanguageData("getDataError"));
          }
        }
      } else {
        $message.error(getLanguageData("needLogin"));
      }
    },
    async setUserAlbumLists(callback?: () => void) {
      if (this.userLogin) {
        try {
          let offset = 0;
          let totalCount: number | null = null;
          this.userAlbum.isLoading = true;
          this.userAlbum.list = [];
          while (totalCount === null || offset < totalCount) {
            const res = await getUserAlbum(30, offset);
            res.data.forEach((v: any) => {
              this.userAlbum.list.push({
                id: v.id,
                cover: v.picUrl,
                name: v.name,
                artist: v.artists,
                time: getLongTime(v.subTime),
              });
            });
            totalCount = res.count;
            offset += 30;
            console.log(totalCount, offset, this.userAlbum.list);
          }
          if (callback && typeof callback === "function") {
            callback();
          }
          this.userAlbum.isLoading = false;
          this.userAlbum.has = true;
        } catch (err) {
          this.userAlbum.isLoading = false;
          if (this.userLogin) {
            console.error(getLanguageData("getDataError"), err);
            $message.error(getLanguageData("getDataError"));
          }
        }
      } else {
        $message.error(getLanguageData("needLogin"));
      }
    },
  },
  persist: [
    {
      storage: localStorage,
      pick: ["userLogin", "cookie", "userData", "userOtherData"],
    },
  ],
});

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useUserDataStore, import.meta.hot));
}

export default useUserDataStore;
