/** Shared between `quantizeClient` and its callers; no runtime imports. */
export interface QuantizeResult {
  /** Highest-scored source colour as `0xAARRGGBB`, or `null` when unavailable. */
  argb: number | null;
  /** Set when the worker (or the fallback) could not produce an answer. */
  error?: string;
}
