/**
 * PCM Capture AudioWorklet - Inline blob approach
 *
 * Captures mono PCM (Float32Array, 128 samples/block) from audio graph
 * and sends to main thread via MessagePort for WASM FFTPlayer consumption.
 */

const WORKLET_CODE = `
class PCMCaptureProcessor extends AudioWorkletProcessor {
  process(inputs) {
    const input = inputs[0];
    if (input && input[0] && input[0].length > 0) {
      this.port.postMessage(input[0]);
    }
    return true;
  }
}
registerProcessor('pcm-capture-processor', PCMCaptureProcessor);
`;

let registered = false;

/**
 * Register the PCM capture AudioWorklet processor.
 * Uses inline blob URL to avoid Vite file-loading issues with AudioWorklet modules.
 */
export async function registerPCMCaptureWorklet(ctx: AudioContext): Promise<void> {
  if (registered) return;
  const blob = new Blob([WORKLET_CODE], { type: 'application/javascript' });
  const url = URL.createObjectURL(blob);
  try {
    await ctx.audioWorklet.addModule(url);
    registered = true;
  } finally {
    URL.revokeObjectURL(url);
  }
}

/**
 * Check if the PCM capture worklet has been registered.
 */
export function isPCMWorkletRegistered(): boolean {
  return registered;
}
