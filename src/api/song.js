import axios from "@/utils/request";
import { LyricService } from "@/utils/LyricsProcessor";

/**
 * 歌曲部分
 */

/**
 * 检查指定音乐是否可用
 * @param {number} id - 要检查的音乐ID
 */
export const checkMusicCanUse = (id) => {
  return axios({
    method: "GET",
    hiddenBar: true,
    url: "/check/music",
    params: {
      id,
      timestamp: new Date().getTime(),
    },
  });
};

/**
 * 获取指定音乐的播放链接
 * @param {number} id - 要获取播放链接的音乐ID
 * @param {string} [level='exhigh'] - 可选参数，音质等级。可选值包括'low'、'medium'、'high'、'exhigh'。默认为'exhigh'
 */
export const getMusicUrl = (id, level = "exhigh") => {
  return axios({
    method: "GET",
    hiddenBar: true,
    url: "/song/url/v1",
    params: {
      id,
      level,
      timestamp: new Date().getTime(),
    },
  });
};

/**
 * 网易云解灰
 * @param {number} id - 要替换播放链接的音乐ID
 */
export const getMusicNumUrl = async (id) => {
  const server = "qq,pyncmd";
  const url = `${import.meta.env.VITE_UNM_API}match?id=${id}&server=${server}`;
  const response = await fetch(url, {
    method: "GET",
    headers: {
      "Content-Type": "application/json",
    },
  });
  if (!response.ok) {
    return Promise.reject(new Error());
  }
  return await response.json();
};

/**
 * 获取指定音乐的歌词 (Unified)
 * @param {number} id - 要获取歌词的音乐ID
 * @param {boolean} [useLyricAtlas=false] - 是否使用 Lyric Atlas API。默认为 false。
 * @returns {Promise<object|null>} Returns LyricData object or null
 */
export const getUnifiedLyric = async (id, useLyricAtlas = false) => {
  const lyricService = new LyricService(useLyricAtlas);
  try {
    // Directly return the result from the service, which is in LyricData format
    const lyricData = await lyricService.fetchLyric(id);
    return lyricData;
  } catch (error) {
    console.error(`Failed to fetch unified lyric for id ${id}:`, error);
    // Return null or an error object compatible with parseLyric error handling
    return null; // Or { code: 500, message: 'Failed to fetch' }
  }
};

/**
 * 检查指定音乐的歌词元数据
 * @param {number} id - 要检查的音乐ID
 * @param {boolean} [useLyricAtlas=true] - 是否使用 Lyric Atlas API。默认为 true，因为目前只有LAAPI支持此功能。
 * @returns {Promise<object|null>} 返回歌词元数据对象或null
 */
export const checkLyricMeta = async (id, useLyricAtlas = true) => {
  // 元数据检查需要Lyric Atlas API，所以默认使用
  const lyricService = new LyricService(useLyricAtlas);
  try {
    // 如果服务不支持元数据检查，会返回null
    const metaData = await lyricService.checkLyricMeta(id);
    return metaData;
  } catch (error) {
    console.error(`Failed to check lyric meta for id ${id}:`, error);
    return null;
  }
};

/**
 * 获取指定音乐的详情。
 * @param {string} ids - 要获取详情的音乐ID，多个ID用逗号分隔
 */
export const getMusicDetail = (ids) => {
  return axios({
    method: "GET",
    hiddenBar: true,
    url: "/song/detail",
    params: {
      ids,
      timestamp: new Date().getTime(),
    },
  });
};

/**
 * 获取包含指定音乐的相似歌单
 * @param {number} id - 要查询的音乐ID
 */
export const getSimiPlayList = (id) => {
  return axios({
    method: "GET",
    url: "/simi/playlist",
    params: {
      id,
    },
  });
};

/**
 * 获取与指定音乐相似的音乐列表
 * @param {number} id - 要查询的音乐ID
 */
export const getSimiSong = (id) => {
  return axios({
    method: "GET",
    url: "/simi/song",
    params: {
      id,
    },
  });
};

/**
 * 获取客户端歌曲下载
 * @param {number} id - 要下载的音乐ID
 * @param {number} br - 码率, 默认设置了 999000 即最大码率, 如果要 320k 则可设置为 320000, 其他类推
 */
export const getSongDownload = (id, br = 999000) => {
  return axios({
    method: "GET",
    hiddenBar: true,
    url: "/song/download/url",
    params: {
      id,
      br,
      timestamp: new Date().getTime(),
    },
  });
};

/**
 * 听歌打卡
 * @param {number} id - 音乐ID
 * @param {number} sourceid - 来源ID
 */
export const songScrobble = (id, sourceid = 0, time = 0) => {
  return axios({
    method: "GET",
    url: "/scrobble",
    hiddenBar: true,
    params: {
      id,
      sourceid,
      time,
      timestamp: new Date().getTime(),
    },
  });
};
