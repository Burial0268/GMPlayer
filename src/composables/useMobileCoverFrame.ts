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

    return {
      width: size,
      height: size,
      left: targetB.x - rootB.x + (targetB.width - size) / 2,
      top: targetB.y - rootB.y + (targetB.height - size) / 2,
      borderRadius: hideLyric ? 12 : 8,
    };
  };

  return {
    calcCoverLayout,
  };
}
