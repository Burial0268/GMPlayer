import type { Router } from "vue-router";
import { createLayerNavigation, type LayerNavigation } from "./controller";
import { userStore } from "@/store";
import { isTauri } from "@/utils/tauri/core/runtime";

let navigation: LayerNavigation;

/** `/user` needs a login; `/local` exists only on Tauri, so the web build always goes to `/user`. */
export const libraryRoute = () => (userStore().userLogin || !isTauri() ? "/user" : "/local");

export function installLayerNavigation(router: Router): LayerNavigation {
  navigation = createLayerNavigation(router, libraryRoute);
  return navigation;
}

export function useLayerNavigation(): LayerNavigation {
  return navigation;
}

export { pageCacheKey } from "./layers";
