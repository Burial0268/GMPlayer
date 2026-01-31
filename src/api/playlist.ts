/**
 * Playlist API - 歌单相关接口
 */

import request from "@/utils/request";

export const playlist = {
  /**
   * 获取歌单分类信息
   */
  getCatlist: (highquality = false) =>
    request<any>({
      method: "GET",
      url: `/playlist/${highquality ? "highquality/tags" : "catlist"}`,
    }),

  /**
   * 获取歌单分类列表
   */
  getTop: (cat = "全部", limit = 30, offset = 0) =>
    request<any>({
      method: "GET",
      url: "/top/playlist",
      params: { cat, limit, offset },
    }),

  /**
   * 获取精品歌单列表
   */
  getHighquality: (cat = "全部", limit = 30, before?: number) =>
    request<any>({
      method: "GET",
      url: "/top/playlist/highquality",
      params: { cat, limit, before },
    }),

  /**
   * 获取歌单详情
   */
  getDetail: (id: number) =>
    request<any>({
      method: "GET",
      url: "/playlist/detail",
      withCredentials: false,
      params: { id, timestamp: Date.now() },
    }),

  /**
   * 获取歌单中所有歌曲信息
   */
  getTracks: (id: number, limit = 30, offset = 0) =>
    request<any>({
      method: "GET",
      url: "/playlist/track/all",
      params: { id, limit, offset, timestamp: Date.now() },
    }),

  /**
   * 新建歌单
   */
  create: (name: string, privacy = false) =>
    request<any>({
      method: "GET",
      url: "/playlist/create",
      params: { name, privacy, timestamp: Date.now() },
    }),

  /**
   * 删除歌单
   */
  delete: (id: number) =>
    request<any>({
      method: "GET",
      url: "/playlist/delete",
      params: { id, timestamp: Date.now() },
    }),

  /**
   * 更新歌单信息
   */
  update: (id: number, name: string, desc: string | null = null, tags: string | null = null) =>
    request<any>({
      method: "GET",
      url: "/playlist/update",
      params: { id, name, desc, tags, timestamp: Date.now() },
    }),

  /**
   * 向歌单中添加或删除歌曲
   */
  updateTracks: (pid: number, tracks: number[], op: "add" | "del" = "add") =>
    request<any>({
      method: "GET",
      url: "/playlist/tracks",
      params: { op, pid, tracks, timestamp: Date.now() },
    }),

  /**
   * 收藏/取消收藏歌单
   * @param t - 1为收藏，2为取消收藏
   */
  subscribe: (id: number, t: 1 | 2) =>
    request<any>({
      method: "GET",
      url: "/playlist/subscribe",
      params: { t, id, timestamp: Date.now() },
    }),
};

export default playlist;

// Legacy exports
export const getPlayListCatlist = playlist.getCatlist;
export const getTopPlaylist = playlist.getTop;
export const getHighqualityPlaylist = playlist.getHighquality;
export const getPlayListDetail = playlist.getDetail;
export const getAllPlayList = playlist.getTracks;
export const createPlaylist = playlist.create;
export const delPlayList = playlist.delete;
export const playlistUpdate = playlist.update;
export const addSongToPlayList = playlist.updateTracks;
export const likePlaylist = playlist.subscribe;
