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

import { NativeSound } from './NativeSound';
import type { SoundOptions, SoundEventType, SoundEventCallback, ISound } from './types';

// Development mode detection
const IS_DEV = import.meta.env?.DEV ?? false;

/**
 * BufferedSound - Downloads full audio, then delegates to NativeSound via Blob URL
 */
export class BufferedSound implements ISound {
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

  // Pre-load event storage: listeners registered before NativeSound exists
  private _preListeners: Map<SoundEventType, SoundEventCallback[]> = new Map();
  private _preOnceListeners: Map<SoundEventType, SoundEventCallback[]> = new Map();

  // Compatibility structure for legacy spectrum access
  public _sounds: { _node: HTMLAudioElement }[];

  constructor({ src, volume = 1, preload = true }: SoundOptions) {
    this._src = Array.isArray(src) ? src[0] : src;
    this._volume = volume;
    this._preload = preload;
    this._sounds = [{ _node: new Audio() }];

    if (IS_DEV) {
      console.log('BufferedSound created:', this._src);
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
        console.log('BufferedSound: Starting download:', this._src);
      }

      const response = await fetch(this._src, {
        signal: this._abortController.signal,
        credentials: 'omit',
      });

      if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`);
      }

      const contentType = response.headers.get('content-type') || 'audio/mpeg';
      const contentLength = response.headers.get('content-length');
      const total = contentLength ? parseInt(contentLength, 10) : 0;

      let arrayBuffer: ArrayBuffer;

      if (!response.body || total === 0) {
        // Fallback: no streaming, just read all at once
        arrayBuffer = await response.arrayBuffer();
        this._downloadProgress = 1;
        this._emitPre('progress', 1);
      } else {
        // Stream download with progress tracking
        const reader = response.body.getReader();
        const chunks: Uint8Array[] = [];
        let received = 0;

        while (true) {
          const { done, value } = await reader.read();
          if (done) break;

          chunks.push(value);
          received += value.length;

          if (total > 0) {
            this._downloadProgress = received / total;
            this._emitPre('progress', this._downloadProgress);
          }
        }

        // Combine chunks
        arrayBuffer = new ArrayBuffer(received);
        const view = new Uint8Array(arrayBuffer);
        let offset = 0;
        for (const chunk of chunks) {
          view.set(chunk, offset);
          offset += chunk.length;
        }
      }

      if (this._unloading) return;

      // Create Blob URL
      const blob = new Blob([arrayBuffer], { type: contentType });
      this._blobUrl = URL.createObjectURL(blob);

      if (IS_DEV) {
        console.log('BufferedSound: Download complete, Blob URL created');
      }

      // Create NativeSound with Blob URL (fully in-memory, no network needed)
      this._createInnerSound();
    } catch (err: any) {
      if (err.name === 'AbortError') {
        if (IS_DEV) {
          console.log('BufferedSound: Download aborted');
        }
        return;
      }

      console.error('BufferedSound: Failed to download audio:', err);
      this._emitPre('loaderror');
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

    // Forward all pre-registered event listeners to NativeSound
    for (const [event, callbacks] of this._preListeners) {
      for (const cb of callbacks) {
        this._inner.on(event, cb);
      }
    }
    for (const [event, callbacks] of this._preOnceListeners) {
      for (const cb of callbacks) {
        this._inner.once(event, cb);
      }
    }
    this._preListeners.clear();
    this._preOnceListeners.clear();

    // Update compatibility structure to real audio element
    this._sounds = this._inner._sounds;
    this._loaded = true;

    // Execute pending seek (must happen before play)
    if (this._pendingSeek !== null) {
      this._inner.seek(this._pendingSeek);
      this._pendingSeek = null;
    }

    // Execute pending play
    if (this._pendingPlay) {
      this._pendingPlay = false;
      this._inner.play();
    }
  }

  // ── Pre-load Event Emitter ────────────────────────────

  private _emitPre(event: SoundEventType, ...args: unknown[]): void {
    const listeners = this._preListeners.get(event);
    if (listeners) {
      for (const cb of listeners) {
        try { cb(...args); } catch (e) {
          console.error(`BufferedSound: Error in pre-load ${event} listener:`, e);
        }
      }
    }
    const onceListeners = this._preOnceListeners.get(event);
    if (onceListeners) {
      for (const cb of onceListeners) {
        try { cb(...args); } catch (e) {
          console.error(`BufferedSound: Error in pre-load once ${event} listener:`, e);
        }
      }
      this._preOnceListeners.delete(event);
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

  seek(pos?: number): number | this {
    if (this._inner) {
      if (pos === undefined) {
        return this._inner.seek() as number;
      }
      this._inner.seek(pos);
      return this;
    }
    // Not loaded yet
    if (pos === undefined) {
      return this._pendingSeek ?? 0;
    }
    this._pendingSeek = pos;
    return this;
  }

  duration(): number {
    return this._inner?.duration() ?? 0;
  }

  volume(vol?: number): number | this {
    if (this._inner) {
      if (vol === undefined) {
        return this._inner.volume() as number;
      }
      this._inner.volume(vol);
      return this;
    }
    if (vol === undefined) {
      return this._volume;
    }
    this._volume = Math.max(0, Math.min(1, vol));
    return this;
  }

  fade(from: number, to: number, duration: number): this {
    if (this._inner) {
      this._inner.fade(from, to, duration);
    }
    return this;
  }

  on(event: SoundEventType, callback: SoundEventCallback): this {
    if (this._inner) {
      this._inner.on(event, callback);
    } else {
      if (!this._preListeners.has(event)) {
        this._preListeners.set(event, []);
      }
      this._preListeners.get(event)!.push(callback);
    }
    return this;
  }

  once(event: SoundEventType, callback: SoundEventCallback): this {
    if (this._inner) {
      this._inner.once(event, callback);
    } else {
      if (!this._preOnceListeners.has(event)) {
        this._preOnceListeners.set(event, []);
      }
      this._preOnceListeners.get(event)!.push(callback);
    }
    return this;
  }

  off(event: SoundEventType, callback?: SoundEventCallback): this {
    if (this._inner) {
      this._inner.off(event, callback);
    } else if (callback) {
      const listeners = this._preListeners.get(event);
      if (listeners) {
        const idx = listeners.indexOf(callback);
        if (idx > -1) listeners.splice(idx, 1);
      }
      const onceListeners = this._preOnceListeners.get(event);
      if (onceListeners) {
        const idx = onceListeners.indexOf(callback);
        if (idx > -1) onceListeners.splice(idx, 1);
      }
    } else {
      this._preListeners.delete(event);
      this._preOnceListeners.delete(event);
    }
    return this;
  }

  getFrequencyData(): Uint8Array<ArrayBuffer> {
    return this._inner?.getFrequencyData() ?? new Uint8Array(0);
  }

  getFFTData(): number[] {
    return this._inner?.getFFTData() ?? [];
  }

  getLowFrequencyVolume(): number {
    return this._inner?.getLowFrequencyVolume() ?? 0;
  }

  getAverageAmplitude(): number {
    return this._inner?.getAverageAmplitude() ?? 0;
  }

  /**
   * Get download progress (0-1)
   */
  getDownloadProgress(): number {
    return this._downloadProgress;
  }

  unload(): void {
    if (IS_DEV) {
      console.log('BufferedSound: unloading');
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

    this._preListeners.clear();
    this._preOnceListeners.clear();
    this._loaded = false;
  }
}
