export const INLINE_QUEUE_MEDIA_QUERY = "(min-width: 1041px)";
export const PLAYLIST_DRAWER_MEDIA_QUERY = "(max-width: 1040px)";

export const isInlineQueueLayout = () => {
  if (typeof window === "undefined") return false;
  return window.matchMedia(INLINE_QUEUE_MEDIA_QUERY).matches;
};
