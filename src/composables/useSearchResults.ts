import { getSearchData } from "@/api/search";
import type { SearchType } from "@/api";
import { useRoutePagination } from "./useRoutePagination";

export function useSearchResults<T>(options: {
  category: string;
  type: SearchType;
  items: string;
  total: string;
  map: (items: any[], offset: number) => T[];
}) {
  return useRoutePagination<T>({
    routeName: `s-${options.category}`,
    load: async ({ route, page, limit, hiddenBar }) => {
      const offset = (page - 1) * limit;
      const response = await getSearchData(
        String(route.query.keywords ?? ""),
        limit,
        offset,
        options.type,
        { hiddenBar },
      );
      if (!response.result) throw new Error("Search response is missing its result");
      return {
        items: options.map(response.result[options.items] ?? [], offset),
        total: response.result[options.total] ?? 0,
      };
    },
  });
}
