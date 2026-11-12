/* @ts-self-types="./lyric_segmenter.d.ts" */
import * as wasm from "./lyric_segmenter_bg.wasm";
import { __wbg_set_wasm } from "./lyric_segmenter_bg.js";

__wbg_set_wasm(wasm);

export {
    LyricSegmenter
} from "./lyric_segmenter_bg.js";
