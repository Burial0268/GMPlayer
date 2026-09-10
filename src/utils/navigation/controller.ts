import { computed, readonly, ref, shallowRef } from "vue";
import type { RouteLocationNormalized, RouteLocationRaw, Router } from "vue-router";
import {
  layerDirection,
  nextLayerId,
  pageCacheKey,
  pageLayer,
  topLayer,
  visibleOverlays,
  type AppLayer,
  type LayerHistory,
  type LayerKind,
  type LayerSource,
  type LayerTransition,
  type RootEntry,
  type SourceKind,
} from "./layers";
import { boundLayers, historyData, readLayerHistory, writeLayerHistory } from "./history";
import { captureSource } from "./sources";

export interface OpenPageOptions {
  origin?: Event | HTMLElement;
  kind?: SourceKind;
  identity?: string;
}

export function createLayerNavigation(router: Router) {
  const state = shallowRef<LayerHistory>({ version: 1, entryId: nextLayerId(), layers: [] });
  const transition = shallowRef<LayerTransition>({
    id: 0,
    direction: "none",
    from: [],
    to: [],
    routeChanged: false,
  });
  const transitioning = ref(false);
  const dockedQueue = ref(false);
  const beforeListeners = new Set<() => void>();
  const cancelListeners = new Set<() => void>();
  const roots = new Map<RootEntry, AppLayer>();
  let backPending = false;
  let pop: { state?: LayerHistory; direction: "back" | "forward" | "" } | undefined;
  const transactions = new WeakMap<
    RouteLocationNormalized,
    { from: LayerHistory; pop: typeof pop }
  >();

  const current = computed(() => topLayer(state.value.layers));
  const overlays = computed(() => visibleOverlays(state.value.layers));
  const presentedOverlays = computed(() => {
    if (transitioning.value && !overlays.value.length)
      return visibleOverlays(transition.value.from);
    return overlays.value;
  });
  const playerVisible = computed(() =>
    presentedOverlays.value.some((layer) => layer.kind === "player"),
  );
  const queueVisible = computed(
    () =>
      dockedQueue.value ||
      presentedOverlays.value.some((layer) => layer.presentation === "queue-floating"),
  );
  const playerQueueVisible = computed(() =>
    overlays.value.some((layer) => layer.presentation === "queue-player"),
  );
  const searchVisible = computed(() => current.value?.presentation === "search");
  const searchLayer = computed(() =>
    [...state.value.layers].reverse().find((layer) => layer.kind === "search"),
  );
  const canGoBack = computed(
    () => state.value.layers.length > 1 || Boolean(current.value && !current.value.isRoot),
  );

  const stopHistory = router.options.history.listen((_to, _from, info) => {
    pop = { state: readLayerHistory(router.options.history.state), direction: info.direction };
  });

  const stopBefore = router.beforeEach((to) => {
    transactions.set(to, { from: state.value, pop });
    beforeListeners.forEach((listener) => listener());
  });

  function inferLayers(to: RouteLocationNormalized, from: AppLayer[]): AppLayer[] {
    const previous = topLayer(from);
    const next = pageLayer(to, previous);
    const sameSearch = previous?.kind === "search" && next.kind === "search";
    if (previous && (previous.pageKey === next.pageKey || sameSearch)) {
      return [
        ...from.slice(0, -1),
        {
          ...next,
          id: previous.id,
          parentId: previous.parentId,
          source: previous.source,
          root: previous.root,
          isRoot: previous.isRoot,
        },
      ];
    }
    return next.isRoot ? [next] : [...from, next];
  }

  const stopAfter = router.afterEach((to, from, failure) => {
    const transaction = transactions.get(to);
    if (failure) {
      if (transaction?.pop === pop) pop = undefined;
      backPending = false;
      cancelListeners.forEach((listener) => listener());
      return;
    }
    if (to.meta.standalone) return;
    const previous = transaction?.from ?? state.value;
    const requested = readLayerHistory(router.options.history.state) ?? transaction?.pop?.state;
    const requestedTop = requested && topLayer(requested.layers);
    const matches =
      requestedTop &&
      (requestedTop.route === to.fullPath || requestedTop.pageKey === pageCacheKey(to));
    const adopts =
      matches &&
      (requested.entryId !== previous.entryId || transaction?.pop || !previous.layers.length);
    const layers = boundLayers(
      adopts
        ? requested.layers.map((layer, index) =>
            index === requested.layers.length - 1 ? { ...layer, route: to.fullPath } : layer,
          )
        : inferLayers(to, previous.layers),
    );
    const entryId = adopts ? requested.entryId : nextLayerId();
    const direction = layerDirection(previous.layers, layers, transaction?.pop?.direction);
    const next = topLayer(layers)!;
    const routeChanged =
      from.fullPath !== to.fullPath &&
      (!previous.layers.length || pageCacheKey(from) !== pageCacheKey(to));
    transition.value = {
      id: transition.value.id + 1,
      direction,
      from: previous.layers,
      to: layers,
      source: direction === "pop" ? topLayer(previous.layers)?.source : next.source,
      routeChanged,
    };
    transitioning.value = routeChanged && previous.layers.length > 0;
    state.value = { version: 1, entryId, layers };
    if (next.isRoot) roots.set(next.root, next);
    writeLayerHistory(router.options.history, state.value);
    if (transaction?.pop === pop) pop = undefined;
    backPending = false;
  });

  const stopError = router.onError(() => {
    pop = undefined;
    backPending = false;
    cancelListeners.forEach((listener) => listener());
  });

  function go(to: RouteLocationRaw, layers: AppLayer[], replace = false) {
    const resolved = router.resolve(to);
    const value: LayerHistory = { version: 1, entryId: nextLayerId(), layers: boundLayers(layers) };
    return router[replace ? "replace" : "push"]({
      path: resolved.path,
      query: resolved.query,
      hash: resolved.hash,
      force: true,
      state: historyData(value),
    });
  }

  function sourceFor(options: OpenPageOptions): LayerSource | undefined {
    return current.value
      ? captureSource(options.origin, current.value.id, options.kind, options.identity)
      : undefined;
  }

  function openPage(to: RouteLocationRaw, options: OpenPageOptions = {}) {
    const parent = current.value;
    const next = pageLayer(router.resolve(to), parent);
    if (parent) {
      next.isRoot = false;
      next.root = parent.root;
      next.parentId = parent.id;
    }
    next.source = sourceFor(options);
    return go(to, [...state.value.layers, next]);
  }

  function replacePage(to: RouteLocationRaw) {
    const previous = current.value;
    if (!previous) return router.replace(to);
    const next = pageLayer(router.resolve(to));
    return go(
      to,
      [
        ...state.value.layers.slice(0, -1),
        {
          ...next,
          id: previous.id,
          root: previous.root,
          isRoot: previous.isRoot,
          parentId: previous.parentId,
          source: previous.source,
        },
      ],
      true,
    );
  }

  function switchRoot(root: RootEntry, fallback: RouteLocationRaw) {
    const saved = roots.get(root);
    const fallbackRoute = router.resolve(fallback);
    // The library destination can change after login/logout.
    const to =
      saved && pageCacheKey(router.resolve(saved.route)) === pageCacheKey(fallbackRoute)
        ? saved.route
        : fallback;
    if (
      current.value?.isRoot &&
      current.value.root === root &&
      current.value.route === router.resolve(to).fullPath
    )
      return;
    const next = saved && saved.route === to ? saved : pageLayer(router.resolve(to));
    return go(to, [{ ...next, root, isRoot: true, parentId: undefined, source: undefined }]);
  }

  function openTemporary(kind: "search" | "player" | "queue", options: OpenPageOptions = {}) {
    const previous = current.value;
    if (!previous || (previous.kind === kind && previous.presentation !== "page")) return;
    if (kind === "search" && previous.kind === "search") {
      return go(
        previous.route,
        [
          ...state.value.layers.slice(0, -1),
          {
            ...previous,
            presentation: "search",
            search: { query: previous.search?.query ?? "", phase: "input" },
          },
        ],
        true,
      );
    }
    const next: AppLayer = {
      id: nextLayerId(),
      parentId: previous.id,
      kind,
      root: previous.root,
      isRoot: false,
      route: router.currentRoute.value.fullPath,
      pageKey: pageCacheKey(router.currentRoute.value),
      label:
        kind === "search"
          ? "navigation.search"
          : kind === "player"
            ? "navigation.player"
            : "general.name.playlists",
      presentation:
        kind === "queue" ? (previous.kind === "player" ? "queue-player" : "queue-floating") : kind,
      source: sourceFor(options),
      ...(kind === "search" ? { search: { query: "", phase: "input" as const } } : {}),
    };
    return go(next.route, [...state.value.layers, next]);
  }

  function closeTop(expected?: LayerKind): boolean {
    if (backPending) return true;
    const top = current.value;
    if (!top || (expected && expected !== top.kind)) return false;
    if (state.value.layers.length > 1) {
      backPending = true;
      router.back();
      return true;
    }
    if (!top.isRoot) {
      backPending = true;
      const target =
        top.root === "library" ? "/local" : top.root === "discover" ? "/discover" : "/";
      const next = pageLayer(router.resolve(target));
      void go(target, [next], true);
      return true;
    }
    return false;
  }

  function updateSearchQuery(query: string) {
    const top = current.value;
    if (top?.kind !== "search" || top.search?.query === query) return;
    const layers = [
      ...state.value.layers.slice(0, -1),
      {
        ...top,
        search: { query, phase: top.search?.phase ?? ("input" as const) },
      },
    ];
    state.value = { ...state.value, layers };
    writeLayerHistory(router.options.history, state.value);
  }

  function openQueue(options: OpenPageOptions = {}) {
    if (
      typeof window !== "undefined" &&
      window.matchMedia("(min-width: 1041px)").matches &&
      current.value?.kind !== "player"
    ) {
      dockedQueue.value = true;
      return;
    }
    return openTemporary("queue", options);
  }

  function closeQueue() {
    if (current.value?.kind === "queue") return closeTop("queue");
    dockedQueue.value = false;
    return true;
  }

  return {
    state: readonly(state),
    current,
    transition: readonly(transition),
    isTransitioning: readonly(transitioning),
    overlays,
    presentedOverlays,
    playerVisible,
    queueVisible,
    playerQueueVisible,
    searchVisible,
    searchLayer,
    canGoBack,
    openPage,
    replacePage,
    switchRoot,
    openQueue,
    closeQueue,
    closeTop,
    updateSearchQuery,
    syncQueueLayout: (inline: boolean) => {
      if (inline && current.value?.presentation === "queue-floating") {
        dockedQueue.value = true;
        closeTop("queue");
      } else if (!inline && dockedQueue.value) {
        dockedQueue.value = false;
        void openTemporary("queue");
      }
    },
    openSearch: (origin?: Event | HTMLElement) => openTemporary("search", { origin }),
    openPlayer: (origin?: Event | HTMLElement) => openTemporary("player", { origin }),
    hasLayer: (kind: LayerKind) => state.value.layers.some((layer) => layer.kind === kind),
    finishTransition: (id: number) => {
      if (transition.value.id === id) transitioning.value = false;
    },
    onBeforeNavigation: (listener: () => void) => {
      beforeListeners.add(listener);
      return () => beforeListeners.delete(listener);
    },
    onNavigationCancelled: (listener: () => void) => {
      cancelListeners.add(listener);
      return () => cancelListeners.delete(listener);
    },
    dispose: () => {
      stopHistory();
      stopBefore();
      stopAfter();
      stopError();
      beforeListeners.clear();
      cancelListeners.clear();
    },
  };
}

export type LayerNavigation = ReturnType<typeof createLayerNavigation>;
