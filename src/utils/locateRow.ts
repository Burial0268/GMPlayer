import { nextTick } from "vue";

/**
 * 把列表滚到让某一行落在指定位置，并且真的落在那里。
 *
 * 算一次 scrollTop 再 `scrollTo` 在这个应用里总差一截，因为列表会在滚动途中自己挪行：
 * vueuc 的 `n-virtual-list` 量到真实行高后 `scrollBy` 补偿（顺带打断原生平滑滚动）；
 * DataLists 的页面窗口按统一行高切片，带别名的行更高，每重切一次下面整体错开；两者都
 * 只渲染视口附近的行，目标常常根本不在 DOM 里。所以目标每一帧都从行的实时位置重新量，
 * 滚动收敛到行实际所在的地方。
 */
export type LocateAlign = "start" | "center" | "nearest";

export interface LocateRowOptions {
  /** 真正在滚的元素。 */
  scroller: HTMLElement;
  /** 每次现查：虚拟化列表会换掉行节点。 */
  findRow: () => HTMLElement | null;
  align: LocateAlign;
  /** 滚动视口上下被浮层（Nav、sticky 工具栏、底部播放条）盖住的高度，px。 */
  insets?: () => { top: number; bottom: number };
  /** 贴边对齐时与可视带边缘留的空隙。 */
  margin?: number;
  /** 目标行不在 DOM 里时，瞬时跳到它附近把它渲染出来。 */
  materialize?: () => void;
  /** 程序化滚动之后让列表当场重排，之后量到的才是重排后的位置。 */
  sync?: () => unknown;
  instant?: boolean;
}

export interface LocateHandle {
  /** 落定为 true；被用户打断、或目标行没能渲染出来为 false。 */
  done: Promise<boolean>;
  cancel: () => void;
}

/** vueuc 刚挂载时要等 ResizeObserver 量到视口高度才渲染行。 */
const MATERIALIZE_FRAMES = 12;
/** 每轮残差来自窗口重切，几何收敛。 */
const SETTLE_PASSES = 4;
/** 临界阻尼：不会滚过目标再弹回来。0.3 s 的响应与 `miuixSpring` 同一量级。 */
const OMEGA = (2 * Math.PI) / 0.3;
/** 目标一直在动（图片撑开行之类）时的兜底：到点直接对准。 */
const GLIDE_LIMIT_MS = 1500;
const INTERRUPTS = ["wheel", "touchstart", "pointerdown", "keydown"] as const;

const nextFrame = () => new Promise<number>((resolve) => requestAnimationFrame(resolve));

export function locateRow(options: LocateRowOptions): LocateHandle {
  const { scroller, findRow, align, insets, margin = 0, materialize, sync, instant } = options;
  let cancelled = false;
  const cancel = () => {
    cancelled = true;
  };
  // 用户一动就交还控制权。挂在 window 上：拖 naive 的滚动条落在滚动容器之外。
  for (const type of INTERRUPTS) {
    window.addEventListener(type, cancel, { capture: true, passive: true });
  }

  /** 这一行落到位时 scrollTop 应该是多少。 */
  const goalFor = (row: HTMLElement): number => {
    const box = scroller.getBoundingClientRect();
    const rect = row.getBoundingClientRect();
    const inset = insets?.() ?? { top: 0, bottom: 0 };
    const top = box.top + inset.top + margin;
    const bottom = box.bottom - inset.bottom - margin;
    let delta = 0;
    if (align === "center") {
      delta = rect.top + rect.height / 2 - (top + bottom) / 2;
    } else if (align === "start" || rect.top < top || rect.height > bottom - top) {
      delta = rect.top - top;
    } else if (rect.bottom > bottom) {
      delta = rect.bottom - bottom;
    }
    const max = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
    return Math.min(max, Math.max(0, scroller.scrollTop + delta));
  };

  /** 瞬时对准；每写一次都让列表重排再重量，直到行不再挪动。 */
  const settle = async (): Promise<boolean> => {
    for (let pass = 0; pass < SETTLE_PASSES; pass++) {
      const row = findRow();
      if (cancelled || !row) return false;
      const goal = goalFor(row);
      if (Math.abs(goal - scroller.scrollTop) < 1) return true;
      scroller.scrollTop = goal;
      await sync?.();
    }
    return true;
  };

  const glide = async (): Promise<boolean> => {
    let position = scroller.scrollTop;
    let velocity = 0;
    let applied = scroller.scrollTop;
    let last: number | undefined;
    const deadline = performance.now() + GLIDE_LIMIT_MS;
    for (;;) {
      const now = await nextFrame();
      const row = findRow();
      if (cancelled || !row) return false;
      if (now > deadline) return settle();
      // 列表自己挪过 scrollTop（vueuc 的行高补偿）：认下这段位移，否则下一帧写回去
      // 就把它抹掉，内容会跳一下。
      if (Math.abs(scroller.scrollTop - applied) > 1) position += scroller.scrollTop - applied;
      const goal = goalFor(row);
      const dt = last === undefined ? 1 / 60 : Math.min(0.05, Math.max(0, (now - last) / 1000));
      last = now;
      // 临界阻尼弹簧的解析步进：任意 dt 都稳定，目标每帧可以换。
      const offset = position - goal;
      const c = velocity + OMEGA * offset;
      const decay = Math.exp(-OMEGA * dt);
      const next = (offset + c * dt) * decay;
      velocity = (c - OMEGA * (offset + c * dt)) * decay;
      position = goal + next;
      const arrived = Math.abs(next) < 0.5 && Math.abs(velocity) < 20;
      scroller.scrollTop = arrived ? goal : position;
      applied = scroller.scrollTop;
      if (arrived) return settle();
    }
  };

  const done = (async () => {
    try {
      let row = findRow();
      if (!row && materialize) {
        materialize();
        await sync?.();
        for (let frame = 0; !row && !cancelled && frame < MATERIALIZE_FRAMES; frame++) {
          row = findRow();
          if (!row) await nextFrame();
        }
        // 已经跳过去了，视线早已脱离原处，剩下几像素的校正不值得再滑一段。
        return row && !cancelled ? await settle() : false;
      }
      if (!row || cancelled) return false;
      return instant ? await settle() : await glide();
    } finally {
      for (const type of INTERRUPTS) {
        window.removeEventListener(type, cancel, { capture: true });
      }
    }
  })();

  return { done, cancel };
}

/** naive `n-virtual-list` 实例上用得到的那两个方法。 */
export interface VirtualListInst {
  scrollTo: (options: { index?: number; top?: number; debounce?: boolean }) => void;
  getScrollContainer: () => HTMLElement | null | undefined;
}

/** 在 `n-virtual-list` 自己的滚动容器里定位第 `index` 行。 */
export const locateVirtualRow = (
  list: VirtualListInst | null | undefined,
  index: number,
  options: {
    rowSelector: string;
    align: LocateAlign;
    /** 跳转时目标行上方先让出几行，居中时一跳就落在中间附近。 */
    lead?: number;
    instant?: boolean;
  },
): LocateHandle | null => {
  const scroller = list?.getScrollContainer();
  if (!list || !scroller || index < 0) return null;
  return locateRow({
    scroller,
    findRow: () => scroller.querySelector<HTMLElement>(options.rowSelector),
    align: options.align,
    // `debounce: false` 是 vueuc 唯一的顶对齐，在它自己的行高坐标系里精确。
    materialize: () =>
      list.scrollTo({ index: Math.max(0, index - (options.lead ?? 0)), debounce: false }),
    // vueuc 只在 scroll 事件里同步视口；手动派发一次，重渲染就发生在绘制之前。
    sync: () => {
      scroller.dispatchEvent(new Event("scroll"));
      return nextTick();
    },
    instant: options.instant,
  });
};
