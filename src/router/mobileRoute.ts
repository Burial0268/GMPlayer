import {
  defineComponent,
  h,
  inject,
  onMounted,
  provide,
  shallowReactive,
  shallowRef,
  watch,
  type Component,
} from "vue";
import {
  routeLocationKey,
  routerViewLocationKey,
  useRouter,
  type RouteLocationNormalizedLoaded,
} from "vue-router";
import PageLoadState from "@/components/Navigation/PageLoadState.vue";
import DetailPageSkeleton from "@/components/Navigation/DetailPageSkeleton.vue";
import { pageCacheKey } from "@/utils/navigation/layers";
import { useLayerNavigation } from "@/utils/navigation";
import { pageSourceKey, sourceSnapshot } from "@/utils/navigation/sources";

type RouteModule = { default: Component };
let surfaceId = 0;

/** Commit a stable mobile surface before downloading the page module. */
export function mobileRoute(loader: () => Promise<RouteModule>, nested = false) {
  let resolved: Component | undefined;
  let pending: Promise<RouteModule> | undefined;
  const load = () =>
    (pending ??= loader()
      .then((module) => {
        resolved = module.default;
        return module;
      })
      .catch((error) => {
        pending = undefined;
        throw error;
      }));

  const Surface = defineComponent({
    name: `RouteSurface${++surfaceId}`,
    inheritAttrs: false,
    setup(_props, { attrs, slots }) {
      const router = useRouter();
      const parentRoute = inject(routerViewLocationKey)!;
      const snapshot = shallowRef(parentRoute.value);
      const key = pageCacheKey(snapshot.value);
      const routeName = snapshot.value.name;
      const component = shallowRef(resolved);
      const failed = shallowRef(false);
      if (!nested)
        provide(pageSourceKey, sourceSnapshot(useLayerNavigation().current.value?.source));
      let rendered = false;
      // A cached parent's RouterView must keep its own children while another page is active.
      watch(
        parentRoute,
        (route) => {
          if (pageCacheKey(route) === key) snapshot.value = route;
        },
        { flush: "sync" },
      );
      provide(routerViewLocationKey, snapshot);
      const localRoute = {} as RouteLocationNormalizedLoaded;
      for (const field of Object.keys(snapshot.value) as (keyof RouteLocationNormalizedLoaded)[]) {
        Object.defineProperty(localRoute, field, {
          enumerable: true,
          get: () => snapshot.value[field],
        });
      }
      provide(routeLocationKey, shallowReactive(localRoute));

      const resolve = async () => {
        failed.value = false;
        try {
          component.value = (await load()).default;
        } catch (error) {
          failed.value = true;
          console.warn("[navigation] page module failed to load", error);
        }
      };
      onMounted(() => {
        if (!component.value) void resolve();
      });

      return () => {
        const mayMount =
          rendered ||
          (pageCacheKey(router.currentRoute.value) === key &&
            (!nested || router.currentRoute.value.name === routeName));
        let content;
        if (component.value && mayMount) {
          rendered = true;
          content = h(component.value, attrs, slots);
        } else if (!failed.value && !nested && snapshot.value.meta.navigationDetail) {
          content = h(DetailPageSkeleton, { kind: snapshot.value.meta.navigationDetail });
        } else {
          content = h(PageLoadState, {
            loading: !failed.value,
            error: failed.value,
            onRetry: resolve,
          });
        }
        return h(
          "div",
          {
            class: nested ? "route-tab-surface" : "route-surface",
            "data-route-key": nested ? undefined : key,
          },
          nested ? content : h("div", { class: "route-surface-content" }, content),
        );
      };
    },
  });

  return () => {
    if (window.matchMedia("(max-width: 768px)").matches) return Promise.resolve(Surface);
    return load().then(() => Surface);
  };
}
