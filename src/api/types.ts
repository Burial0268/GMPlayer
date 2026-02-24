/**
 * API Module - Type Definitions
 */

import type { AxiosProgressEvent } from "axios";

/** 通用分页参数 */
export interface PaginationParams {
  limit?: number;
  offset?: number;
}

/** 专辑区域类型 */
export type AlbumArea = "ALL" | "ZH" | "EA" | "KR" | "JP";

/** 歌手类型 */
export type ArtistType = -1 | 1 | 2 | 3; // -1:全部 1:男歌手 2:女歌手 3:乐队

/** 歌手区域 */
export type ArtistArea = -1 | 0 | 7 | 8 | 16 | 96; // -1:全部 7:华语 96:欧美 8:日本 16:韩国 0:其他

export type ArtistSongsSortOrder = "time" | "hot";

/** 搜索类型 */
export type SearchType =
  | 1 // 单曲
  | 10 // 专辑
  | 100 // 歌手
  | 1000 // 歌单
  | 1002 // 用户
  | 1004 // MV
  | 1006 // 歌词
  | 1009 // 电台
  | 1014 // 视频
  | 1018 // 综合
  | 2000; // 声音

/** 音质等级 */
export type MusicLevel = "low" | "medium" | "high" | "exhigh";

/** 评论资源类型 */
export type CommentResourceType = "music" | "mv" | "playlist" | "album" | "dj" | "video";

/** 推荐类型 */
export type PersonalizedType = "mv" | "newsong" | "djprogram" | "privatecontent" | null;

/** 上传进度回调 */
export type UploadProgressCallback = (e: AxiosProgressEvent) => void;
