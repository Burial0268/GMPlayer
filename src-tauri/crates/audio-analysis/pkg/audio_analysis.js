/* @ts-self-types="./audio_analysis.d.ts" */
import * as wasm from "./audio_analysis_bg.wasm";
import { __wbg_set_wasm } from "./audio_analysis_bg.js";

__wbg_set_wasm(wasm);

export {
    LFOptionsJs, WasmAudioProcessor
} from "./audio_analysis_bg.js";
