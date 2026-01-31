/**
 * Comment API - 评论相关接口
 */

import request from "@/utils/request";
import type { CommentResourceType } from "./types";

export const comment = {
  /**
   * 获取评论
   */
  get: (id: number, offset = 0, before: number | null = null, type: CommentResourceType = "music") =>
    request<any>({
      method: "GET",
      url: `/comment/${type}`,
      params: { id, offset, before, timestamp: Date.now() },
    }),

  /**
   * 评论点赞
   * @param t - 0为取消点赞，1为点赞
   * @param type - 资源类型，0为歌曲
   */
  like: (id: number, cid: number, t: 0 | 1, type = 0) =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/comment/like",
      params: { id, cid, t, type, timestamp: Date.now() },
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
