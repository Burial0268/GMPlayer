import { ref, computed, type Ref, type ComputedRef } from "vue";

type MaybeComputedRef<T> = Ref<T> | ComputedRef<T>;

export function useMiniCoverFrame(
  containerRef: MaybeComputedRef<HTMLElement | null>,
  phonyBigRef: MaybeComputedRef<HTMLElement | null>,
  phonySmallRef: MaybeComputedRef<HTMLElement | null>,
  currentLayer: Ref<number>,
) {
  const currentCoverStyle = ref<{
    width: number;
    height: number;
    left: number;
    top: number;
    borderRadius: number;
  } | null>(null);

  const calcCoverLayout = (isBigCover = true) => {
    const root = containerRef.value;
    if (!root) return undefined;
    const target = isBigCover ? phonyBigRef.value : phonySmallRef.value;
    if (!target) return undefined;

    let rootEl: HTMLElement = root;
    while (getComputedStyle(rootEl).display === "contents") {
      rootEl = rootEl.parentElement!;
    }
    const rootB = rootEl.getBoundingClientRect();
    const targetB = target.getBoundingClientRect();
    const size = Math.min(target.clientWidth, target.clientHeight);
    if (size <= 0) return undefined;

    return {
      width: size,
      height: size,
      left: targetB.x - rootB.x + (targetB.width - size) / 2,
      top: targetB.y - rootB.y + (targetB.height - size) / 2,
      borderRadius: isBigCover ? 12 : 8,
    };
  };

  const updateCoverStyle = () => {
    const isBigCover = currentLayer.value === 1;
    currentCoverStyle.value = calcCoverLayout(isBigCover);
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
    if (phonyBigRef.value) layoutResizeObserver.observe(phonyBigRef.value);
    if (phonySmallRef.value) layoutResizeObserver.observe(phonySmallRef.value);
    if (containerRef.value) layoutResizeObserver.observe(containerRef.value);
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
