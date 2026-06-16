/* @ts-self-types="./gmplayer_audio_backend.d.ts" */
import * as wasm from "./gmplayer_audio_backend_bg.wasm";
import { __wbg_set_wasm } from "./gmplayer_audio_backend_bg.js";

__wbg_set_wasm(wasm);

export {
    LFOptionsJs, WasmAudioBackend, WasmAudioProcessor
} from "./gmplayer_audio_backend_bg.js";
