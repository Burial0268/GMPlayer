export const INLINE_QUEUE_MEDIA_QUERY = "(min-width: 1041px)";
// 队列有三种呈现，按外壳形态分界，而不是按「宽/窄」二分：
//   ≥1041px  内联队列列（App.vue 的 .queue-column）
//   769–1040 右侧抽屉（仍是桌面外壳：有 Sidebar，无 TabBar）
//   ≤768px   底部抽屉（移动外壳：TabBar + 迷你播放条，见 global.scss 同一断点）
// 768 这个界必须和 --app-tab-bar-height / --app-bottom-chrome 的断点一致，
// 否则会出现「有 TabBar 却弹右侧抽屉」的中间态。
export const PLAYLIST_DRAWER_MEDIA_QUERY = "(min-width: 769px) and (max-width: 1040px)";
export const PLAYLIST_SHEET_MEDIA_QUERY = "(max-width: 768px)";

export const isInlineQueueLayout = () => {
  if (typeof window === "undefined") return false;
  return window.matchMedia(INLINE_QUEUE_MEDIA_QUERY).matches;
};
