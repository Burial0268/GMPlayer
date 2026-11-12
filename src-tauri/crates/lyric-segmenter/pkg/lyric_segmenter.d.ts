/* tslint:disable */
/* eslint-disable */

export class LyricSegmenter {
    free(): void;
    [Symbol.dispose](): void;
    constructor();
    /**
     * One entry per segment: its end as a JS string index, shifted left by one, with bit 0
     * set when the segment is word-like.
     */
    segments(text: string): Uint32Array;
}
