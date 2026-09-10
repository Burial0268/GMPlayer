import type { HistoryState, RouterHistory } from "vue-router";
import { HISTORY_KEY, MAX_LAYERS, type AppLayer, type LayerHistory } from "./layers";

const roots = new Set(["home", "discover", "library", "settings"]);
const kinds = new Set(["page", "search", "player", "queue"]);
const presentations = new Set(["page", "search", "player", "queue-player", "queue-floating"]);

export function readLayerHistory(state: HistoryState): LayerHistory | undefined {
  const value = state[HISTORY_KEY] as unknown as LayerHistory | undefined;
  if (
    !value ||
    value.version !== 1 ||
    typeof value.entryId !== "string" ||
    !Array.isArray(value.layers) ||
    !value.layers.length ||
    value.layers.length > MAX_LAYERS
  )
    return;
  const ids = new Set<string>();
  for (const layer of value.layers) {
    if (
      !layer ||
      typeof layer.id !== "string" ||
      ids.has(layer.id) ||
      !roots.has(layer.root) ||
      !kinds.has(layer.kind) ||
      !presentations.has(layer.presentation) ||
      typeof layer.route !== "string" ||
      !layer.route.startsWith("/") ||
      typeof layer.pageKey !== "string" ||
      typeof layer.label !== "string" ||
      typeof layer.isRoot !== "boolean" ||
      (layer.parentId !== undefined && !ids.has(layer.parentId)) ||
      (layer.search &&
        (typeof layer.search.query !== "string" ||
          !["input", "results"].includes(layer.search.phase)))
    )
      return;
    ids.add(layer.id);
  }
  return value;
}

export function historyData(value: LayerHistory): HistoryState {
  return { [HISTORY_KEY]: value as unknown as HistoryState };
}

export function writeLayerHistory(history: RouterHistory, value: LayerHistory) {
  // Router owns its fields; only replace our own namespace through its public adapter.
  history.replace(history.location, { ...history.state, ...historyData(value) });
}

export function boundLayers(layers: AppLayer[]): AppLayer[] {
  if (layers.length <= MAX_LAYERS) return layers;
  const kept = layers.slice(-MAX_LAYERS);
  kept[0] = { ...kept[0], parentId: undefined, source: undefined };
  return kept;
}
