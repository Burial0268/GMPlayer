/**
 * NativeSound - HTML5 Audio player with Web Audio API integration
 *
 * Key improvements:
 * - Uses global AudioContextManager for better resource management
 * - Prevents MediaElementAudioSourceNode re-creation errors
 * - Mobile-optimized background playback support
 * - Enhanced error recovery
 * - Reduced console.log for production performance
 */

import { AudioEffectManager } from "./AudioEffectManager";
import { AudioContextManager } from "./AudioContextManager";
import type { SoundOptions, SoundEventType, SoundEventCallback, ISound } from "./types";

// Development mode detection
const IS_DEV = import.meta.env?.DEV ?? false;

/**
 * NativeSound - HTML5 Audio player with Web Audio API integration
 */
export class NativeSound implements ISound {
  // Audio element
  private _audio: HTMLAudioElement;

  // State
  private _volume: number;
  private _isPlaying: boolean = false;
  private _loaded: boolean = false;
  private _pendingPlay: boolean = false;
  private _unloading: boolean = false;

  // Event system
  private _eventListeners: Map<SoundEventType, SoundEventCallback[]> = new Map();
  private _onceListeners: Map<SoundEventType, SoundEventCallback[]> = new Map();

  // Web Audio API nodes
  private _sourceNode: MediaElementAudioSourceNode | null = null;
  private _gainNode: GainNode | null = null;
  private _effectManager: AudioEffectManager | null = null;
  private _isAudioGraphInitialized: boolean = false;

  // Fade animation
  private _fadeAnimationId: number | null = null;

  // Native event handler references (for cleanup)
  private _boundHandlers: {
    canplaythrough: () => void;
    play: () => void;
    pause: () => void;
    ended: () => void;
    error: () => void;
    stalled: () => void;
    waiting: () => void;
    loadeddata: () => void;
  } | null = null;

  // AudioContext resume handler
  private _boundResumeHandler: (() => void) | null = null;

  // Error recovery state
  private _consecutiveErrors: number = 0;
  private _maxConsecutiveErrors: number = 3;
  private _lastErrorTime: number = 0;
  private _errorResetDelay: number = 5000; // Reset error count after 5s

  // Seek guard: handler for the current pending 'seeked' event
  private _seekedHandler: (() => void) | null = null;

  // Compatibility structure for legacy spectrum access
  public _sounds: { _node: HTMLAudioElement }[];

  constructor({ src, volume = 1, preload = true }: SoundOptions) {
    // Audio element
    this._audio = new Audio();
    this._audio.crossOrigin = "anonymous";
    this._audio.preload = preload ? "auto" : "metadata";
    this._audio.src = Array.isArray(src) ? src[0] : src;

    // Mobile-specific optimizations
    if (AudioContextManager.isMobile()) {
      // Disable preload on mobile to save bandwidth
      this._audio.preload = "metadata";
      // Enable inline playback on iOS
      (this._audio as any).playsInline = true;
      this._audio.setAttribute("playsinline", "");
      this._audio.setAttribute("webkit-playsinline", "");
    }

    // State
    this._volume = volume;

    // Compatibility structure for legacy spectrum access
    this._sounds = [{ _node: this._audio }];

    this._bindNativeEvents();

    // Listen for AudioContext state changes
    this._boundResumeHandler = this._handleContextResume.bind(this);
    AudioContextManager.on("resumed", this._boundResumeHandler);

    if (IS_DEV) {
      console.log("NativeSound created:", this._audio.src);
    }
  }

  /**
   * Handle AudioContext resume events
   */
  private _handleContextResume(): void {
    // If playback was pending while context was suspended, retry
    if (this._pendingPlay && this._loaded) {
      if (IS_DEV) {
        console.log("NativeSound: AudioContext resumed, retrying play");
      }
      this._pendingPlay = false;
      this._doPlay();
    }
  }

  /**
   * Initialize Web Audio API graph
   * Chain: source -> gainNode -> destination (audio output)
   *        source -> effectManager (parallel branch for spectrum analysis only)
   */
  private async _initAudioGraph(): Promise<boolean> {
    if (this._isAudioGraphInitialized) return true;

    try {
      const audioCtx = AudioContextManager.getContext();
      if (!audioCtx) {
        console.warn("NativeSound: AudioContext not available");
        return false;
      }

      // Register PCM capture worklet before creating AudioEffectManager
      // Skip on mobile: worklet is not used (AnalyserNode fallback for lowFreqVolume)
      if (!AudioContextManager.isMobile()) {
        await AudioContextManager.registerWorklet();
      }

      // CRITICAL: Only create MediaElementSourceNode once per audio element
      // Attempting to create multiple sources from same element throws DOMException
      if (!this._sourceNode) {
        this._sourceNode = audioCtx.createMediaElementSource(this._audio);
      }

      // Create gain node for volume control
      this._gainNode = audioCtx.createGain();
      this._gainNode.gain.value = this._volume;

      // Create effect manager (WASM FFT + AnalyserNode for lowFreqVolume)
      this._effectManager = new AudioEffectManager(audioCtx);

      // Connect effect manager for spectrum analysis (parallel branch, doesn't affect audio)
      this._effectManager.connect(this._sourceNode);

      // Connect main audio chain: source -> gainNode -> destination
      this._sourceNode.connect(this._gainNode);
      this._gainNode.connect(audioCtx.destination);

      this._isAudioGraphInitialized = true;

      if (IS_DEV) {
        console.log("NativeSound: Audio graph initialized");
      }

      return true;
    } catch (err) {
      console.error("NativeSound: Failed to initialize audio graph", err);
      return false;
    }
  }

  private _bindNativeEvents(): void {
    // Create bound handlers that can be removed later
    this._boundHandlers = {
      canplaythrough: () => {
        if (IS_DEV) {
          console.log("NativeSound: canplaythrough (load)");
        }
        this._loaded = true;
        this._emit("load");
        if (this._pendingPlay) {
          this._pendingPlay = false;
          this._doPlay();
        }
      },
      loadeddata: () => {
        // Fired when first frame is loaded - good for mobile
        if (!this._loaded && this._audio.readyState >= 2) {
          if (IS_DEV) {
            console.log("NativeSound: loadeddata (early load)");
          }
          this._loaded = true;
          this._emit("load");
          if (this._pendingPlay) {
            this._pendingPlay = false;
            this._doPlay();
          }
        }
      },
      play: () => {
        if (IS_DEV) {
          console.log("NativeSound: play event");
        }
        this._isPlaying = true;
        this._consecutiveErrors = 0; // Reset error count on successful play
        this._emit("play");
      },
      pause: () => {
        if (this._unloading) return;
        if (IS_DEV) {
          console.log("NativeSound: pause event");
        }
        this._isPlaying = false;
        this._emit("pause");
      },
      ended: () => {
        if (IS_DEV) {
          console.log("NativeSound: ended event");
        }
        this._isPlaying = false;
        this._emit("end");
      },
      stalled: () => {
        if (this._unloading) return;
        console.warn("NativeSound: stalled - network issue");
      },
      waiting: () => {
        if (this._unloading) return;
        if (IS_DEV) {
          console.log("NativeSound: waiting for data");
        }
      },
      error: () => {
        if (this._unloading) return;

        // Error recovery logic
        const now = Date.now();
        if (now - this._lastErrorTime > this._errorResetDelay) {
          this._consecutiveErrors = 0;
        }
        this._lastErrorTime = now;
        this._consecutiveErrors++;

        const error = this._audio.error;
        const errorMessages: Record<number, string> = {
          1: "MEDIA_ERR_ABORTED",
          2: "MEDIA_ERR_NETWORK",
          3: "MEDIA_ERR_DECODE",
          4: "MEDIA_ERR_SRC_NOT_SUPPORTED",
        };
        const errorMsg = error
          ? errorMessages[error.code] || `Unknown error code: ${error.code}`
          : "Unknown error";

        console.error("NativeSound: error event -", errorMsg, "src:", this._audio.src);

        this._isPlaying = false;
        this._pendingPlay = false;

        // Only emit errors if not exceeding max
        if (this._consecutiveErrors <= this._maxConsecutiveErrors) {
          this._emit("loaderror");
          this._emit("playerror");
        }
      },
    };

    // Handle case where audio is already loaded (cached)
    if (this._audio.readyState >= 3) {
      if (IS_DEV) {
        console.log("NativeSound: audio already loaded (readyState:", this._audio.readyState, ")");
      }
      this._loaded = true;
      // Defer emit to allow event listeners to be registered
      setTimeout(() => this._emit("load"), 0);
    } else {
      this._audio.addEventListener("canplaythrough", this._boundHandlers.canplaythrough, {
        once: true,
      });
      // Also listen for loadeddata for faster mobile response
      this._audio.addEventListener("loadeddata", this._boundHandlers.loadeddata, { once: true });
    }

    this._audio.addEventListener("play", this._boundHandlers.play);
    this._audio.addEventListener("pause", this._boundHandlers.pause);
    this._audio.addEventListener("ended", this._boundHandlers.ended);
    this._audio.addEventListener("error", this._boundHandlers.error);
    this._audio.addEventListener("stalled", this._boundHandlers.stalled);
    this._audio.addEventListener("waiting", this._boundHandlers.waiting);
  }

  private _emit(event: SoundEventType, ...args: unknown[]): void {
    const listeners = this._eventListeners.get(event);
    if (listeners) {
      listeners.forEach((cb) => {
        try {
          cb(...args);
        } catch (e) {
          console.error(`NativeSound: Error in ${event} listener:`, e);
        }
      });
    }
    const onceListeners = this._onceListeners.get(event);
    if (onceListeners) {
      onceListeners.forEach((cb) => {
        try {
          cb(...args);
        } catch (e) {
          console.error(`NativeSound: Error in once ${event} listener:`, e);
        }
      });
      this._onceListeners.delete(event);
    }
  }

  playing(): boolean {
    return !this._audio.paused && !this._audio.ended;
  }

  private async _doPlay(): Promise<void> {
    // Initialize audio graph on first play (requires user interaction)
    if (!this._isAudioGraphInitialized) {
      const success = await this._initAudioGraph();
      if (!success) {
        console.error("NativeSound: Failed to initialize audio graph");
        this._emit("playerror");
        return;
      }
    }

    // Resume AudioContext if suspended
    const audioCtx = AudioContextManager.getContext();
    if (audioCtx && audioCtx.state === "suspended") {
      try {
        await AudioContextManager.resume();
      } catch (err) {
        console.warn("NativeSound: Failed to resume AudioContext", err);
        // Continue anyway - might work
      }
    }

    try {
      await this._audio.play();
    } catch (err: any) {
      // Handle common play() rejections
      const errName = err?.name || "";
      if (errName === "NotAllowedError") {
        console.warn("NativeSound: play() blocked - user interaction required");
        this._pendingPlay = true; // Retry on next user interaction
      } else if (errName === "NotSupportedError") {
        console.error("NativeSound: play() failed - source not supported");
        this._emit("playerror");
      } else if (errName === "AbortError") {
        console.warn("NativeSound: play() aborted");
      } else {
        console.error("NativeSound: play() failed:", err);
        this._emit("playerror");
      }
    }
  }

  play(): this {
    if (IS_DEV) {
      console.log(
        "NativeSound: play() called, loaded:",
        this._loaded,
        "readyState:",
        this._audio.readyState,
      );
    }
    if (this._loaded || this._audio.readyState >= 2) {
      this._loaded = true;
      this._doPlay();
    } else {
      if (IS_DEV) {
        console.log("NativeSound: queuing play until loaded");
      }
      this._pendingPlay = true;
      // Force load if not already loading
      if (this._audio.readyState === 0) {
        this._audio.load();
      }
    }
    return this;
  }

  pause(): this {
    this._audio.pause();
    this._pendingPlay = false;
    return this;
  }

  stop(): this {
    this._audio.pause();
    this._audio.currentTime = 0;
    this._isPlaying = false;
    this._pendingPlay = false;
    return this;
  }

  seek(pos?: number): number | this {
    if (pos === undefined) {
      return this._audio.currentTime;
    }
    try {
      // Remove previous seeked handler if rapid-seeking
      if (this._seekedHandler) {
        this._audio.removeEventListener("seeked", this._seekedHandler);
        this._seekedHandler = null;
      }

      this._audio.currentTime = pos;
      // Flush stale PCM data from the WASM FFTPlayer queue
      // and gate incoming PCM until the seek actually completes
      if (this._effectManager) {
        this._effectManager.clearFFTState();
        this._seekedHandler = () => {
          this._seekedHandler = null;
          this._effectManager?.onSeeked();
        };
        this._audio.addEventListener("seeked", this._seekedHandler, { once: true });
      }
    } catch (e) {
      console.warn("NativeSound: Failed to seek to", pos, e);
    }
    return this;
  }

  duration(): number {
    return this._audio.duration || 0;
  }

  volume(vol?: number): number | this {
    if (vol === undefined) {
      return this._volume;
    }
    this._volume = Math.max(0, Math.min(1, vol));
    // Use GainNode for volume control (doesn't affect spectrum)
    if (this._gainNode) {
      this._gainNode.gain.value = this._volume;
    }
    return this;
  }

  fade(from: number, to: number, duration: number): this {
    if (this._fadeAnimationId) {
      cancelAnimationFrame(this._fadeAnimationId);
    }

    // Initialize audio graph if needed for GainNode access
    if (!this._isAudioGraphInitialized) {
      this._initAudioGraph().catch(() => {});
    }

    const audioCtx = AudioContextManager.getContext();

    // Use Web Audio API's built-in fade if available
    if (this._gainNode && audioCtx && audioCtx.state === "running") {
      const currentTime = audioCtx.currentTime;
      this._gainNode.gain.cancelScheduledValues(currentTime);
      this._gainNode.gain.setValueAtTime(from, currentTime);
      this._gainNode.gain.linearRampToValueAtTime(to, currentTime + duration / 1000);

      // Emit fade event after duration
      setTimeout(() => {
        this._volume = to;
        this._emit("fade");
      }, duration);
    } else {
      // Fallback to requestAnimationFrame fade
      this._volume = from;
      const startTime = performance.now();

      const animate = (time: number): void => {
        const progress = Math.max(0, Math.min((time - startTime) / duration, 1));
        const eased = 1 - Math.pow(1 - progress, 2);
        const newVolume = Math.max(0, Math.min(1, from + (to - from) * eased));
        this._volume = newVolume;

        if (progress < 1) {
          this._fadeAnimationId = requestAnimationFrame(animate);
        } else {
          this._fadeAnimationId = null;
          this._volume = Math.max(0, Math.min(1, to));
          this._emit("fade");
        }
      };

      this._fadeAnimationId = requestAnimationFrame(animate);
    }
    return this;
  }

  on(event: SoundEventType, callback: SoundEventCallback): this {
    if (!this._eventListeners.has(event)) {
      this._eventListeners.set(event, []);
    }
    this._eventListeners.get(event)!.push(callback);
    return this;
  }

  once(event: SoundEventType, callback: SoundEventCallback): this {
    if (!this._onceListeners.has(event)) {
      this._onceListeners.set(event, []);
    }
    this._onceListeners.get(event)!.push(callback);
    return this;
  }

  off(event: SoundEventType, callback?: SoundEventCallback): this {
    if (callback) {
      const listeners = this._eventListeners.get(event);
      if (listeners) {
        const index = listeners.indexOf(callback);
        if (index > -1) listeners.splice(index, 1);
      }
    } else {
      this._eventListeners.delete(event);
    }
    return this;
  }

  /**
   * Get frequency data for spectrum visualization
   * @returns Uint8Array containing frequency data
   */
  getFrequencyData(): Uint8Array<ArrayBuffer> {
    return this._effectManager ? this._effectManager.getFrequencyData() : new Uint8Array(0);
  }

  /**
   * Get FFT data as number[] from WASM FFTPlayer
   * @returns number[] of spectrum values (0-255 range)
   */
  getFFTData(): number[] {
    return this._effectManager ? this._effectManager.getFFTData() : [];
  }

  /**
   * Get smoothed low frequency volume
   * @returns number in 0-1 range
   */
  getLowFrequencyVolume(): number {
    return this._effectManager ? this._effectManager.getLowFrequencyVolume() : 0;
  }

  /**
   * Get average amplitude from the last getFrequencyData() call
   * @returns number in 0-255 range
   */
  getAverageAmplitude(): number {
    return this._effectManager ? this._effectManager.getAverageAmplitude() : 0;
  }

  /**
   * Get the AudioEffectManager instance for direct configuration.
   * Allows runtime tuning of freq range, lowFreqVolume parameters, etc.
   */
  getEffectManager(): AudioEffectManager | null {
    return this._effectManager;
  }

  /**
   * Get the GainNode for crossfade volume control.
   * Used by AutoMixEngine/CrossfadeManager.
   */
  getGainNode(): GainNode | null {
    return this._gainNode;
  }

  /**
   * Get the MediaElementAudioSourceNode.
   * Used by AutoMixEngine for audio graph inspection.
   */
  getSourceNode(): MediaElementAudioSourceNode | null {
    return this._sourceNode;
  }

  /**
   * Force-initialize the audio graph before playback.
   * Used by AutoMix to ensure GainNodes are ready for crossfade scheduling.
   */
  async ensureAudioGraph(): Promise<boolean> {
    if (this._isAudioGraphInitialized) return true;
    return this._initAudioGraph();
  }

  unload(): void {
    if (IS_DEV) {
      console.log("NativeSound: unloading");
    }
    this._unloading = true;

    if (this._fadeAnimationId) {
      cancelAnimationFrame(this._fadeAnimationId);
      this._fadeAnimationId = null;
    }

    // Remove AudioContext event listener
    if (this._boundResumeHandler) {
      AudioContextManager.off("resumed", this._boundResumeHandler);
      this._boundResumeHandler = null;
    }

    // Remove native event listeners first
    if (this._boundHandlers) {
      this._audio.removeEventListener("canplaythrough", this._boundHandlers.canplaythrough);
      this._audio.removeEventListener("loadeddata", this._boundHandlers.loadeddata);
      this._audio.removeEventListener("play", this._boundHandlers.play);
      this._audio.removeEventListener("pause", this._boundHandlers.pause);
      this._audio.removeEventListener("ended", this._boundHandlers.ended);
      this._audio.removeEventListener("error", this._boundHandlers.error);
      this._audio.removeEventListener("stalled", this._boundHandlers.stalled);
      this._audio.removeEventListener("waiting", this._boundHandlers.waiting);
      this._boundHandlers = null;
    }

    // Remove pending seeked handler
    if (this._seekedHandler) {
      this._audio.removeEventListener("seeked", this._seekedHandler);
      this._seekedHandler = null;
    }

    this._audio.pause();
    this._audio.src = "";
    this._audio.load();

    // Cleanup Web Audio nodes
    if (this._effectManager) {
      this._effectManager.disconnect();
      this._effectManager = null;
    }
    if (this._sourceNode) {
      try {
        this._sourceNode.disconnect();
      } catch (e) {
        // May already be disconnected
      }
      this._sourceNode = null;
    }
    if (this._gainNode) {
      try {
        this._gainNode.disconnect();
      } catch (e) {
        // May already be disconnected
      }
      this._gainNode = null;
    }
    // Note: We don't close AudioContext here - it's managed globally

    this._eventListeners.clear();
    this._onceListeners.clear();
    this._isAudioGraphInitialized = false;
  }
}
