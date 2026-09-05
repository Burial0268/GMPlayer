/**
 * Cover-palette quantization, off the main thread.
 *
 * `QuantizerCelebi.quantize` is the single most expensive synchronous step in
 * the whole cover-art pipeline, and it is **not** proportional to the pixel
 * count: the cost is in Wu's box-splitting down to `maxColors`, so shrinking the
 * sample grid barely helps (64×64 → 32×32 only went 51 ms → 33 ms) while halving
 * `maxColors` would change the colour that comes out. Measured on the 64×64 grid
 * `coverPalette.ts` samples: 30 ms for flat artwork, ~78 ms for a photo-like
 * cover, 141 ms for high-entropy input.
 *
 * That block used to land on whichever frame the image happened to decode in —
 * which on a playlist page is the frame that also renders the first rows. It is
 * pure arithmetic over a detached pixel snapshot with no DOM access at all, so a
 * worker is the whole fix rather than a rearrangement.
 *
 * Deliberately only the quantizer lives here. `Score`, `Hct`,
 * `themeFromSourceColor` and ColorThief together are ~5 ms and several of them
 * read the settings store, so keeping them on the main thread keeps this worker
 * a pure function — and keeps the *quantizer* out of the main bundle, which
 * nothing else imports.
 */

import { QuantizerCelebi, Score } from "@material/material-color-utilities";

interface QuantizeRequest {
  id: number;
  /** Packed `0xAARRGGBB`, opaque pixels only. */
  pixels: Uint32Array;
  maxColors: number;
}

interface QuantizeResponse {
  id: number;
  /** Highest-scored source colour, or `null` when there was nothing to score. */
  argb: number | null;
  error?: string;
}

self.onmessage = (e: MessageEvent<QuantizeRequest>) => {
  const { id, pixels, maxColors } = e.data;
  const post = (response: QuantizeResponse) => {
    (self as unknown as Worker).postMessage(response);
  };
  try {
    if (!pixels.length) {
      post({ id, argb: null });
      return;
    }
    // `quantize` is typed for `number[]`; hand it a real array rather than the
    // typed view so nothing downstream depends on a `Uint32Array` behaving like
    // one. 4096 elements, so the copy does not register.
    const quantized = QuantizerCelebi.quantize(Array.from(pixels), maxColors);
    const ranked = Score.score(quantized);
    post({ id, argb: ranked[0] ?? null });
  } catch (err) {
    post({ id, argb: null, error: err instanceof Error ? err.message : String(err) });
  }
};
