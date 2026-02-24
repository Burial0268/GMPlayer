/**
 * PCM Capture AudioWorklet - Inline blob approach
 *
 * Captures mono PCM (Float32Array, 128 samples/block) from audio graph
 * and sends to main thread via MessagePort for WASM FFTPlayer consumption.
 *
 * Buffer: accumulates 512 samples (~11.6ms at 44100Hz) before posting,
 * using zero-copy transfer to minimize audio-thread overhead.
 */

const WORKLET_CODE = `
class PCMCaptureProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this._buffer = new Float32Array(512);
    this._offset = 0;
  }
  process(inputs) {
    const input = inputs[0];
    if (!input || !input[0] || input[0].length === 0) return true;

    const data = input[0];
    let pos = 0;

    while (pos < data.length) {
      const space = 512 - this._offset;
      const count = Math.min(space, data.length - pos);
      this._buffer.set(data.subarray(pos, pos + count), this._offset);
      this._offset += count;
      pos += count;

      if (this._offset >= 512) {
        this.port.postMessage(this._buffer, [this._buffer.buffer]);
        this._buffer = new Float32Array(512);
        this._offset = 0;
      }
    }

    return true;
  }
}
registerProcessor('pcm-capture-processor', PCMCaptureProcessor);
`;

// Track which AudioContext the worklet was registered with.
// A simple boolean flag gets stale when the AudioContext is recreated
// (e.g., after close, HMR, or system audio device change).
let registeredCtx: AudioContext | null = null;

/**
 * Register the PCM capture AudioWorklet processor.
 * Uses inline blob URL to avoid Vite file-loading issues with AudioWorklet modules.
 * Tracks the AudioContext to detect stale registration.
 */
export async function registerPCMCaptureWorklet(ctx: AudioContext): Promise<void> {
  // Already registered with this exact context
  if (registeredCtx === ctx) return;

  const blob = new Blob([WORKLET_CODE], { type: "application/javascript" });
  const url = URL.createObjectURL(blob);
  try {
    await ctx.audioWorklet.addModule(url);
  } catch (err) {
    // Processor name already registered in this context (e.g., HMR re-evaluation
    // resets the module-level registeredCtx while the AudioContext persists).
    // The processor IS available — just can't be double-registered.
    if (err instanceof DOMException && err.name === "NotSupportedError") {
      // fall through to set registeredCtx
    } else {
      throw err;
    }
  } finally {
    URL.revokeObjectURL(url);
  }
  registeredCtx = ctx;
}

/**
 * Check if the PCM capture worklet has been registered for a specific AudioContext.
 * @param ctx The AudioContext to check against
 */
export function isPCMWorkletRegisteredFor(ctx: AudioContext): boolean {
  return registeredCtx === ctx;
}

/**
 * Reset worklet registration state.
 * Called when the AudioContext is recreated.
 */
export function resetPCMWorkletRegistration(): void {
  registeredCtx = null;
}
