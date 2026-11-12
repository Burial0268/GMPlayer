/**
 * Puts the WASM segmenter (`src-tauri/crates/lyric-segmenter`: jieba for Chinese, ICU4X for
 * everything else) behind `Intl.Segmenter` for word granularity.
 *
 * It has to be the global rather than something passed in: AMLL builds its word segmenter as a
 * static field when its module loads, and never exports a way to swap it. That also fixes the
 * order — this module is the first import in `main.ts`, ahead of anything that imports AMLL —
 * and the shape: `segment()` is synchronous and the WASM arrives later, so until it has loaded
 * (or if it never does) an instance answers with the native implementation. Grapheme and
 * sentence granularity always stay native. Without a native `Intl.Segmenter` nothing is
 * installed; AMLL and `lyricLineBreak` already handle its absence.
 */
import type { LyricSegmenter } from "@player-helper/lyric-segmenter";

type Segment = Intl.SegmentData;

const Native = typeof Intl !== "undefined" ? Intl.Segmenter : undefined;
let wasm: LyricSegmenter | undefined;

/** Decodes the crate's packed segment ends (UTF-16 end << 1 | word-like). */
function* decode(input: string, ends: Uint32Array): Generator<Segment> {
  let index = 0;
  for (const packed of ends) {
    const end = packed >>> 1;
    yield { segment: input.slice(index, end), index, input, isWordLike: (packed & 1) === 1 };
    index = end;
  }
}

function wasmSegments(input: string, segmenter: LyricSegmenter): Intl.Segments {
  const ends = segmenter.segments(input);
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

if (Native) {
  class WordSegmenter extends Native {
    readonly #word: boolean;

    constructor(locales?: string | string[], options?: Intl.SegmenterOptions) {
      super(locales, options);
      this.#word = this.resolvedOptions().granularity === "word";
    }

    segment(input: string): Intl.Segments {
      return this.#word && wasm ? wasmSegments(String(input), wasm) : super.segment(input);
    }
  }

  Object.defineProperty(Intl, "Segmenter", {
    value: WordSegmenter,
    writable: true,
    configurable: true,
  });

  // A few MB of dictionaries: fetch once the app is up instead of competing with first paint.
  const load = () =>
    import("@player-helper/lyric-segmenter")
      .then(({ LyricSegmenter }) => {
        const started = performance.now();
        wasm = new LyricSegmenter();
        console.info(`[lyricSegmenter] ready in ${Math.round(performance.now() - started)} ms`);
      })
      .catch((error) => console.warn("[lyricSegmenter] WASM unavailable, using native", error));
  if (typeof requestIdleCallback === "function") requestIdleCallback(load, { timeout: 3000 });
  else setTimeout(load, 1000);
}
