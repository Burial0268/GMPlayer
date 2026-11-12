/**
 * Owns the lyric-segmenter WASM so that downloading it and building jieba's dictionary
 * (~200 ms) never block the main thread. Answers batches of texts with the crate's packed
 * segment ends; `lyricSegmenter.ts` caches them for its synchronous `Intl.Segmenter`.
 */
import type { LyricSegmenter } from "@player-helper/lyric-segmenter";

// The project's lib is DOM, so `self` types as Window; this file runs as a dedicated worker.
const scope = self as unknown as Worker;

interface Request {
  id: number;
  texts: string[];
}

const ready: Promise<LyricSegmenter> = import("@player-helper/lyric-segmenter").then(
  ({ LyricSegmenter }) => {
    const started = performance.now();
    const segmenter = new LyricSegmenter();
    scope.postMessage({ ready: Math.round(performance.now() - started) });
    return segmenter;
  },
);
// Tell the page at once, so it stops asking instead of waiting out each request.
ready.catch((error) => scope.postMessage({ failed: String(error) }));

scope.onmessage = async ({ data }: MessageEvent<Request>) => {
  try {
    const segmenter = await ready;
    const results = data.texts.map((text) => segmenter.segments(text));
    scope.postMessage(
      { id: data.id, results },
      results.map((result) => result.buffer as ArrayBuffer),
    );
  } catch (error) {
    scope.postMessage({ id: data.id, error: String(error) });
  }
};
