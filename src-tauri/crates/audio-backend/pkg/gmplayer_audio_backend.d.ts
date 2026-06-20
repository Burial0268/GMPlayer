/* tslint:disable */
/* eslint-disable */

export class DecodedAudioJs {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    channels(): number;
    duration(): number;
    sampleRate(): number;
    samples(): Float32Array;
}

/**
 * Return type for `getLFOptions` (plain data object visible in JS).
 */
export class LFOptionsJs {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    bin_count: number;
    gradient_threshold: number;
    smoothing_factor: number;
    window_size: number;
}

/**
 * WASM-side state holder for the browser IPC runtime.
 *
 * It deliberately does not touch sockets, threads, files, CPAL, or Tauri.
 * Browser code calls `sendMessageJson`, applies returned effects to the Web
 * output host, and feeds media callbacks back via the `apply*` methods so
 * events stay in the same shape as the native backend.
 */
export class WasmAudioBackend {
    free(): void;
    [Symbol.dispose](): void;
    applyLoadError(error: string): string;
    /**
     * Feed browser metadata once the HTML media element has loaded.
     */
    applyLoadedTrack(duration: number, sample_rate: number, channels: number, bitrate: number): string;
    applyPlayError(error: string): string;
    applyPlayPosition(position: number): string;
    applyPlaybackFinished(): string;
    applyPlaybackState(is_playing: boolean): string;
    applyVolume(volume: number): string;
    /**
     * Decode browser-fetched audio bytes into mono PCM for offline AutoMix analysis.
     *
     * This is intentionally state-independent: AutoMix analysis must not mutate
     * the playback backend's playlist/current-track state.
     */
    decodeAudioBytes(bytes: Uint8Array, extension: string): DecodedAudioJs;
    /**
     * Decode browser-fetched audio bytes for the WASM analysis path.
     *
     * Playback remains owned by the browser media host; this sidechain feeds
     * the same Rust `audio-analysis` processor used by native playback.
     */
    loadAnalysisBytes(bytes: Uint8Array, extension: string, music_id: string): string;
    constructor();
    processAnalysisFrame(position: number, delta_ms: number): string;
    /**
     * Process an `AudioThreadEventMessage<AudioThreadMessage>` JSON envelope.
     *
     * Returns `{ events, effects }` as JSON. Parse failures are returned as a
     * `loadError` event instead of panicking across the WASM boundary.
     */
    sendMessageJson(envelope_json: string): string;
    stateJson(): string;
    syncStatusJson(): string;
}

export class WasmAudioProcessor {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Clear PCM queue and reset all state (e.g. on seek).
     */
    clear(): void;
    /**
     * Release all heap allocations.
     */
    free(): void;
    /**
     * Get current low-frequency analyzer configuration.
     */
    getLFOptions(): LFOptionsJs;
    /**
     * Get the cached low-frequency volume (0-1) from the last `processFrame` call.
     */
    getLowFreq(): number;
    /**
     * Get raw FFT magnitudes aggregated into `count` groups (128-group AMLL resolution).
     * Returns a new Float32Array of aggregated raw magnitudes, or empty if not available.
     */
    getRawBins(count: number): Float32Array;
    /**
     * Get the cached normalized spectrum (0-255) from the last `processFrame` call.
     */
    getSpectrum(): Float32Array;
    /**
     * Check if the processor is ready (always true — no WASM init failure possible).
     */
    isReady(): boolean;
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
     */
    constructor(output_size: number, freq_min: number, freq_max: number, bin_count: number, window_size: number, gradient_threshold: number, smoothing_factor: number);
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
     */
    processFrame(delta_ms: number, spectrum: Float32Array): number;
    /**
     * Push mono PCM samples from AudioWorklet.
     * Called ~86 times/sec (every ~512 samples at 44.1kHz).
     *
     * `sample_rate`: AudioContext sample rate (e.g. 44100, 48000)
     * `samples`: Float32Array of mono PCM
     */
    pushPCM(sample_rate: number, samples: Float32Array): void;
    /**
     * Reset normalization, smoothing, and gradient state (e.g. on track change).
     * Does NOT clear the PCM queue.
     */
    reset(): void;
    /**
     * Update frequency range for FFT spectrum output at runtime.
     */
    setFreqRange(min: number, max: number): void;
    /**
     * Update low-frequency analyzer options at runtime.
     * All parameters are optional — pass undefined/null to keep current value.
     */
    setLFOptions(bin_count?: number | null, window_size?: number | null, gradient_threshold?: number | null, smoothing_factor?: number | null): void;
}
