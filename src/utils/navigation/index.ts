import type { Router } from "vue-router";
import { createLayerNavigation, type LayerNavigation } from "./controller";

let navigation: LayerNavigation;

export function installLayerNavigation(router: Router): LayerNavigation {
  navigation = createLayerNavigation(router);
  return navigation;
}

export function useLayerNavigation(): LayerNavigation {
  return navigation;
}

export { pageCacheKey } from "./layers";
