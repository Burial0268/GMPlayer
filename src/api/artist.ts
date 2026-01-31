/**
 * Artist API - 歌手相关接口
 */

import request from "@/utils/request";
import type { ArtistType, ArtistArea } from "./types";

export const artist = {
  /**
   * 歌手分类列表
   */
  getList: (
    type: ArtistType = -1,
    area: ArtistArea = -1,
    limit = 30,
    offset = 0,
    initial = -1
  ) =>
    request<any>({
      method: "GET",
      url: "/artist/list",
      params: { type, area, limit, offset, initial },
    }),

  /**
   * 获取歌手详情
   */
  getDetail: (id: number) =>
    request<any>({
      method: "GET",
      url: "/artist/detail",
      params: { id },
    }),

  /**
   * 获取歌手部分信息和热门歌曲
   */
  getSongs: (id: number) =>
    request<any>({
      method: "GET",
      url: "/artists",
      params: { id, timestamp: Date.now() },
    }),

  /**
   * 获取歌手全部歌曲
   */
  getAllSongs: (id: number, limit = 30, offset = 0, order: "hot" | "time" = "hot") =>
    request<any>({
      method: "GET",
      url: "/artist/songs",
      params: { id, limit, offset, order, timestamp: Date.now() },
    }),

  /**
   * 获取歌手专辑
   */
  getAlbums: (id: number, limit = 30, offset = 0) =>
    request<any>({
      method: "GET",
      url: "/artist/album",
      params: { id, limit, offset },
    }),

  /**
   * 获取歌手视频
   */
  getVideos: (id: number, limit = 30, offset = 0) =>
    request<any>({
      method: "GET",
      url: "/artist/mv",
      params: { id, limit, offset },
    }),

  /**
   * 收藏/取消收藏歌手
   * @param t - 1为收藏，其他为取消收藏
   */
  subscribe: (id: number, t: number) =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/artist/sub",
      params: { t, id, timestamp: Date.now() },
    }),

  /**
   * 获取用户收藏的歌手列表
   */
  getSublist: () =>
    request<any>({
      method: "GET",
      url: "/artist/sublist",
      params: { timestamp: Date.now() },
    }),

  /**
   * 获取热门歌手列表
   */
  getTop: (limit = 6) =>
    request<any>({
      method: "GET",
      url: "/top/artists",
      params: { limit },
    }),
};

export default artist;

// Legacy exports
export const getArtistList = artist.getList;
export const getArtistDetail = artist.getDetail;
export const getArtistSongs = artist.getSongs;
export const getArtistAllSongs = artist.getAllSongs;
export const getArtistAblums = artist.getAlbums;
export const getArtistVideos = artist.getVideos;
export const likeArtist = artist.subscribe;
export const getUserArtistlist = artist.getSublist;
export const getTopArtists = artist.getTop;
