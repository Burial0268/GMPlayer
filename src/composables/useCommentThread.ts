import { computed, ref, watch, type Ref } from "vue";
import { getComment } from "@/api/comment";
import type { CommentResourceType } from "@/api/types";

type CommentItem = { commentId: number; time: number; [key: string]: any };

/**
 * One resource's comment thread: hot comments, the "all" list paged by "load more", and which
 * of the two is showing. Shared by the comment page, the MV page and the desktop player's panel,
 * so they page, fail and switch tabs the same way.
 *
 * `id` null means "no thread" (a local file has none) and shows the empty state without a request.
 * Rows are deep-reactive on purpose: Comment toggles `liked` in place, and the cache in
 * `api/comment` hands every view the same objects, so a like shows everywhere.
 */
export function useCommentThread(id: Ref<number | null>, type: CommentResourceType = "music") {
  const tab = ref<"hot" | "all">("hot");
  const hot = ref<CommentItem[]>([]);
  const all = ref<CommentItem[]>([]);
  const total = ref(0);
  const hasMore = ref(false);
  const loading = ref(false);
  const loadingMore = ref(false);
  const error = ref(false);
  let serial = 0;

  const visible = computed(() => (tab.value === "hot" ? hot.value : all.value));

  const clear = () => {
    hot.value = [];
    all.value = [];
    total.value = 0;
    hasMore.value = false;
  };

  const request = async (append: boolean, fresh = false) => {
    const current = id.value;
    const requestId = ++serial;
    if (!current) {
      clear();
      loading.value = false;
      loadingMore.value = false;
      error.value = false;
      return;
    }
    const offset = append ? all.value.length : 0;
    // Past offset 5000 Netease pages by the time of the last comment already shown.
    const before = append && offset >= 5000 ? (all.value.at(-1)?.time ?? null) : null;
    if (append) loadingMore.value = true;
    else {
      loading.value = true;
      error.value = false;
    }

    try {
      const res = await getComment(current, offset, before, type, fresh);
      if (requestId !== serial) return;
      const comments: CommentItem[] = Array.isArray(res?.comments) ? res.comments : [];
      if (append) {
        all.value.push(...comments);
      } else {
        hot.value = Array.isArray(res?.hotComments) ? res.hotComments : [];
        // A copy: "load more" pushes onto it, and the first page is shared through the cache.
        all.value = comments.slice();
        total.value = Number(res?.total) || comments.length;
        if (!hot.value.length) tab.value = "all";
      }
      hasMore.value = Boolean(res?.more ?? all.value.length < total.value);
    } catch (err) {
      if (requestId !== serial) return;
      console.error("[comments] failed to load", err);
      if (!append) {
        clear();
        error.value = true;
      }
    } finally {
      if (requestId === serial) {
        loading.value = false;
        loadingMore.value = false;
      }
    }
  };

  const refresh = (fresh = false) => request(false, fresh);
  const loadMore = () => {
    if (!loadingMore.value && hasMore.value) void request(true);
  };

  watch(
    id,
    () => {
      tab.value = "hot";
      clear();
      void request(false);
    },
    { immediate: true },
  );

  return { tab, hot, all, visible, total, hasMore, loading, loadingMore, error, refresh, loadMore };
}
