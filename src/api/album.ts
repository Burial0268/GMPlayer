/**
 * Album API - 专辑相关接口
 */

import request from "@/utils/request";
import type { AlbumArea } from "./types";

export const album = {
  /**
   * 获取专辑内容
   */
  get: (id: number) =>
    request<any>({
      method: "GET",
      url: "/album",
      params: { id, timestamp: Date.now() },
    }),

  /**
   * 获取全部新碟
   */
  getNew: (area: AlbumArea, limit = 30, offset = 0) =>
    request<any>({
      method: "GET",
      url: "/album/new",
      params: { area, limit, offset, timestamp: Date.now() },
    }),

  /**
   * 获取最新专辑列表
   */
  getNewest: () =>
    request<any>({
      method: "GET",
      url: "/album/newest",
    }),

  /**
   * 收藏/取消收藏专辑
   * @param t - 1为收藏，2为取消收藏
   */
  subscribe: (id: number, t: 1 | 2) =>
    request<any>({
      method: "GET",
      url: "/album/sub",
      params: { t, id, timestamp: Date.now() },
    }),

  /**
   * 获取用户收藏的专辑列表
   */
  getSublist: (limit = 30, offset = 0) =>
    request<any>({
      method: "GET",
      url: "/album/sublist",
      params: { limit, offset, timestamp: Date.now() },
    }),
};

export default album;

// Legacy exports
export const getAlbum = album.get;
export const getAlbumNew = album.getNew;
export const getNewAlbum = album.getNewest;
export const likeAlbum = album.subscribe;
export const getUserAlbum = album.getSublist;

// Re-exports from other modules for backward compatibility
export { getToplist } from "./home";
