import { ref, shallowRef, type VNodeChild } from "vue";

/**
 * The subset of naive's `DialogOptions` the app's confirmations use. `window.$dialog`
 * is backed by `ConfirmSheet` (a GlassSheet) instead of naive's dialog, so every
 * confirmation has the same look as the other sheets without touching its call site.
 */
export interface ConfirmOptions {
  title?: string;
  content?: string | (() => VNodeChild);
  positiveText?: string;
  negativeText?: string;
  /** Awaited before closing, as naive does; returning `false` keeps the sheet open. */
  onPositiveClick?: () => unknown;
  onNegativeClick?: () => unknown;
  // Accepted for compatibility with naive's options; the sheet has one look.
  type?: string;
  class?: string;
}

export const confirmState = shallowRef<ConfirmOptions | null>(null);
export const confirmShow = ref(false);

const open = (options: ConfirmOptions) => {
  confirmState.value = options;
  confirmShow.value = true;
};

export const confirmApi = { create: open, warning: open, info: open, error: open, success: open };
export type ConfirmApi = typeof confirmApi;
