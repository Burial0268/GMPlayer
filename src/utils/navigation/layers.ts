import type { RouteLocationNormalized } from "vue-router";
import { refFromQuery, refToQuery } from "@/utils/playlistSource";

export type RootEntry = "home" | "discover" | "library" | "settings";
export type LayerKind = "page" | "search" | "player" | "queue";
export type LayerDirection = "push" | "pop" | "root" | "replace" | "none";
export type SourceKind = "card" | "cover" | "control";
export type NavigationRoute = Pick<
  RouteLocationNormalized,
  "path" | "fullPath" | "query" | "matched" | "meta"
>;

export interface LayerSource {
  id: string;
  ownerId: string;
  kind: SourceKind;
  identity?: string;
}

export interface AppLayer {
  id: string;
  parentId?: string;
  kind: LayerKind;
  root: RootEntry;
  isRoot: boolean;
  route: string;
  pageKey: string;
  label: string;
  source?: LayerSource;
  presentation: "page" | "search" | "player" | "queue-player" | "queue-floating";
  search?: { query: string; phase: "input" | "results" };
}

export interface LayerHistory {
  version: 1;
  entryId: string;
  layers: AppLayer[];
}

export interface LayerTransition {
  id: number;
  direction: LayerDirection;
  from: AppLayer[];
  to: AppLayer[];
  source?: LayerSource;
  routeChanged: boolean;
}

export const HISTORY_KEY = "gmplayerLayers";
export const MAX_LAYERS = 32;
export const MAX_VIEW_STATES = 64;

let serial = 0;
const session = Date.now().toString(36);
export const nextLayerId = () => `${session}-${(++serial).toString(36)}`;
export const topLayer = (layers: readonly AppLayer[]) => layers.at(-1);
export const viewStateKey = (layer: AppLayer) => `${layer.id}:${layer.route}`;

// Page data, history entries and a clicked card have deliberately different keys.
export function pageCacheKey(route: NavigationRoute): string {
  const record = route.matched[0];
  const base = record?.path ?? route.path;
  if (base === "/search") return `${base}:${String(route.query.keywords ?? "")}`;
  if (base === "/local/playlist") {
    const query = Object.entries(refToQuery(refFromQuery(route.query))).sort(([a], [b]) =>
      a.localeCompare(b),
    );
    return `${base}:${JSON.stringify(query)}`;
  }
  if (base === "/local/song") return `${base}:${String(route.query.key ?? "")}`;
  const id = route.query.id;
  return `${base}${id === null || id === undefined ? "" : `:${String(id)}`}`;
}

export function pageLayer(route: NavigationRoute, parent?: AppLayer): AppLayer {
  const root = route.meta.navigationRoot;
  const isSearch = route.matched[0]?.name === "search";
  return {
    id: nextLayerId(),
    parentId: root ? undefined : parent?.id,
    kind: isSearch ? "search" : "page",
    root: root ?? parent?.root ?? route.meta.navigationFallback ?? "home",
    isRoot: Boolean(root),
    route: route.fullPath,
    pageKey: pageCacheKey(route),
    label: route.meta.navigationLabel ?? "",
    presentation: "page",
    ...(isSearch
      ? { search: { query: String(route.query.keywords ?? ""), phase: "results" as const } }
      : {}),
  };
}

export function layerDirection(
  from: readonly AppLayer[],
  to: readonly AppLayer[],
  historyDirection?: "back" | "forward" | "",
): LayerDirection {
  const previous = topLayer(from);
  const next = topLayer(to);
  if (!previous || !next) return "none";
  if (previous.id === next.id) return "replace";
  if (previous.root !== next.root || (previous.isRoot && next.isRoot)) return "root";
  if (from.some((layer) => layer.id === next.id)) return "pop";
  if (to.some((layer) => layer.id === previous.id)) return "push";
  return historyDirection === "back" ? "pop" : "push";
}

export function visibleOverlays(layers: readonly AppLayer[]): AppLayer[] {
  let page = -1;
  for (let i = layers.length - 1; i >= 0; i--) {
    if (layers[i].presentation === "page") {
      page = i;
      break;
    }
  }
  const overlays = layers.slice(page + 1);
  const top = topLayer(overlays);
  if (!top) return [];
  if (top.presentation === "queue-player") {
    return overlays.filter((layer) => layer.kind === "player" || layer.id === top.id);
  }
  return [top];
}

export function remember<K, V>(cache: Map<K, V>, key: K, value: V, limit = MAX_VIEW_STATES) {
  cache.delete(key);
  cache.set(key, value);
  while (cache.size > limit) cache.delete(cache.keys().next().value as K);
}
