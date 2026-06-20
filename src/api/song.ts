/**
 * Song API - 歌曲相关接口
 */

import request from "@/utils/request";
import { LyricService } from "@/utils/LyricsProcessor";
import type { MusicLevel } from "./types";

export const song = {
  /**
   * 检查音乐是否可用
   */
  checkAvailable: (id: number) =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/check/music",
      params: { id, timestamp: Date.now() },
    }),

  /**
   * 获取音乐播放链接
   */
  getUrl: (id: number, level: MusicLevel = "exhigh") =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/song/url/v1",
      params: { id, level, timestamp: Date.now() },
    }),

  /**
   * 网易云解灰（通过 UNM API）
   */
  getUnmUrl: async (id: number) => {
    const server = "qq,pyncmd";
    const url = `${import.meta.env.VITE_UNM_API}match?id=${id}&server=${server}`;
    const response = await fetch(url, {
      method: "GET",
      headers: { "Content-Type": "application/json" },
    });
    if (!response.ok) throw new Error("UNM request failed");
    return response.json();
  },

  /**
   * 获取歌词（统一接口）
   */
  getLyric: async (id: number, useTTMLRepo = false) => {
    const lyricService = new LyricService(useTTMLRepo);
    try {
      return await lyricService.fetchLyric(id);
    } catch (error) {
      console.error(`Failed to fetch lyric for id ${id}:`, error);
      return null;
    }
  },

  /**
   * 检查歌词元数据
   */
  checkLyricMeta: async (id: number, useTTMLRepo = true) => {
    const lyricService = new LyricService(useTTMLRepo);
    try {
      return await lyricService.checkLyricMeta(id);
    } catch (error) {
      console.error(`Failed to check lyric meta for id ${id}:`, error);
      return null;
    }
  },

  /**
   * 获取音乐详情
   */
  getDetail: (ids: string | number | number[]) => {
    const idsStr = Array.isArray(ids) ? ids.join(",") : String(ids);
    return request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/song/detail",
      params: { ids: idsStr, timestamp: Date.now() },
    });
  },

  /**
   * 获取相似歌单
   */
  getSimiPlaylist: (id: number) =>
    request<any>({
      method: "GET",
      url: "/simi/playlist",
      params: { id },
    }),

  /**
   * 获取相似歌曲
   */
  getSimiSong: (id: number) =>
    request<any>({
      method: "GET",
      url: "/simi/song",
      params: { id },
    }),

  /**
   * 获取歌曲下载链接
   */
  getDownloadUrl: (id: number, br = 999000) =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/song/download/url",
      params: { id, br, timestamp: Date.now() },
    }),

  /**
   * 听歌打卡
   */
  scrobble: (id: number, sourceid = 0, time = 0) =>
    request<any>({
      method: "GET",
      url: "/scrobble",
      hiddenBar: true,
      params: { id, sourceid, time, timestamp: Date.now() },
    }),

  /**
   * 听歌打卡 V2 (NCBL 加密版)
   * 使用桌面客户端 NCBL 加密日志格式上报听歌记录
   * @param id 歌曲 id
   * @param time 播放时长(秒)
   * @param options sourceid 来源列表 id / source 来源名称(默认 list) / name 歌曲名 /
   *   artist 艺术家 / bitrate 码率(默认 320) / level 音质等级(默认 exhigh) / total 歌曲总时长(秒)
   */
  scrobbleV2: (
    id: number,
    time: number,
    options: {
      sourceid?: number;
      source?: string;
      name?: string;
      artist?: string;
      bitrate?: number;
      level?: string;
      total?: number;
    } = {},
  ) =>
    request<any>({
      method: "GET",
      url: "/scrobble/v1",
      hiddenBar: true,
      params: { id, time, timestamp: Date.now(), ...options },
    }),

  /**
   * 提交歌曲播放状态
   * 使用桌面客户端 Cookie/UA 形态提交当前播放进度
   */
  submitPlayState: (
    id: number,
    options: {
      sessionId?: string;
      progress?: number;
      playMode?: string;
      type?: string;
      cookie?: string;
      ua?: string;
    } = {},
  ) =>
    request<any>({
      method: "GET",
      url: "/relay/play/state/submit",
      hiddenBar: true,
      params: { id, timestamp: Date.now(), ...options },
    }),

  /**
   * 喜欢/取消喜欢歌曲
   */
  like: (id: number, like = true) =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/like",
      params: { id, like, timestamp: Date.now() },
    }),
};

export default song;

// Legacy exports
export const checkMusicCanUse = song.checkAvailable;
export const getMusicUrl = song.getUrl;
export const getMusicNumUrl = song.getUnmUrl;
export const getUnifiedLyric = song.getLyric;
export const checkLyricMeta = song.checkLyricMeta;
export const getMusicDetail = song.getDetail;
export const getSimiPlayList = song.getSimiPlaylist;
export const getSimiSong = song.getSimiSong;
export const getSongDownload = song.getDownloadUrl;
export const songScrobble = song.scrobble;
export const songScrobbleV2 = song.scrobbleV2;
export const submitSongPlayState = song.submitPlayState;
export const setLikeSong = song.like;
