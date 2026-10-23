<template>
  <div ref="host" class="app-layer-host" @wheel.passive="cancelMotion">
    <router-view v-slot="{ Component, route }">
      <Transition
        :css="false"
        :mode="isMobile ? undefined : 'out-in'"
        @before-enter="beforeEnter"
        @enter="enter"
        @before-leave="beforeLeave"
        @leave="leave"
        @enter-cancelled="cancelMotion"
        @leave-cancelled="cancelMotion"
      >
        <KeepAlive :max="10">
          <component :is="Component" :key="pageCacheKey(route)" />
        </KeepAlive>
      </Transition>
    </router-view>
  </div>
</template>

<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { animateMini } from "motion-v";
import { useLayerNavigation, pageCacheKey } from "@/utils/navigation";
import {
  remember,
  viewStateKey,
  type AppLayer,
  type LayerTransition,
} from "@/utils/navigation/layers";
import { rectOf, restoreSourceFocus, type SurfaceRect } from "@/utils/navigation/sources";
import { animateRouteLayers, type InterruptedSurface } from "@/utils/navigation/routeMotion";
import { layerMotion, motionDuration } from "@/utils/navigation/motion";
import { useLayerPresentation } from "@/composables/useLayerPresentation";

const navigation = useLayerNavigation();
useLayerPresentation();
const host = ref<HTMLElement>();
const isMobile = ref(window.matchMedia("(max-width: 768px)").matches);
const scrollStates = new Map<string, { top: number; left: number }>();
const originalStyles = new Map<HTMLElement, string | null>();
const leaving = new Map<HTMLElement, () => void>();
const outgoingRects = new Map<HTMLElement, SurfaceRect>();
let active: ReturnType<typeof animateRouteLayers> | undefined;
let interrupted: InterruptedSurface | undefined;
let desktopAnimation: ReturnType<typeof animateMini> | undefined;
let desktopDone: (() => void) | undefined;
let frame = 0;
let run = 0;
let hostStyle: string | null | undefined;
let scrollOverflow: string | undefined;
let enterDone: (() => void) | undefined;

const scroller = () => host.value?.closest<HTMLElement>(".n-scrollbar-container");
const routeLayer = (): AppLayer | undefined =>
  [...navigation.state.value.layers].reverse().find((layer) => layer.presentation === "page");
const saveStyle = (element: HTMLElement) => {
  if (!originalStyles.has(element)) originalStyles.set(element, element.getAttribute("style"));
};
const restoreStyle = (element: HTMLElement) => {
  if (!originalStyles.has(element)) return;
  const style = originalStyles.get(element);
  if (style === null || style === undefined) element.removeAttribute("style");
  else element.setAttribute("style", style);
  originalStyles.delete(element);
  element.inert = false;
  element.removeAttribute("aria-hidden");
};

const saveScroll = () => {
  const element = scroller();
  const layer = routeLayer();
  if (element && layer)
    remember(scrollStates, viewStateKey(layer), {
      top: element.scrollTop,
      left: element.scrollLeft,
    });
};

function positionLeavingPages() {
  if (!host.value) return;
  const bounds = rectOf(host.value);
  leaving.forEach((_done, element) => {
    const rect = outgoingRects.get(element)!;
    element.style.left = `${rect.x - bounds.x}px`;
    element.style.top = `${rect.y - bounds.y}px`;
  });
}

function releaseLayout() {
  const element = scroller();
  if (element && scrollOverflow !== undefined) element.style.overflowY = scrollOverflow;
  scrollOverflow = undefined;
  if (host.value && hostStyle !== undefined) {
    if (hostStyle === null) host.value.removeAttribute("style");
    else host.value.setAttribute("style", hostStyle);
  }
  hostStyle = undefined;
  delete document.documentElement.dataset.navigationOverlayHandoff;
}

function cancelMotion() {
  run++;
  if (frame) cancelAnimationFrame(frame);
  frame = 0;
  active?.cancel();
  active = undefined;
  desktopAnimation?.cancel();
  desktopAnimation = undefined;
  desktopDone?.();
  desktopDone = undefined;
  originalStyles.forEach((_value, element) => restoreStyle(element));
  const callbacks = [...leaving.values()];
  leaving.clear();
  outgoingRects.clear();
  callbacks.forEach((done) => done());
  enterDone?.();
  enterDone = undefined;
  releaseLayout();
  navigation.finishTransition(navigation.transition.value.id);
}

const removeBefore = navigation.onBeforeNavigation(() => {
  saveScroll();
  interrupted = active?.snapshot();
  cancelMotion();
});
const removeCancelled = navigation.onNavigationCancelled(cancelMotion);

watch(
  () => navigation.transition.value.id,
  () => {
    const change = navigation.transition.value;
    const from = change.from.at(-1);
    const to = change.to.at(-1);
    if (
      !change.routeChanged &&
      from?.presentation === "page" &&
      to?.presentation === "page" &&
      from.route !== to.route
    ) {
      const saved = scrollStates.get(viewStateKey(to)) ?? { top: 0, left: 0 };
      scroller()?.scrollTo({ ...saved, behavior: "instant" });
    }
  },
  { flush: "post" },
);

function beforeLeave(element: Element) {
  if (!(element instanceof HTMLElement)) return;
  saveStyle(element);
  outgoingRects.set(element, rectOf(element));
  element.inert = true;
  element.setAttribute("aria-hidden", "true");
}

function leave(element: Element, done: () => void) {
  if (!(element instanceof HTMLElement)) return done();
  if (!isMobile.value) {
    desktopDone = () => {
      restoreStyle(element);
      done();
    };
    desktopAnimation = animateMini(
      element,
      { opacity: [1, 0], transform: ["translateY(0px)", "translateY(-8px)"] },
      {
        duration: motionDuration(layerMotion.desktop),
        ease: layerMotion.ease,
      },
    );
    void Promise.resolve(desktopAnimation).then(() => {
      desktopDone?.();
      desktopDone = undefined;
    });
    return;
  }
  leaving.set(element, done);
  // Remove the leaving page from flow before restoring the shared scrollport.
  const rect = outgoingRects.get(element)!;
  Object.assign(element.style, {
    position: "absolute",
    width: `${rect.width}px`,
    margin: "0",
    pointerEvents: "none",
    zIndex: "2",
  });
  positionLeavingPages();
}

function beforeEnter(element: Element) {
  if (!(element instanceof HTMLElement) || !host.value) return;
  saveStyle(element);
  const scroll = scroller();
  const layer = routeLayer();
  if (!scroll || !layer) return;
  const saved = scrollStates.get(viewStateKey(layer)) ?? { top: 0, left: 0 };
  hostStyle = host.value.getAttribute("style");
  // Restore before KeepAlive activation so virtual rows see the parent's position immediately.
  host.value.style.minHeight = `${Math.max(scroll.clientHeight, saved.top + scroll.clientHeight)}px`;
  scroll.scrollTo({ top: saved.top, left: saved.left, behavior: "instant" });
  scrollOverflow = scroll.style.overflowY;
  scroll.style.overflowY = "hidden";
  // Visibility is inherited and can start a separate transition on cached text and icons.
  if (isMobile.value) element.style.opacity = "0";
  // Scroll restoration must not move the visible page while its projection is being staged.
  positionLeavingPages();
}

async function enter(element: Element, done: () => void) {
  if (!(element instanceof HTMLElement) || !host.value) return done();
  const request = navigation.transition.value as LayerTransition;
  const token = ++run;
  enterDone = done;
  const complete = () => {
    restoreStyle(element);
    leaving.forEach((leaveDone, old) => {
      restoreStyle(old);
      leaveDone();
    });
    leaving.clear();
    outgoingRects.clear();
    enterDone = undefined;
    done();
    releaseLayout();
    navigation.finishTransition(request.id);
    if (token !== run) return;
    if (request.direction === "pop" && restoreSourceFocus(request.source)) return;
    if (navigation.current.value?.presentation !== "page") return;
    const heading =
      [...element.querySelectorAll<HTMLElement>("h1, [data-navigation-title]")].find(
        (candidate) => candidate.getClientRects().length > 0,
      ) ?? element;
    heading.setAttribute("tabindex", "-1");
    heading.focus({ preventScroll: true });
  };
  if (!isMobile.value) {
    desktopDone = complete;
    desktopAnimation = animateMini(
      element,
      { opacity: [0, 1], transform: ["translateY(8px)", "translateY(0px)"] },
      {
        duration: motionDuration(request.direction === "none" ? 0 : layerMotion.desktop),
        ease: layerMotion.ease,
      },
    );
    void Promise.resolve(desktopAnimation).then(() => {
      if (token === run) {
        desktopDone = undefined;
        complete();
      }
    });
    return;
  }
  await nextTick();
  if (token !== run || !host.value) return;
  frame = requestAnimationFrame(() => {
    frame = 0;
    if (token !== run || !host.value) return;
    positionLeavingPages();
    element.style.opacity = "";
    element.style.position = "relative";
    element.style.zIndex = request.direction === "pop" ? "1" : "3";
    if (
      request.from.at(-1)?.presentation !== "page" ||
      request.to.at(-1)?.presentation !== "page"
    ) {
      document.documentElement.dataset.navigationOverlayHandoff = "";
    }
    active = animateRouteLayers({
      incoming: element,
      outgoing: leaving.keys().next().value,
      host: host.value,
      transition: request,
      interrupted,
      complete,
    });
    interrupted = undefined;
  });
}

const resized = () => {
  cancelMotion();
  isMobile.value = window.matchMedia("(max-width: 768px)").matches;
};
const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
onMounted(() => {
  window.addEventListener("resize", resized);
  window.visualViewport?.addEventListener("resize", cancelMotion);
  window.addEventListener("gmplayer-cancel-layer-motion", cancelMotion);
  reducedMotion.addEventListener("change", cancelMotion);
});
onBeforeUnmount(() => {
  cancelMotion();
  removeBefore();
  removeCancelled();
  window.removeEventListener("resize", resized);
  window.visualViewport?.removeEventListener("resize", cancelMotion);
  window.removeEventListener("gmplayer-cancel-layer-motion", cancelMotion);
  reducedMotion.removeEventListener("change", cancelMotion);
});
</script>

<style lang="scss">
.app-layer-host {
  position: relative;
  min-width: 0;
}
.settings-main > .app-layer-host,
.settings-main .route-surface,
.settings-main .route-surface-content {
  height: 100%;
}
.route-surface {
  position: relative;
  min-width: 0;
}
.route-surface-content {
  display: flow-root;
}
.route-surface:focus,
.route-surface [data-navigation-title]:focus,
.route-surface h1[tabindex="-1"]:focus {
  outline: none;
}
.route-surface[data-navigation-cover-hidden] [data-navigation-cover="page"] {
  visibility: hidden !important;
}
.route-surface[data-navigation-title-hidden] [data-navigation-title="page"] {
  visibility: hidden !important;
}
.route-surface[data-navigation-cover-hidden] [data-navigation-cover="page"] .route-shadow {
  opacity: 0;
}
.route-surface[data-navigation-surface]::before {
  content: "";
  position: absolute;
  top: var(--route-surface-top, 0px);
  left: var(--route-surface-left, 0px);
  width: var(--route-surface-width, 100%);
  height: var(--route-surface-height, 100%);
  z-index: -1;
  background: var(--app-shell-bg, #fff);
  pointer-events: none;
}
html[data-navigation-overlay-handoff] .app-layout > .n-layout-content {
  z-index: 2300;
}
html[data-navigation-overlay-handoff] .app-nav-overlay {
  z-index: 2400;
}
html[data-navigation-overlay-handoff] .content-top-shadow {
  z-index: 2350;
}
@media (prefers-reduced-motion: reduce) {
  .route-surface {
    scroll-behavior: auto;
  }
}
</style>
