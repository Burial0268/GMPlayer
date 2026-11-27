/**
 * 主输入设备没有悬停能力（手机、纯触屏平板）。
 *
 * 触屏上点一下会模拟出 `mouseenter`，却永远等不到 `mouseleave`，所以凡是靠悬停触发的
 * 浮层（naive 的 `trigger="hover"` 提示、滚动条）都会在点击后一直停在屏幕上。CSS 侧用
 * `@media (hover: hover)` 处理，脚本 / 组件属性侧用这个。
 *
 * 每次读取而不是缓存：平板接上鼠标或键盘后这个值会变。
 */
export const isTouchOnly = (): boolean =>
  typeof window !== "undefined" && (window.matchMedia?.("(hover: none)")?.matches ?? false);

/**
 * Whether a `contextmenu` came from a long press rather than a mouse right-click.
 *
 * Chromium dispatches it as a PointerEvent carrying the input's `pointerType`, which is what
 * answers the question on a touch laptop or a tablet with a mouse; elsewhere fall back to the
 * primary input. The menu then follows the hand that asked for it: a tray for a finger, a
 * dropdown at the pointer for a mouse.
 */
export const isTouchContextMenu = (event: Event | null | undefined): boolean => {
  const pointerType = (event as PointerEvent | null | undefined)?.pointerType;
  return pointerType ? pointerType !== "mouse" : isTouchOnly();
};
