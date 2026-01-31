/**
 * User API - 用户相关接口
 */

import request from "@/utils/request";
import type { UploadProgressCallback } from "./types";

export const user = {
  /**
   * 获取用户等级信息
   */
  getLevel: () =>
    request<any>({
      method: "GET",
      url: "/user/level",
      params: { timestamp: Date.now() },
    }),

  /**
   * 获取用户订阅信息
   */
  getSubcount: () =>
    request<any>({
      method: "GET",
      url: "/user/subcount",
      params: { timestamp: Date.now() },
    }),

  /**
   * 获取用户歌单列表
   */
  getPlaylist: (uid: number, limit = 30, offset = 0) =>
    request<any>({
      method: "GET",
      url: "/user/playlist",
      params: { uid, limit, offset, timestamp: Date.now() },
    }),

  /**
   * 获取用户喜欢的音乐列表
   */
  getLikelist: (uid: number) =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/likelist",
      params: { uid, timestamp: Date.now() },
    }),

  /**
   * 获取用户云盘数据
   */
  getCloud: (limit = 30, offset = 0) =>
    request<any>({
      method: "GET",
      url: "/user/cloud",
      params: { limit, offset, timestamp: Date.now() },
    }),

  /**
   * 删除云盘歌曲
   */
  deleteCloudSong: (id: number) =>
    request<any>({
      method: "GET",
      url: "/user/cloud/del",
      params: { id, timestamp: Date.now() },
    }),

  /**
   * 云盘歌曲信息匹配纠正
   */
  matchCloudSong: (uid: number, sid: number, asid: number) =>
    request<any>({
      method: "GET",
      url: "/cloud/match",
      params: { uid, sid, asid, timestamp: Date.now() },
    }),

  /**
   * 上传歌曲到云盘
   */
  uploadCloudSong: (file: File, onUploadProgress?: UploadProgressCallback) => {
    const formData = new FormData();
    formData.append("songFile", file);
    return request<any>({
      url: "/cloud",
      method: "POST",
      hiddenBar: true,
      params: { timestamp: Date.now() },
      data: formData,
      headers: { "Content-Type": "multipart/form-data" },
      timeout: 200000,
      onUploadProgress,
    });
  },

  /**
   * 每日签到
   * @param type - 0为安卓端签到，1为web/PC签到
   */
  dailySignin: (type: 0 | 1 = 0) =>
    request<any>({
      method: "GET",
      url: "/daily_signin",
      params: { type, timestamp: Date.now() },
    }),

  /**
   * 云贝签到
   */
  yunbeiSign: () =>
    request<any>({
      method: "GET",
      url: "/yunbei/sign",
      params: { timestamp: Date.now() },
    }),

  /**
   * 喜欢/取消喜欢歌曲
   */
  likeSong: (id: number, like = true) =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/like",
      params: { id, like, timestamp: Date.now() },
    }),
};

export default user;

// Legacy exports
export const getUserLevel = user.getLevel;
export const getUserSubcount = user.getSubcount;
export const getUserPlaylist = user.getPlaylist;
export const getLikelist = user.getLikelist;
export const setLikeSong = user.likeSong;
export const getCloud = user.getCloud;
export const setCloudDel = user.deleteCloudSong;
export const setCloudMatch = user.matchCloudSong;
export const upCloudSong = user.uploadCloudSong;
export const userDailySignin = user.dailySignin;
export const userYunbeiSign = user.yunbeiSign;

// Re-exports from other modules for backward compatibility
export { getUserAlbum } from "./album";
export { getUserArtistlist } from "./artist";
export { userLogOut } from "./login";
