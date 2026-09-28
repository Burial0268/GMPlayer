import { nextTick, onBeforeUnmount, onMounted, watch } from "vue";
import { musicStore, siteStore } from "@/store";
import { useLayerNavigation } from "@/utils/navigation";
import { restoreSourceFocus } from "@/utils/navigation/sources";
import { useResponsiveLayout } from "@/composables/useResponsiveLayout";

const FOCUSABLE = "button, a[href], input, textarea, select, [tabindex]:not([tabindex='-1'])";

export function useLayerPresentation() {
  const navigation = useLayerNavigation();
  const { isMobile } = useResponsiveLayout();
  const music = musicStore();
  const site = siteStore();
  const inertElements = new Map<HTMLElement, boolean>();
  let focusGeneration = 0;

  // Compatibility fields are projections. UI actions only write through navigation.
  watch(
    [navigation.playerVisible, navigation.queueVisible, navigation.searchVisible],
    ([player, queue, search]) => {
      music.setBigPlayerState(player);
      music.showPlayList = queue;
      site.searchInputActive = search;
    },
    { immediate: true, flush: "sync" },
  );

  const activeSurface = (): HTMLElement | undefined => {
    const top = navigation.current.value;
    if (!top || top.presentation === "page") return;
    const selector =
      top.presentation === "queue-floating"
        ? ".playlist-sheet-layer, .playlist-drawer"
        : `[data-navigation-layer='${top.presentation}']`;
    return [...document.querySelectorAll<HTMLElement>(selector)].find(
      (element) =>
        element.getClientRects().length && getComputedStyle(element).visibility !== "hidden",
    );
  };
  const modalSurface = (surface = activeSurface()) => {
    const presentation = navigation.current.value?.presentation;
    if (!isMobile.value) {
      if (presentation === "queue-floating") return;
      // The desktop queue shares the player's controls and modal boundary.
      if (presentation === "queue-player") {
        return surface?.closest<HTMLElement>("[data-navigation-layer='player']") ?? undefined;
      }
    }
    return surface;
  };

  const clearInert = () => {
    inertElements.forEach((value, element) => {
      element.inert = value;
    });
    inertElements.clear();
  };

  const focusable = (surface: HTMLElement) =>
    [...surface.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
      (element) =>
        !element.closest("[inert], [hidden]") &&
        !element.matches(":disabled") &&
        element.getClientRects().length,
    );

  watch(
    () => [navigation.current.value?.id, navigation.current.value?.presentation, isMobile.value],
    async ([, , mobile], previous) => {
      const generation = ++focusGeneration;
      clearInert();
      await nextTick();
      if (previous && mobile !== previous[2]) {
        // Other resize listeners replace the player layout after this watcher runs.
        await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
      }
      if (generation !== focusGeneration) return;
      const surface = activeSurface();
      if (!surface) {
        if (
          !navigation.transition.value.routeChanged &&
          navigation.transition.value.direction === "pop"
        ) {
          restoreSourceFocus(navigation.transition.value.source);
        }
        return;
      }
      const modal = modalSurface(surface);
      if (modal) {
        let branch: HTMLElement = modal;
        while (branch.parentElement) {
          for (const sibling of branch.parentElement.children) {
            if (
              !(sibling instanceof HTMLElement) ||
              sibling === branch ||
              sibling.matches(
                // `.titlebar--floating` is the desktop window-control cluster (min/max/close),
                // teleported to <body> so it can out-stack the body-level BigPlayer. As a direct
                // sibling of the player surface it would otherwise be caught by this focus-trap and
                // set `inert` — leaving the controls visible but unclickable, so the user cannot
                // close the app while the player is open. OS window chrome is never trapped by an
                // in-page modal; keep it live alongside the other global chrome below.
                "script, style, link, .titlebar--floating, .v-binder-follower-container, .n-modal-container, .n-message-container, .n-notification-container",
              )
            )
              continue;
            inertElements.set(sibling, sibling.inert);
            sibling.inert = true;
          }
          branch = branch.parentElement;
          if (branch === document.body) break;
        }
      }
      const target =
        surface.querySelector<HTMLInputElement>("input") ?? focusable(surface)[0] ?? surface;
      if (target === surface) surface.setAttribute("tabindex", "-1");
      target.focus({ preventScroll: true });
    },
    { immediate: true, flush: "post" },
  );

  const nativeModal = () =>
    [
      ...document.querySelectorAll<HTMLElement>(
        ".n-modal-container .n-modal, .n-dialog, .n-image-preview-container",
      ),
    ].some((element) => element.getClientRects().length && !element.closest("[inert]"));

  const handleBack = () => {
    if (nativeModal()) {
      document.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Escape",
          code: "Escape",
          bubbles: true,
          cancelable: true,
        }),
      );
      return true;
    }
    return navigation.closeTop();
  };
  const keydown = (event: KeyboardEvent) => {
    if (event.defaultPrevented) return;
    const surface = modalSurface();
    // Naive's dialogs and image preview own Escape while they are above an app layer.
    if (nativeModal()) return;
    if (event.key === "Escape" && navigation.canGoBack.value) {
      event.preventDefault();
      handleBack();
    } else if (event.key === "Tab" && surface) {
      const items = focusable(surface);
      const first = items[0] ?? surface;
      const last = items.at(-1) ?? surface;
      if (
        event.shiftKey &&
        (document.activeElement === first || !surface.contains(document.activeElement))
      ) {
        event.preventDefault();
        last.focus({ preventScroll: true });
      } else if (
        !event.shiftKey &&
        (document.activeElement === last || !surface.contains(document.activeElement))
      ) {
        event.preventDefault();
        first.focus({ preventScroll: true });
      }
    }
  };
  const nativeWindow = window as Window & { __gmplayerBack?: () => boolean };
  const queueMedia = window.matchMedia("(min-width: 1041px)");
  const queueLayoutChanged = () => navigation.syncQueueLayout(queueMedia.matches);
  onMounted(() => {
    queueMedia.addEventListener("change", queueLayoutChanged);
    window.addEventListener("keydown", keydown);
    nativeWindow.__gmplayerBack = handleBack;
  });
  onBeforeUnmount(() => {
    queueMedia.removeEventListener("change", queueLayoutChanged);
    focusGeneration++;
    clearInert();
    window.removeEventListener("keydown", keydown);
    delete nativeWindow.__gmplayerBack;
  });
}
