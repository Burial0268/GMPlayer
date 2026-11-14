/**
 * BufferedSound - Full audio buffering via Blob URL + NativeSound
 *
 * Architecture:
 * 1. Downloads complete audio via fetch() with progress tracking
 * 2. Creates a Blob URL from the downloaded data (fully in-memory)
 * 3. Delegates to NativeSound with the Blob URL as source
 *
 * This avoids AudioBufferSourceNode (one-shot, async race conditions) entirely.
 * HTMLAudioElement with a Blob URL gives us:
 * - Native play/pause/seek without recreating nodes
 * - Stable Web Audio graph (MediaElementAudioSourceNode persists)
 * - No network dependency after initial download (tab switch safe)
 */

import { NativeSound } from "./NativeSound";
import { NEUTRAL_LOW_FREQ_VOLUME } from "../analysis/AudioEffectManager";
import { SOUND_EVENTS, type SoundOptions, type ISound } from "./types";
import { SoundEvents } from "./SoundEvents";
import type { AudioEffectManager } from "../analysis/AudioEffectManager";

// Development mode detection
const IS_DEV = import.meta.env?.DEV ?? false;
const EMPTY_U8 = new Uint8Array(0);

/**
 * BufferedSound - Downloads full audio, then delegates to NativeSound via Blob URL
 */
export class BufferedSound extends SoundEvents implements ISound {
  readonly kind = "element";

  // Internal NativeSound (created after download completes)
  private _inner: NativeSound | null = null;

  // Source URL and options
  private _src: string;
  private _volume: number;
  private _preload: boolean;

  // Blob URL (revoked on unload)
  private _blobUrl: string | null = null;

  // Download state
  private _downloadProgress: number = 0;
  private _abortController: AbortController | null = null;
  private _unloading: boolean = false;
  private _loaded: boolean = false;

  // Pending operations before inner NativeSound is ready
  private _pendingPlay: boolean = false;
  private _pendingSeek: number | null = null;

  constructor({ src, volume = 1, preload = true }: SoundOptions) {
    super();
    this._src = Array.isArray(src) ? src[0] : src;
    this._volume = volume;
    this._preload = preload;

    if (IS_DEV) {
      console.log("BufferedSound created:", this._src);
    }

    if (preload) {
      this._startDownload();
    }
  }

  // ── Download ──────────────────────────────────────────

  private async _startDownload(): Promise<void> {
    if (this._unloading) return;

    try {
      this._abortController = new AbortController();

      if (IS_DEV) {
        console.log("BufferedSound: Starting download:", this._src);
      }

      const response = await fetch(this._src, {
        signal: this._abortController.signal,
        credentials: "omit",
      });

      if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`);
      }

      const contentType = response.headers.get("content-type") || "audio/mpeg";
      const contentLength = response.headers.get("content-length");
      const total = contentLength ? parseInt(contentLength, 10) : 0;

      let arrayBuffer: ArrayBuffer;

      if (!response.body || total === 0) {
        // Fallback: no streaming, just read all at once
        arrayBuffer = await response.arrayBuffer();
        this._downloadProgress = 1;
        this._emit("progress", 1);
      } else {
        // Stream download with progress tracking
        const reader = response.body.getReader();
        let received = 0;

        // Fast-path: pre-allocate and fill when content-length is known.
        // Falls back to chunk aggregation if the server sends more than declared.
        let directView: Uint8Array | null = total > 0 ? new Uint8Array(total) : null;
        let chunks: Uint8Array[] | null = directView ? null : [];

        while (true) {
          const { done, value } = await reader.read();
          if (done) break;

          if (directView) {
            // Guard against incorrect content-length
            if (received + value.length > directView.length) {
              chunks = [directView.subarray(0, received), value];
              directView = null;
            } else {
              directView.set(value, received);
            }
          } else {
            chunks!.push(value);
          }

          received += value.length;

          // total is non-zero in this branch; keep the same progress semantics.
          this._downloadProgress = received / total;
          this._emit("progress", this._downloadProgress);
        }

        if (directView) {
          // If the server ended early, slice down to actual received bytes
          arrayBuffer =
            received === directView.length
              ? (directView.buffer as ArrayBuffer)
              : (directView.slice(0, received).buffer as ArrayBuffer);
        } else {
          // Combine chunks
          arrayBuffer = new ArrayBuffer(received);
          const view = new Uint8Array(arrayBuffer);
          let offset = 0;
          for (const chunk of chunks!) {
            view.set(chunk, offset);
            offset += chunk.length;
          }
        }
      }

      if (this._unloading) return;

      // Create Blob URL
      const blob = new Blob([arrayBuffer], { type: contentType });
      arrayBuffer = new ArrayBuffer(0);
      this._blobUrl = URL.createObjectURL(blob);

      if (IS_DEV) {
        console.log("BufferedSound: Download complete, Blob URL created");
      }

      // Create NativeSound with Blob URL (fully in-memory, no network needed)
      this._createInnerSound();
    } catch (err: any) {
      if (err.name === "AbortError") {
        if (IS_DEV) {
          console.log("BufferedSound: Download aborted");
        }
        return;
      }

      console.error("BufferedSound: Failed to download audio:", err);
      this._emit("loaderror");
    }
  }

  // ── Inner NativeSound Setup ───────────────────────────

  private _createInnerSound(): void {
    if (!this._blobUrl || this._unloading) return;

    this._inner = new NativeSound({
      src: [this._blobUrl],
      volume: this._volume,
      preload: true,
    });

    // Listeners live on this object for its whole life; the inner sound only feeds them.
    for (const event of SOUND_EVENTS) {
      this._inner.on(event, (...args: []) => this._emit(event, ...args));
    }
    this._loaded = true;

    // Execute pending seek (must happen before play)
    if (this._pendingSeek !== null) {
      this._inner.seekTo(this._pendingSeek);
      this._pendingSeek = null;
    }

    // Execute pending play
    if (this._pendingPlay) {
      this._pendingPlay = false;
      this._inner.play();
    }
  }

  // ── ISound Implementation (delegates to inner NativeSound) ──

  playing(): boolean {
    return this._inner?.playing() ?? false;
  }

  play(): this {
    if (this._inner) {
      this._inner.play();
    } else {
      this._pendingPlay = true;
      if (!this._abortController && !this._unloading) {
        this._startDownload();
      }
    }
    return this;
  }

  pause(): this {
    if (this._inner) {
      this._inner.pause();
    }
    this._pendingPlay = false;
    return this;
  }

  stop(): this {
    if (this._inner) {
      this._inner.stop();
    }
    this._pendingPlay = false;
    this._pendingSeek = null;
    return this;
  }

  get position(): number {
    return this._inner?.position ?? this._pendingSeek ?? 0;
  }

  seekTo(pos: number): this {
    if (this._inner) this._inner.seekTo(pos);
    else this._pendingSeek = pos;
    return this;
  }

  duration(): number {
    return this._inner?.duration() ?? 0;
  }

  get volume(): number {
    return this._inner?.volume ?? this._volume;
  }

  setVolume(vol: number): this {
    if (this._inner) this._inner.setVolume(vol);
    else this._volume = Math.max(0, Math.min(1, vol));
    return this;
  }

  fade(from: number, to: number, duration: number): this {
    if (this._inner) {
      this._inner.fade(from, to, duration);
    }
    return this;
  }

  getFrequencyData(): Uint8Array<ArrayBuffer> {
    return this._inner?.getFrequencyData() ?? (EMPTY_U8 as Uint8Array<ArrayBuffer>);
  }

  getFFTData(): ArrayLike<number> {
    return this._inner?.getFFTData() ?? [];
  }

  getLowFrequencyVolume(): number {
    return this._inner?.getLowFrequencyVolume() ?? NEUTRAL_LOW_FREQ_VOLUME;
  }

  getAverageAmplitude(): number {
    return this._inner?.getAverageAmplitude() ?? 0;
  }

  getEffectManager(): AudioEffectManager | null {
    return this._inner?.getEffectManager() ?? null;
  }

  getGainNode(): GainNode | null {
    return this._inner?.getGainNode() ?? null;
  }

  /**
   * Get download progress (0-1)
   */
  getDownloadProgress(): number {
    return this._downloadProgress;
  }

  /**
   * Get the Blob URL (if downloaded), or null.
   * Used by AutoMixEngine for offline analysis.
   */
  getBlobUrl(): string | null {
    return this._blobUrl;
  }

  /**
   * Whether the audio has been fully downloaded and inner NativeSound is ready.
   */
  isLoaded(): boolean {
    return this._loaded;
  }

  /**
   * Force-initialize the inner NativeSound's audio graph.
   * Used by AutoMix to ensure GainNodes are ready before crossfade.
   */
  async ensureAudioGraph(): Promise<boolean> {
    if (this._inner) {
      return this._inner.ensureAudioGraph();
    }
    return false;
  }

  unload(): void {
    if (IS_DEV) {
      console.log("BufferedSound: unloading");
    }
    this._unloading = true;

    // Abort download
    if (this._abortController) {
      this._abortController.abort();
      this._abortController = null;
    }

    // Unload inner NativeSound
    if (this._inner) {
      this._inner.unload();
      this._inner = null;
    }

    // Revoke Blob URL to free memory
    if (this._blobUrl) {
      URL.revokeObjectURL(this._blobUrl);
      this._blobUrl = null;
    }

    this._clearListeners();
    this._loaded = false;
  }
}
