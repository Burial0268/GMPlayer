/**
 * Puts the WASM segmenter (`src-tauri/crates/lyric-segmenter`: jieba for Chinese, ICU4X for
 * everything else) behind `Intl.Segmenter` for word granularity.
 *
 * It has to be the global rather than something passed in: AMLL builds its word segmenter as a
 * static field when its module loads, and never exports a way to swap it. That also fixes the
 * order — this module is the first import in `main.ts`, ahead of anything that imports AMLL.
 *
 * `segment()` is synchronous but the WASM lives in a worker (`lyricSegmenter.worker.ts`), so
 * that loading it and building jieba's dictionary never block the main thread. The two meet in
 * a cache: `prepareLyricSegments` has the worker cut a song's lines before they are laid out,
 * and `segment()` answers a cached text from it and anything else natively. AMLL only ever
 * segments a whole line's text, which is exactly what gets prepared. Grapheme and sentence
 * granularity always stay native. Without a native `Intl.Segmenter` nothing is installed;
 * AMLL and `lyricLineBreak` already handle its absence.
 */

const Native = typeof Intl !== "undefined" ? Intl.Segmenter : undefined;

/** Packed segment ends per text (UTF-16 end << 1 | word-like), in insertion order. */
const cache = new Map<string, Uint32Array>();
// A few songs' worth of lines; Map order makes the first key the oldest.
const CACHE_LIMIT = 4096;
const remember = (text: string, ends: Uint32Array) => {
  cache.set(text, ends);
  if (cache.size > CACHE_LIMIT) cache.delete(cache.keys().next().value as string);
};

function* decode(input: string, ends: Uint32Array): Generator<Intl.SegmentData> {
  let index = 0;
  for (const packed of ends) {
    const end = packed >>> 1;
    yield { segment: input.slice(index, end), index, input, isWordLike: (packed & 1) === 1 };
    index = end;
  }
}

function cachedSegments(input: string, ends: Uint32Array): Intl.Segments {
  return {
    [Symbol.iterator]: () => decode(input, ends),
    containing(codeUnitIndex = 0) {
      const at = Math.trunc(Number(codeUnitIndex)) || 0;
      if (at < 0 || at >= input.length) return undefined;
      for (const segment of decode(input, ends)) {
        if (at < segment.index + segment.segment.length) return segment;
      }
      return undefined;
    },
  };
}

let worker: Worker | null = null;
let failed = !Native;
let requestId = 0;
const pending = new Map<number, (results?: Uint32Array[]) => void>();

const retire = (reason: unknown) => {
  console.warn("[lyricSegmenter] worker unavailable, using native", reason);
  failed = true;
  worker?.terminate();
  worker = null;
  pending.forEach((settle) => settle());
  pending.clear();
};

function getWorker(): Worker | null {
  if (failed) return null;
  if (worker) return worker;
  try {
    worker = new Worker(new URL("./lyricSegmenter.worker.ts", import.meta.url), {
      type: "module",
    });
  } catch (error) {
    retire(error);
    return null;
  }
  worker.onmessage = ({ data }: MessageEvent) => {
    if ("ready" in data) console.info(`[lyricSegmenter] ready in ${data.ready} ms (worker)`);
    else if ("failed" in data) retire(data.failed);
    else {
      const settle = pending.get(data.id);
      pending.delete(data.id);
      settle?.(data.results);
    }
  };
  worker.onerror = (event) => retire(event.message);
  return worker;
}

let nativeWords: Intl.Segmenter | undefined;
/** Whether the native segmenter cuts `text` anywhere other than `ends` does. */
function nativeDiffers(text: string, ends: Uint32Array): boolean {
  nativeWords ??= Native && new Native(undefined, { granularity: "word" });
  if (!nativeWords) return true;
  let i = 0;
  for (const { index, segment, isWordLike } of nativeWords.segment(text)) {
    const packed = ends[i++];
    if (packed === undefined || packed >>> 1 !== index + segment.length) return true;
    if (((packed & 1) === 1) !== Boolean(isWordLike)) return true;
  }
  return i !== ends.length;
}

const settled = Promise.resolve();
const unchanged = Promise.resolve(false);

/**
 * Has the worker cut `texts` so `segment()` can answer them from the cache.
 *
 * `ready` resolves when the answer is in or after `waitMs`, whichever is first, so a cold worker
 * never holds lyrics back: those lines lay out natively. AMLL never re-breaks a line it has laid
 * out, so a late answer would otherwise never show. `late` resolves `true` once such an answer
 * has landed and cuts at least one text differently from the native segmenter, i.e. when the
 * caller has to hand its lines to AMLL again to get the better breaks.
 */
export function prepareLyricSegments(
  texts: Iterable<string>,
  waitMs = 300,
): { ready: Promise<void>; late: Promise<boolean> } {
  const missing = [...new Set(texts)].filter((text) => text.trim() && !cache.has(text));
  const target = missing.length ? getWorker() : null;
  if (!target) return { ready: settled, late: unchanged };
  const id = ++requestId;
  let timedOut = false;
  const answered = new Promise<boolean>((resolve) => {
    pending.set(id, (results) => {
      results?.forEach((ends, i) => remember(missing[i], ends));
      resolve(timedOut && Boolean(results?.some((ends, i) => nativeDiffers(missing[i], ends))));
    });
  });
  target.postMessage({ id, texts: missing });
  const timeout = new Promise<void>((resolve) =>
    setTimeout(() => {
      timedOut = true;
      resolve();
    }, waitMs),
  );
  return { ready: Promise.race([answered.then(() => undefined), timeout]), late: answered };
}

if (Native) {
  class WordSegmenter extends Native {
    readonly #word: boolean;

    constructor(locales?: string | string[], options?: Intl.SegmenterOptions) {
      super(locales, options);
      this.#word = this.resolvedOptions().granularity === "word";
    }

    segment(input: string): Intl.Segments {
      const text = String(input);
      const ends = this.#word ? cache.get(text) : undefined;
      return ends ? cachedSegments(text, ends) : super.segment(text);
    }
  }

  Object.defineProperty(Intl, "Segmenter", {
    value: WordSegmenter,
    writable: true,
    configurable: true,
  });

  // Start the worker loading once the app is up, so the first song finds it warm.
  if (typeof requestIdleCallback === "function") {
    requestIdleCallback(() => getWorker(), { timeout: 3000 });
  } else setTimeout(getWorker, 1000);
}
