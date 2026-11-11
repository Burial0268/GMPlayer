import type { ComputedRef, Ref } from "vue";

type MaybeComputedRef<T> = Ref<T> | ComputedRef<T>;

export function useMobileCoverFrame(
  bigPlayerRef: MaybeComputedRef<HTMLElement | null>,
  phonyBigCoverRef: MaybeComputedRef<HTMLElement | null>,
  phonySmallCoverRef: MaybeComputedRef<HTMLElement | null>,
) {
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
    // The queue pager translates the layouts, not this page: a transform on the
    // page would isolate plus-lighter for the whole slide. Strip those shifts
    // so the frame stays in page space; the cover applies the same shift itself.
    let shiftX = 0;
    let shiftY = 0;
    for (let node = targetCover.parentElement; node && node !== rootEl; node = node.parentElement) {
      const { transform } = getComputedStyle(node);
      if (transform === "none") continue;
      const matrix = new DOMMatrixReadOnly(transform);
      shiftX += matrix.m41;
      shiftY += matrix.m42;
    }

    return {
      width: size,
      height: size,
      left: targetB.x - rootB.x - shiftX + (targetB.width - size) / 2,
      top: targetB.y - rootB.y - shiftY + (targetB.height - size) / 2,
      borderRadius: hideLyric ? 12 : 8,
    };
  };

  return {
    calcCoverLayout,
  };
}
