import type { TrackAnalysis } from "./TrackAnalyzer";

const IS_DEV = import.meta.env?.DEV ?? false;

let worker: Worker | null = null;
let workerFailed = false;
let requestId = 0;

interface PendingRequest {
  resolve: (result: TrackAnalysis) => void;
  reject: (err: Error) => void;
}

const pendingRequests = new Map<number, PendingRequest>();

function getWorker(): Worker | null {
  if (workerFailed) return null;
  if (worker) return worker;

  try {
    worker = new Worker(new URL("./analysis-worker.ts", import.meta.url), { type: "module" });

    worker.onmessage = (e: MessageEvent) => {
      const { type, id } = e.data;
      const pending = pendingRequests.get(id);
      if (!pending) return;
      pendingRequests.delete(id);

      if (type === "result") {
        pending.resolve(e.data as TrackAnalysis);
      } else if (type === "error") {
        pending.reject(new Error(e.data.error));
      }
    };

    worker.onerror = (err) => {
      console.warn("TrackAnalyzer: Worker error", err);
      for (const [id, pending] of pendingRequests) {
        pending.reject(new Error("Worker error"));
        pendingRequests.delete(id);
      }
    };

    if (IS_DEV) {
      console.log("TrackAnalyzer: Web Worker initialized");
    }

    return worker;
  } catch (err) {
    console.warn("TrackAnalyzer: Failed to create Worker, will use main-thread fallback", err);
    workerFailed = true;
    return null;
  }
}

export function hasAnalysisWorker(): boolean {
  return getWorker() !== null;
}

export function analyzePcmViaWorker(
  monoData: Float32Array,
  sampleRate: number,
  duration: number,
  analyzeBPM: boolean,
): Promise<TrackAnalysis> {
  const w = getWorker();
  if (!w) return Promise.reject(new Error("Analysis Worker is unavailable"));

  const id = ++requestId;

  return new Promise<TrackAnalysis>((resolve, reject) => {
    const timeout = setTimeout(() => {
      pendingRequests.delete(id);
      reject(new Error("Worker analysis timed out"));
    }, 30000);

    pendingRequests.set(id, {
      resolve: (result) => {
        clearTimeout(timeout);
        if (IS_DEV) {
          console.log("TrackAnalyzer: Worker analysis complete", {
            duration: duration.toFixed(1) + "s",
            rms: result.volume.rms.toFixed(4),
            lufs: result.volume.estimatedLUFS.toFixed(1),
            bpm: !analyzeBPM
              ? "skipped"
              : (result.bpm?.bpm ?? "null (duration=" + duration.toFixed(1) + "s)"),
            bpmConfidence: result.bpm?.confidence?.toFixed(2) ?? "n/a",
            outroType: result.outro?.outroType ?? "n/a",
            outroConfidence: result.outro?.outroConfidence?.toFixed(2) ?? "n/a",
            suggestedCrossfadeStart: result.outro?.suggestedCrossfadeStart?.toFixed(1) ?? "n/a",
          });
        }
        resolve(result);
      },
      reject: (err) => {
        clearTimeout(timeout);
        reject(err);
      },
    });

    w.postMessage({ type: "analyze", id, monoData, sampleRate, duration, analyzeBPM }, [
      monoData.buffer,
    ]);
  });
}

export function analyzeBytesViaWorker(
  bytes: Uint8Array,
  extension: string,
  analyzeBPM: boolean,
): Promise<TrackAnalysis> {
  const w = getWorker();
  if (!w) return Promise.reject(new Error("Analysis Worker is unavailable"));

  const id = ++requestId;

  return new Promise<TrackAnalysis>((resolve, reject) => {
    const timeout = setTimeout(() => {
      pendingRequests.delete(id);
      reject(new Error("WASM decode+analysis timed out"));
    }, 60000);

    pendingRequests.set(id, {
      resolve: (result) => {
        clearTimeout(timeout);
        resolve(result);
      },
      reject: (err) => {
        clearTimeout(timeout);
        reject(err);
      },
    });

    const transferable =
      bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength
        ? bytes.buffer
        : bytes.slice().buffer;
    const transferBytes = new Uint8Array(transferable);
    w.postMessage({ type: "decodeAndAnalyze", id, bytes: transferBytes, extension, analyzeBPM }, [
      transferable,
    ]);
  });
}

export function terminateAnalysisWorker(): void {
  if (worker) {
    worker.terminate();
    worker = null;
  }
  pendingRequests.clear();
}
