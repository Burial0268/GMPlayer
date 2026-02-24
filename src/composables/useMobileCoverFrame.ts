import { ref, computed, type Ref, type ComputedRef } from "vue";

type MaybeComputedRef<T> = Ref<T> | ComputedRef<T>;

export function useMobileCoverFrame(
  bigPlayerRef: MaybeComputedRef<HTMLElement | null>,
  phonyBigCoverRef: MaybeComputedRef<HTMLElement | null>,
  phonySmallCoverRef: MaybeComputedRef<HTMLElement | null>,
  mobileLayer: Ref<number>,
) {
  const currentCoverStyle = ref<{
    width: number;
    height: number;
    left: number;
    top: number;
    borderRadius: number;
  } | null>(null);

  const calcCoverLayout = (hideLyric = true) => {
    const root = bigPlayerRef.value;
    if (!root) return undefined;
    const targetCover = hideLyric ? phonyBigCoverRef.value : phonySmallCoverRef.value;
    if (!targetCover) return undefined;

    let rootEl: HTMLElement = root;
    while (getComputedStyle(rootEl).display === "contents") {
      rootEl = rootEl.parentElement!;
    }
    const rootB = rootEl.getBoundingClientRect();
    const targetB = targetCover.getBoundingClientRect();
    const size = Math.min(targetCover.clientWidth, targetCover.clientHeight);
    if (size <= 0) return undefined;

    return {
      width: size,
      height: size,
      left: targetB.x - rootB.x + (targetB.width - size) / 2,
      top: targetB.y - rootB.y + (targetB.height - size) / 2,
      borderRadius: hideLyric ? 12 : 8,
    };
  };

  const updateCoverStyle = () => {
    const hideLyric = mobileLayer.value === 1;
    currentCoverStyle.value = calcCoverLayout(hideLyric);
  };

  const coverFrameStyle = computed(() => {
    const s = currentCoverStyle.value;
    if (!s) return {};
    return {
      width: s.width + "px",
      height: s.height + "px",
      left: s.left + "px",
      top: s.top + "px",
      borderRadius: s.borderRadius + "px",
    };
  });

  let layoutResizeObserver: ResizeObserver | null = null;

  const setupResizeObserver = () => {
    cleanupResizeObserver();
    updateCoverStyle();
    layoutResizeObserver = new ResizeObserver(updateCoverStyle);
    if (phonyBigCoverRef.value) layoutResizeObserver.observe(phonyBigCoverRef.value);
    if (phonySmallCoverRef.value) layoutResizeObserver.observe(phonySmallCoverRef.value);
    if (bigPlayerRef.value) layoutResizeObserver.observe(bigPlayerRef.value);
  };

  const cleanupResizeObserver = () => {
    layoutResizeObserver?.disconnect();
    layoutResizeObserver = null;
  };

  return {
    currentCoverStyle,
    coverFrameStyle,
    updateCoverStyle,
    calcCoverLayout,
    setupResizeObserver,
    cleanupResizeObserver,
  };
}
