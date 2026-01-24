/**
 * NativeSound - HTML5 Audio player with Web Audio API integration
 * Based on dev branch AudioElementPlayer architecture
 */

import { AudioEffectManager } from './AudioEffectManager';
import type { SoundOptions, SoundEventType, SoundEventCallback, ISound } from './types';

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
  private _audioCtx: AudioContext | null = null;
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
  } | null = null;

  // Compatibility structure for legacy spectrum access
  public _sounds: { _node: HTMLAudioElement }[];

  constructor({ src, volume = 1, preload = true }: SoundOptions) {
    // Audio element
    this._audio = new Audio();
    this._audio.crossOrigin = 'anonymous';
    this._audio.preload = preload ? 'auto' : 'none';
    this._audio.src = Array.isArray(src) ? src[0] : src;

    // State
    this._volume = volume;

    // Compatibility structure for legacy spectrum access
    this._sounds = [{ _node: this._audio }];

    this._bindNativeEvents();
    console.log('NativeSound created:', this._audio.src);
  }

  /**
   * Initialize Web Audio API graph
   * Chain: source -> effectManager (analyser) -> gainNode -> destination
   */
  private _initAudioGraph(): void {
    if (this._isAudioGraphInitialized) return;

    try {
      this._audioCtx = new (window.AudioContext || window.webkitAudioContext)();

      // Create source from audio element
      this._sourceNode = this._audioCtx.createMediaElementSource(this._audio);

      // Create gain node for volume control
      this._gainNode = this._audioCtx.createGain();
      this._gainNode.gain.value = this._volume;

      // Create effect manager (analyser)
      this._effectManager = new AudioEffectManager(this._audioCtx);

      // Connect chain: source -> effectManager -> gainNode -> destination
      const processedNode = this._effectManager.connect(this._sourceNode);
      processedNode.connect(this._gainNode);
      this._gainNode.connect(this._audioCtx.destination);

      this._isAudioGraphInitialized = true;
      console.log('NativeSound: Audio graph initialized');
    } catch (err) {
      console.error('NativeSound: Failed to initialize audio graph', err);
    }
  }

  private _bindNativeEvents(): void {
    // Create bound handlers that can be removed later
    this._boundHandlers = {
      canplaythrough: () => {
        console.log('NativeSound: canplaythrough (load)');
        this._loaded = true;
        this._emit('load');
        if (this._pendingPlay) {
          this._pendingPlay = false;
          this._doPlay();
        }
      },
      play: () => {
        console.log('NativeSound: play event');
        this._isPlaying = true;
        this._emit('play');
      },
      pause: () => {
        if (this._unloading) return;
        console.log('NativeSound: pause event');
        this._isPlaying = false;
        this._emit('pause');
      },
      ended: () => {
        console.log('NativeSound: ended event');
        this._isPlaying = false;
        this._emit('end');
      },
      error: () => {
        if (this._unloading) return;
        const error = this._audio.error;
        const errorMessages: Record<number, string> = {
          1: 'MEDIA_ERR_ABORTED - fetching process aborted by user',
          2: 'MEDIA_ERR_NETWORK - error occurred when downloading',
          3: 'MEDIA_ERR_DECODE - error occurred when decoding',
          4: 'MEDIA_ERR_SRC_NOT_SUPPORTED - audio not supported',
        };
        const errorMsg = error ? errorMessages[error.code] || `Unknown error code: ${error.code}` : 'Unknown error';
        console.error('NativeSound: error event -', errorMsg, 'src:', this._audio.src);
        this._isPlaying = false;
        this._pendingPlay = false;
        this._emit('loaderror');
        this._emit('playerror');
      },
    };

    // Handle case where audio is already loaded (cached)
    if (this._audio.readyState >= 3) {
      console.log('NativeSound: audio already loaded (readyState:', this._audio.readyState, ')');
      this._loaded = true;
      // Defer emit to allow event listeners to be registered
      setTimeout(() => this._emit('load'), 0);
    } else {
      this._audio.addEventListener('canplaythrough', this._boundHandlers.canplaythrough, { once: true });
    }

    this._audio.addEventListener('play', this._boundHandlers.play);
    this._audio.addEventListener('pause', this._boundHandlers.pause);
    this._audio.addEventListener('ended', this._boundHandlers.ended);
    this._audio.addEventListener('error', this._boundHandlers.error);
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
      this._initAudioGraph();
    }

    // Resume AudioContext if suspended
    if (this._audioCtx && this._audioCtx.state === 'suspended') {
      await this._audioCtx.resume();
    }

    try {
      await this._audio.play();
    } catch (err) {
      console.error('NativeSound: play() failed:', err);
      this._emit('playerror');
    }
  }

  play(): this {
    console.log('NativeSound: play() called, loaded:', this._loaded, 'readyState:', this._audio.readyState);
    if (this._loaded || this._audio.readyState >= 3) {
      this._loaded = true;
      this._doPlay();
    } else {
      console.log('NativeSound: queuing play until loaded');
      this._pendingPlay = true;
    }
    return this;
  }

  pause(): this {
    this._audio.pause();
    return this;
  }

  stop(): this {
    this._audio.pause();
    this._audio.currentTime = 0;
    this._isPlaying = false;
    return this;
  }

  seek(pos?: number): number | this {
    if (pos === undefined) {
      return this._audio.currentTime;
    }
    this._audio.currentTime = pos;
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
      this._initAudioGraph();
    }

    // Use Web Audio API's built-in fade if available
    if (this._gainNode && this._audioCtx) {
      const currentTime = this._audioCtx.currentTime;
      this._gainNode.gain.cancelScheduledValues(currentTime);
      this._gainNode.gain.setValueAtTime(from, currentTime);
      this._gainNode.gain.linearRampToValueAtTime(to, currentTime + duration / 1000);

      // Emit fade event after duration
      setTimeout(() => {
        this._volume = to;
        this._emit('fade');
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
          this._emit('fade');
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
  getFrequencyData(): Uint8Array {
    return this._effectManager ? this._effectManager.getFrequencyData() : new Uint8Array(0);
  }

  /**
   * Get smoothed low frequency volume
   * @returns number in 0-1 range
   */
  getLowFrequencyVolume(): number {
    return this._effectManager ? this._effectManager.getLowFrequencyVolume() : 0;
  }

  unload(): void {
    console.log('NativeSound: unloading');
    this._unloading = true;

    if (this._fadeAnimationId) {
      cancelAnimationFrame(this._fadeAnimationId);
      this._fadeAnimationId = null;
    }

    // Remove native event listeners first
    if (this._boundHandlers) {
      this._audio.removeEventListener('canplaythrough', this._boundHandlers.canplaythrough);
      this._audio.removeEventListener('play', this._boundHandlers.play);
      this._audio.removeEventListener('pause', this._boundHandlers.pause);
      this._audio.removeEventListener('ended', this._boundHandlers.ended);
      this._audio.removeEventListener('error', this._boundHandlers.error);
      this._boundHandlers = null;
    }

    this._audio.pause();
    this._audio.src = '';
    this._audio.load();

    // Cleanup Web Audio nodes
    if (this._effectManager) {
      this._effectManager.disconnect();
      this._effectManager = null;
    }
    if (this._sourceNode) {
      this._sourceNode.disconnect();
      this._sourceNode = null;
    }
    if (this._gainNode) {
      this._gainNode.disconnect();
      this._gainNode = null;
    }
    if (this._audioCtx) {
      this._audioCtx.close().catch(console.warn);
      this._audioCtx = null;
    }

    this._eventListeners.clear();
    this._onceListeners.clear();
    this._isAudioGraphInitialized = false;
  }
}
