/**
 * Search API - 搜索相关接口
 */

import request from "@/utils/request";
import type { SearchType } from "./types";

export const search = {
  /**
   * 获取热门搜索列表
   */
  getHot: () =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/search/hot/detail",
    }),

  /**
   * 搜索建议
   */
  getSuggest: (keywords: string) =>
    request<any>({
      method: "GET",
      hiddenBar: true,
      url: "/search/suggest",
      params: { keywords },
    }),

  /**
   * 搜索结果
   */
  search: (keywords: string, limit = 30, offset = 0, type: SearchType = 1) =>
    request<any>({
      method: "GET",
      url: "/cloudsearch",
      params: { keywords, limit, offset, type },
    }),
};

export default search;

// Legacy exports
export const getSearchHot = search.getHot;
export const getSearchSuggest = search.getSuggest;
export const getSearchData = search.search;
