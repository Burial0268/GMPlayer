import { onActivated, onBeforeUnmount, onDeactivated, ref, shallowRef, watch } from "vue";
import { useRoute, type RouteLocationNormalizedLoaded } from "vue-router";
import { useLayerNavigation, pageCacheKey } from "@/utils/navigation";
import { remember } from "@/utils/navigation/layers";

interface PageResult<T> {
  items: T[];
  total: number;
}

export function useRoutePagination<T>(options: {
  routeName: string;
  load: (context: {
    route: RouteLocationNormalizedLoaded;
    page: number;
    limit: number;
    hiddenBar: boolean;
  }) => Promise<PageResult<T>>;
  limit?: number;
}) {
  const route = useRoute();
  const navigation = useLayerNavigation();
  const instanceKey = pageCacheKey(route);
  const items = shallowRef<T[]>([]);
  const loading = ref(true);
  const error = ref(false);
  const totalCount = ref(0);
  const pageNumber = ref(1);
  const limit = ref(options.limit ?? 30);
  const pages = new Map<string, PageResult<T>>();
  const pending = new Map<string, Promise<PageResult<T>>>();
  let active = true;
  let disposed = false;
  let displayedKey = "";
  let wantedKey = "";

  const load = async (fresh = false) => {
    if (!active || route.name !== options.routeName || pageCacheKey(route) !== instanceKey) return;
    const page = Math.max(1, Number(route.query.page) || 1);
    pageNumber.value = page;
    const key = `${route.fullPath}:${limit.value}`;
    wantedKey = key;
    if (!fresh && displayedKey === key) return;
    error.value = false;
    loading.value = true;
    const snapshot = { ...route, query: { ...route.query } };
    try {
      let result = fresh ? undefined : pages.get(key);
      if (!result) {
        let request = pending.get(key);
        if (!request) {
          request = options.load({
            route: snapshot,
            page,
            limit: limit.value,
            hiddenBar: !items.value.length,
          });
          pending.set(key, request);
        }
        result = await request;
        if (!disposed) remember(pages, key, result, 3);
      }
      if (disposed || wantedKey !== key) return;
      items.value = result.items;
      totalCount.value = result.total;
      displayedKey = key;
    } catch (reason) {
      if (!disposed && wantedKey === key) {
        error.value = true;
        console.warn("[navigation] page data failed to load", reason);
      }
    } finally {
      pending.delete(key);
      if (wantedKey === key) loading.value = false;
    }
  };

  watch(
    () => route.fullPath,
    () => {
      void load();
    },
    { immediate: true },
  );
  onActivated(() => {
    active = true;
    void load();
  });
  onDeactivated(() => {
    active = false;
  });
  onBeforeUnmount(() => {
    disposed = true;
    pages.clear();
    pending.clear();
  });

  const pageNumberChange = (page: number) =>
    navigation.replacePage({
      path: route.path,
      query: { ...route.query, page },
    });
  const pageSizeChange = (size: number) => {
    limit.value = size;
    if (pageNumber.value !== 1) void pageNumberChange(1);
    else void load();
  };
  return {
    items,
    loading,
    error,
    totalCount,
    pageNumber,
    pageNumberChange,
    pageSizeChange,
    retry: () => load(true),
  };
}
