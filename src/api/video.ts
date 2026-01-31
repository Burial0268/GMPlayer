/**
 * Video API - 视频相关接口
 */

import request from "@/utils/request";

export const video = {
  /**
   * 获取MV详情
   */
  getDetail: (mvid: number) =>
    request<any>({
      method: "GET",
      url: "/mv/detail",
      params: { mvid },
    }),

  /**
   * 获取MV播放地址
   */
  getUrl: (id: number, r: string | null = null) =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/mv/url",
      params: { id, r },
    }),

  /**
   * 获取相似MV列表
   */
  getSimi: (mvid: number) =>
    request<any>({
      method: "GET",
      url: "/simi/mv",
      params: { mvid },
    }),
};

export default video;

// Legacy exports
export const getVideoDetail = video.getDetail;
export const getVideoUrl = video.getUrl;
export const getSimiVideo = video.getSimi;
