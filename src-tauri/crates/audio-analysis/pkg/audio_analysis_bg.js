/**
 * Return type for `getLFOptions` (plain data object visible in JS).
 */
export class LFOptionsJs {
    static __wrap(ptr) {
        const obj = Object.create(LFOptionsJs.prototype);
        obj.__wbg_ptr = ptr;
        LFOptionsJsFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        LFOptionsJsFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_lfoptionsjs_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    get bin_count() {
        const ret = wasm.__wbg_get_lfoptionsjs_bin_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    get gradient_threshold() {
        const ret = wasm.__wbg_get_lfoptionsjs_gradient_threshold(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    get smoothing_factor() {
        const ret = wasm.__wbg_get_lfoptionsjs_smoothing_factor(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    get window_size() {
        const ret = wasm.__wbg_get_lfoptionsjs_window_size(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} arg0
     */
    set bin_count(arg0) {
        wasm.__wbg_set_lfoptionsjs_bin_count(this.__wbg_ptr, arg0);
    }
    /**
     * @param {number} arg0
     */
    set gradient_threshold(arg0) {
        wasm.__wbg_set_lfoptionsjs_gradient_threshold(this.__wbg_ptr, arg0);
    }
    /**
     * @param {number} arg0
     */
    set smoothing_factor(arg0) {
        wasm.__wbg_set_lfoptionsjs_smoothing_factor(this.__wbg_ptr, arg0);
    }
    /**
     * @param {number} arg0
     */
    set window_size(arg0) {
        wasm.__wbg_set_lfoptionsjs_window_size(this.__wbg_ptr, arg0);
    }
}
if (Symbol.dispose) LFOptionsJs.prototype[Symbol.dispose] = LFOptionsJs.prototype.free;

export class WasmAudioProcessor {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        WasmAudioProcessorFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_wasmaudioprocessor_free(ptr, 0);
    }
    /**
     * Clear PCM queue and reset all state (e.g. on seek).
     */
    clear() {
        wasm.wasmaudioprocessor_clear(this.__wbg_ptr);
    }
    /**
     * Release all heap allocations.
     */
    free() {
        wasm.wasmaudioprocessor_free(this.__wbg_ptr);
    }
    /**
     * Get current low-frequency analyzer configuration.
     * @returns {LFOptionsJs}
     */
    getLFOptions() {
        const ret = wasm.wasmaudioprocessor_getLFOptions(this.__wbg_ptr);
        return LFOptionsJs.__wrap(ret);
    }
    /**
     * Get the cached low-frequency volume (0-1) from the last `processFrame` call.
     * @returns {number}
     */
    getLowFreq() {
        const ret = wasm.wasmaudioprocessor_getLowFreq(this.__wbg_ptr);
        return ret;
    }
    /**
     * Get raw FFT magnitudes aggregated into `count` groups (128-group AMLL resolution).
     * Returns a new Float32Array of aggregated raw magnitudes, or empty if not available.
     * @param {number} count
     * @returns {Float32Array}
     */
    getRawBins(count) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.wasmaudioprocessor_getRawBins(retptr, this.__wbg_ptr, count);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayF32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Get the cached normalized spectrum (0-255) from the last `processFrame` call.
     * @returns {Float32Array}
     */
    getSpectrum() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.wasmaudioprocessor_getSpectrum(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayF32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Check if the processor is ready (always true — no WASM init failure possible).
     * @returns {boolean}
     */
    isReady() {
        const ret = wasm.wasmaudioprocessor_isReady(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Create a new unified audio processor.
     *
     * Parameters match TypeScript defaults:
     * - `output_size`: spectrum output size (default 2048 desktop, 1024 mobile)
     * - `freq_min`: min frequency in Hz (default 80)
     * - `freq_max`: max frequency in Hz (default 2000)
     * - `bin_count`: raw FFT bins for lowFreq analysis (default 2)
     * - `window_size`: gradient sliding window size (default 10)
     * - `gradient_threshold`: gradient trigger threshold (default 0.35 desktop, 0.1 mobile)
     * - `smoothing_factor`: time-delta smoothing speed (default 0.003)
     * @param {number} output_size
     * @param {number} freq_min
     * @param {number} freq_max
     * @param {number} bin_count
     * @param {number} window_size
     * @param {number} gradient_threshold
     * @param {number} smoothing_factor
     */
    constructor(output_size, freq_min, freq_max, bin_count, window_size, gradient_threshold, smoothing_factor) {
        const ret = wasm.wasmaudioprocessor_new(output_size, freq_min, freq_max, bin_count, window_size, gradient_threshold, smoothing_factor);
        this.__wbg_ptr = ret;
        WasmAudioProcessorFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Single WASM call per RAF frame (~60 fps).
     *
     * Runs FFT (if ≥2048 PCM queued), normalizes to 0-255, computes raw bins,
     * runs gradient + smoothing. Fills `spectrum` (Float32Array) with normalized
     * spectrum values (0-255).
     *
     * `delta_ms`: milliseconds since last frame (from `performance.now()` diff).
     *
     * Returns smoothed low-frequency volume (0-1 range).
     * @param {number} delta_ms
     * @param {Float32Array} spectrum
     * @returns {number}
     */
    processFrame(delta_ms, spectrum) {
        var ptr0 = passArrayF32ToWasm0(spectrum, wasm.__wbindgen_export2);
        var len0 = WASM_VECTOR_LEN;
        const ret = wasm.wasmaudioprocessor_processFrame(this.__wbg_ptr, delta_ms, ptr0, len0, addHeapObject(spectrum));
        return ret;
    }
    /**
     * Push mono PCM samples from AudioWorklet.
     * Called ~86 times/sec (every ~512 samples at 44.1kHz).
     *
     * `sample_rate`: AudioContext sample rate (e.g. 44100, 48000)
     * `samples`: Float32Array of mono PCM
     * @param {number} sample_rate
     * @param {Float32Array} samples
     */
    pushPCM(sample_rate, samples) {
        const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.wasmaudioprocessor_pushPCM(this.__wbg_ptr, sample_rate, ptr0, len0);
    }
    /**
     * Reset normalization, smoothing, and gradient state (e.g. on track change).
     * Does NOT clear the PCM queue.
     */
    reset() {
        wasm.wasmaudioprocessor_reset(this.__wbg_ptr);
    }
    /**
     * Update frequency range for FFT spectrum output at runtime.
     * @param {number} min
     * @param {number} max
     */
    setFreqRange(min, max) {
        wasm.wasmaudioprocessor_setFreqRange(this.__wbg_ptr, min, max);
    }
    /**
     * Update low-frequency analyzer options at runtime.
     * All parameters are optional — pass undefined/null to keep current value.
     * @param {number | null} [bin_count]
     * @param {number | null} [window_size]
     * @param {number | null} [gradient_threshold]
     * @param {number | null} [smoothing_factor]
     */
    setLFOptions(bin_count, window_size, gradient_threshold, smoothing_factor) {
        wasm.wasmaudioprocessor_setLFOptions(this.__wbg_ptr, isLikeNone(bin_count) ? Number.MAX_SAFE_INTEGER : (bin_count) >>> 0, isLikeNone(window_size) ? Number.MAX_SAFE_INTEGER : (window_size) >>> 0, isLikeNone(gradient_threshold) ? Number.MAX_SAFE_INTEGER : Math.fround(gradient_threshold), isLikeNone(smoothing_factor) ? Number.MAX_SAFE_INTEGER : Math.fround(smoothing_factor));
    }
}
if (Symbol.dispose) WasmAudioProcessor.prototype[Symbol.dispose] = WasmAudioProcessor.prototype.free;
export function __wbg___wbindgen_copy_to_typed_array_c5728021fabd0236(arg0, arg1, arg2) {
    new Uint8Array(getObject(arg2).buffer, getObject(arg2).byteOffset, getObject(arg2).byteLength).set(getArrayU8FromWasm0(arg0, arg1));
}
export function __wbg___wbindgen_throw_ea4887a5f8f9a9db(arg0, arg1) {
    throw new Error(getStringFromWasm0(arg0, arg1));
}
export function __wbindgen_object_drop_ref(arg0) {
    takeObject(arg0);
}
const LFOptionsJsFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_lfoptionsjs_free(ptr, 1));
const WasmAudioProcessorFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_wasmaudioprocessor_free(ptr, 1));

function addHeapObject(obj) {
    if (heap_next === heap.length) heap.push(heap.length + 1);
    const idx = heap_next;
    heap_next = heap[idx];

    heap[idx] = obj;
    return idx;
}

function dropObject(idx) {
    if (idx < 1028) return;
    heap[idx] = heap_next;
    heap_next = idx;
}

function getArrayF32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getFloat32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

let cachedFloat32ArrayMemory0 = null;
function getFloat32ArrayMemory0() {
    if (cachedFloat32ArrayMemory0 === null || cachedFloat32ArrayMemory0.byteLength === 0) {
        cachedFloat32ArrayMemory0 = new Float32Array(wasm.memory.buffer);
    }
    return cachedFloat32ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function getObject(idx) { return heap[idx]; }

let heap = new Array(1024).fill(undefined);
heap.push(undefined, null, true, false);

let heap_next = heap.length;

function isLikeNone(x) {
    return x === undefined || x === null;
}

function passArrayF32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getFloat32ArrayMemory0().set(arg, ptr / 4);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function takeObject(idx) {
    const ret = getObject(idx);
    dropObject(idx);
    return ret;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

let WASM_VECTOR_LEN = 0;


let wasm;
export function __wbg_set_wasm(val) {
    wasm = val;
}
