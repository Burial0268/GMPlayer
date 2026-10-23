/**
 * Client for `quantize-worker.ts`.
 *
 * Falls back to running the quantizer on the main thread when a worker cannot be
 * created (some embedded WebViews, a CSP that forbids workers). The fallback is
 * a dynamic import so the quantizer stays out of the main bundle in the normal
 * case — the whole point of moving it here.
 */

import type { QuantizeResult } from "./quantizeTypes";

let worker: Worker | null = null;
let workerFailed = false;
let requestId = 0;

const pending = new Map<number, (result: QuantizeResult) => void>();

/** Long enough that a slow first quantize is not abandoned, short enough to fail. */
const REQUEST_TIMEOUT = 8_000;

const settleAll = (reason: string) => {
  for (const [, resolve] of pending) resolve({ argb: null, error: reason });
  pending.clear();
};

const getWorker = (): Worker | null => {
  if (workerFailed) return null;
  if (worker) return worker;
  try {
    worker = new Worker(new URL("./quantize-worker.ts", import.meta.url), { type: "module" });
    worker.onmessage = (e: MessageEvent<{ id: number; argb: number | null; error?: string }>) => {
      const resolve = pending.get(e.data.id);
      if (!resolve) return;
      pending.delete(e.data.id);
      resolve({ argb: e.data.argb, error: e.data.error });
    };
    worker.onerror = () => {
      // Retire the broken worker: keeping it would make every later cover wait
      // out its full timeout against something that cannot answer.
      worker?.terminate();
      worker = null;
      settleAll("quantize worker error");
    };
    return worker;
  } catch {
    workerFailed = true;
    return null;
  }
};

/**
 * Score the dominant colour of a packed-ARGB pixel set.
 *
 * `pixels` is **transferred**, not copied, so the caller must not touch it
 * afterwards — it is a scratch array built per extraction, never the canvas's
 * own buffer.
 */
export const quantizeCoverPixels = (
  pixels: Uint32Array,
  maxColors: number,
): Promise<QuantizeResult> => {
  if (!pixels.length) return Promise.resolve({ argb: null });

  const active = getWorker();
  if (!active) return quantizeOnMainThread(pixels, maxColors);

  const id = ++requestId;
  return new Promise<QuantizeResult>((resolve) => {
    let done = false;
    const finish = (result: QuantizeResult) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      resolve(result);
    };
    const timer = setTimeout(() => {
      pending.delete(id);
      finish({ argb: null, error: "quantize timed out" });
    }, REQUEST_TIMEOUT);

    pending.set(id, finish);
    try {
      active.postMessage({ id, pixels, maxColors }, [pixels.buffer]);
    } catch (err) {
      pending.delete(id);
      finish({ argb: null, error: err instanceof Error ? err.message : String(err) });
    }
  });
};

const quantizeOnMainThread = async (
  pixels: Uint32Array,
  maxColors: number,
): Promise<QuantizeResult> => {
  try {
    const { QuantizerCelebi, Score } = await import("@material/material-color-utilities");
    const ranked = Score.score(QuantizerCelebi.quantize(Array.from(pixels), maxColors));
    return { argb: ranked[0] ?? null };
  } catch (err) {
    return { argb: null, error: err instanceof Error ? err.message : String(err) };
  }
};
