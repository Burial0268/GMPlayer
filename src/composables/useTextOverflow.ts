import { nextTick, onBeforeUnmount, ref, watch, type Ref, type WatchSource } from "vue";

/**
 * Whether a line-clamped element is actually cutting its text off.
 *
 * Replaces `text.length > 70`, which ignored line breaks and the container's
 * width: a short description with newlines was clamped with no way to read the
 * rest, and a long one on a wide screen offered a button that showed nothing new.
 */
export const useTextOverflow = (el: Ref<HTMLElement | null>, text: WatchSource<unknown>) => {
  const overflowing = ref(false);
  const measure = () => {
    const node = el.value;
    overflowing.value = !!node && node.scrollHeight > node.clientHeight + 1;
  };
  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);

  watch(
    el,
    (node, old) => {
      if (old) observer?.unobserve(old);
      if (node) observer?.observe(node);
      measure();
    },
    { flush: "post" },
  );
  watch(text, () => void nextTick(measure), { flush: "post" });
  onBeforeUnmount(() => observer?.disconnect());

  return overflowing;
};
