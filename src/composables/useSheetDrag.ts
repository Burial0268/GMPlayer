import type { MotionValue } from "motion-v";

const DRAG_THRESHOLD_PX = 8;
const DISMISS_PROGRESS = 0.65;
const DISMISS_VELOCITY = 0.6;

const clamp01 = (value: number) => Math.min(1, Math.max(0, value));

/** 手指落点所在的滚动容器已经滚了多远；抓手不在列表里，closest 自然返回 null。 */
const scrollTopAt = (target: EventTarget | null, scrollers: string) => {
  if (!(target instanceof Element)) return 0;
  const scroller = target.closest(scrollers);
  return scroller instanceof HTMLElement ? scroller.scrollTop : 0;
};

type SheetTouch = {
  x: number;
  y: number;
  dragging: boolean;
  fromGrip: boolean;
  scrollTop: number;
  progress: number;
};

/**
 * 底部抽屉的下拉关闭手势（PlayListSheet 与大播放器的更多面板共用）。
 *
 * `progress` 是 0 = 收起、1 = 展开的进度；抽屉自己负责开合动画，这里只在手势里接管它：
 * 拖动时跟手，松手后要么 `dismiss()`，要么 `settleOpen()` 弹回。
 */
export function useSheetDrag(options: {
  progress: MotionValue<number>;
  /** 面板的实际行程（像素），手势按屏幕距离判定。 */
  travel: () => number;
  /** 停掉正在跑的开合动画，手指接管。 */
  stop: () => void;
  settleOpen: () => void;
  dismiss: () => void;
  /** 下拉只在这些滚动容器已到顶时才算关闭手势。 */
  scrollers: string;
  grip: string;
}) {
  const { progress } = options;
  let touchState: SheetTouch | null = null;
  let suppressGripClick = false;

  const reset = () => {
    touchState = null;
    suppressGripClick = false;
  };

  const handleTouchStart = (event: TouchEvent) => {
    const touch = event.changedTouches?.[0];
    if (!touch) return;
    // 每一次新手势都从「不吞点击」重新开始：拖拽结束后若把标记留着，下一次真正的
    // 轻点就会被白白吃掉一次。
    suppressGripClick = false;
    touchState = {
      x: touch.clientX,
      y: touch.clientY,
      dragging: false,
      fromGrip: event.target instanceof Element && Boolean(event.target.closest(options.grip)),
      scrollTop: scrollTopAt(event.target, options.scrollers),
      progress: 0,
    };
  };

  const handleTouchMove = (event: TouchEvent) => {
    const start = touchState;
    const touch = event.changedTouches?.[0];
    if (!start || !touch) return;

    const deltaX = touch.clientX - start.x;
    const deltaY = touch.clientY - start.y;
    if (!start.dragging) {
      if (Math.abs(deltaY) < DRAG_THRESHOLD_PX) return;
      const vertical = Math.abs(deltaY) > Math.abs(deltaX) * 1.15;
      // 只有「列表已经在顶部」或「手指落在抓手上」时才把下拉解释为关闭手势，否则把
      // 手势还给列表自己滚 —— 和 MobilePlayerLayout 队列页的判据一致。
      if (!vertical || deltaY <= 0 || (!start.fromGrip && start.scrollTop > 4)) {
        touchState = null;
        return;
      }
      start.dragging = true;
      suppressGripClick = start.fromGrip;
      // A moving sheet is grabbed where it is, with velocity now owned by the finger.
      start.progress = clamp01(progress.get());
      options.stop();
      progress.jump(start.progress);
    }

    // 拖拽期间接管手势，阻止列表同时滚动（touchmove 不能加 .passive，否则拦不住）
    if (event.cancelable) event.preventDefault();
    progress.set(clamp01(start.progress - deltaY / options.travel()));
  };

  const handleTouchEnd = () => {
    const start = touchState;
    touchState = null;
    if (!start?.dragging) return;
    const velocity = progress.getVelocity();
    // 甩动优先于位置：慢慢拖到一半再松手应该弹回，快速下滑一点点就该关。
    const dismiss =
      velocity < -DISMISS_VELOCITY
        ? true
        : velocity > DISMISS_VELOCITY
          ? false
          : progress.get() < DISMISS_PROGRESS;
    if (dismiss) options.dismiss();
    else options.settleOpen();
  };

  const handleTouchCancel = () => {
    const dragging = touchState?.dragging;
    reset();
    if (!dragging) return;
    options.stop();
    options.settleOpen();
  };

  const handleGripClick = () => {
    // 拖拽时 preventDefault 过，多数引擎不会再合成 click；这一层是兜底，
    // 免得「从抓手拖一半又弹回」顺带被判成一次关闭。
    if (suppressGripClick) {
      suppressGripClick = false;
      return;
    }
    options.dismiss();
  };

  return {
    reset,
    handleTouchStart,
    handleTouchMove,
    handleTouchEnd,
    handleTouchCancel,
    handleGripClick,
  };
}
