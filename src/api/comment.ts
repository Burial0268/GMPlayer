/**
 * Comment API - 评论相关接口
 */

import request from "@/utils/ncm/request";
import type { CommentResourceType } from "./types";

// /comment/like 的 type 是数字，与 /comment/{type} 的路径段一一对应
const LIKE_TYPE: Record<CommentResourceType, number> = {
  music: 0,
  mv: 1,
  playlist: 2,
  album: 3,
  dj: 4,
  video: 5,
};

// 同一首歌的评论会被歌曲页、评论页、播放器评论面板先后请求，这里共用一份。
// 共用的是同一批对象，所以点赞（就地改 liked/likedCount）在各入口之间也是一致的。
// shortcut: 不按账号区分，切换账号后最多 COMMENT_TTL 内仍显示上个账号的点赞状态。
const COMMENT_TTL = 2 * 60 * 1000;
const COMMENT_CACHE_MAX = 40;
const commentCache = new Map<string, { at: number; value: Promise<any> }>();

const fetchComment = (
  id: number,
  offset: number,
  before: number | null,
  type: CommentResourceType,
) =>
  request<any>({
    method: "GET",
    url: `/comment/${type}`,
    params: { id, offset, before, timestamp: Date.now() },
  });

export const comment = {
  /**
   * 获取评论
   * @param fresh - 跳过缓存（用户主动刷新）
   */
  get: (
    id: number,
    offset = 0,
    before: number | null = null,
    type: CommentResourceType = "music",
    fresh = false,
  ): Promise<any> => {
    const key = `${type}:${id}:${offset}:${before ?? ""}`;
    const hit = commentCache.get(key);
    if (!fresh && hit && Date.now() - hit.at < COMMENT_TTL) return hit.value;

    const value = fetchComment(id, offset, before, type);
    commentCache.delete(key);
    commentCache.set(key, { at: Date.now(), value });
    if (commentCache.size > COMMENT_CACHE_MAX) {
      commentCache.delete(commentCache.keys().next().value!);
    }
    // 失败（含 code 非 200 的应答）不缓存，下次重新请求
    const drop = () => {
      if (commentCache.get(key)?.value === value) commentCache.delete(key);
    };
    value.then((res) => res?.code !== 200 && drop(), drop);
    return value;
  },

  /**
   * 评论点赞
   * @param t - 0为取消点赞，1为点赞
   */
  like: (id: number, cid: number, t: 0 | 1, type: CommentResourceType = "music") =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/comment/like",
      params: { id, cid, t, type: LIKE_TYPE[type], timestamp: Date.now() },
    }),

  /**
   * 发送/删除评论
   * @param t - 0为删除，1为发送，2为回复
   */
  send: (id: number, content: string, t: 0 | 1 | 2, commentId: number | null = null, type = 0) =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/comment",
      params: { id, commentId, content, t, type, timestamp: Date.now() },
    }),
};

export default comment;

// Legacy exports
export const getComment = comment.get;
export const likeComment = comment.like;
export const sendComment = comment.send;
