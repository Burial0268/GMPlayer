/**
 * Home API - 首页推荐相关接口
 */

import request from "@/utils/request";
import type { PersonalizedType } from "./types";

export const home = {
  /**
   * 获取轮播图列表
   */
  getBanner: () =>
    request<any>({
      method: "GET",
      url: "/banner",
    }),

  /**
   * 获取每日推荐歌曲
   */
  getDailySongs: () =>
    request<any>({
      method: "GET",
      url: "/recommend/songs",
      params: { timestamp: Date.now() },
    }),

  /**
   * 获取历史日推可用日期列表
   */
  getDailySongsHistory: () =>
    request<any>({
      method: "GET",
      url: "/history/recommend/songs",
      params: { timestamp: Date.now() },
    }),

  /**
   * 获取历史日推详情数据
   */
  getDailySongsHistoryDetail: (date: string) =>
    request<any>({
      method: "GET",
      url: "/history/recommend/songs/detail",
      params: { date, timestamp: Date.now() },
    }),

  /**
   * 获取私人FM数据
   */
  getPersonalFm: () =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/personal_fm",
      params: { timestamp: Date.now() },
    }),

  /**
   * 将歌曲加入FM垃圾桶
   */
  setFmTrash: (id: number) =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/fm_trash",
      params: { id, timestamp: Date.now() },
    }),

  /**
   * 获取推荐内容列表
   */
  getPersonalized: (type: PersonalizedType = null, limit = 10) =>
    request<any>({
      method: "GET",
      url: `/personalized/${type}`,
      params: { limit },
    }),

  /**
   * 获取排行榜数据
   */
  getToplist: (detail = true) =>
    request<any>({
      method: "GET",
      url: `/toplist${detail ? "/detail" : ""}`,
    }),
};

export default home;

// Legacy exports
export const getBanner = home.getBanner;
export const getDailySongs = home.getDailySongs;
export const getDailySongsHistory = home.getDailySongsHistory;
export const getDailySongsHistoryDetail = home.getDailySongsHistoryDetail;
export const getPersonalFm = home.getPersonalFm;
export const setFmTrash = home.setFmTrash;
export const getPersonalized = home.getPersonalized;
export const getToplist = home.getToplist;

// Re-exports from other modules for backward compatibility
export { getNewAlbum } from "./album";
