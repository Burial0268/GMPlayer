/**
 * AudioContextManager - Global AudioContext singleton with mobile support
 *
 * Key features:
 * - Single AudioContext instance shared across all sounds
 * - Mobile background playback support (visibility change handling)
 * - iOS AudioContext interruption recovery
 * - Automatic resume on user interaction
 */

type AudioContextState = 'suspended' | 'running' | 'closed' | 'interrupted';

interface AudioContextManagerEvents {
  statechange: (state: AudioContextState) => void;
  resumed: () => void;
  interrupted: () => void;
}

class AudioContextManagerClass {
  private _ctx: AudioContext | null = null;
  private _resumePromise: Promise<void> | null = null;
  private _listeners: Map<keyof AudioContextManagerEvents, Set<Function>> = new Map();
  private _isMobile: boolean;
  private _hasUserInteraction: boolean = false;
  private _boundHandlers: {
    visibilityChange: () => void;
    userInteraction: () => void;
    stateChange: () => void;
  } | null = null;

  constructor() {
    this._isMobile = this._detectMobile();
    this._setupGlobalListeners();
  }

  private _detectMobile(): boolean {
    if (typeof navigator === 'undefined') return false;
    return /Android|webOS|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i.test(navigator.userAgent)
      || ('ontouchstart' in window)
      || (navigator.maxTouchPoints > 0);
  }

  private _setupGlobalListeners(): void {
    if (typeof document === 'undefined') return;

    this._boundHandlers = {
      visibilityChange: this._handleVisibilityChange.bind(this),
      userInteraction: this._handleUserInteraction.bind(this),
      stateChange: this._handleStateChange.bind(this),
    };

    // Visibility change for mobile background playback
    document.addEventListener('visibilitychange', this._boundHandlers.visibilityChange);

    // User interaction listeners for AudioContext resume
    const interactionEvents = ['touchstart', 'touchend', 'click', 'keydown'];
    interactionEvents.forEach(event => {
      document.addEventListener(event, this._boundHandlers!.userInteraction, { once: false, passive: true });
    });
  }

  private _handleVisibilityChange(): void {
    if (!this._ctx) return;

    if (document.visibilityState === 'visible') {
      // Page became visible - resume AudioContext
      this._tryResume();
    }
    // Note: We don't suspend on hidden - let audio continue in background
  }

  private _handleUserInteraction(): void {
    this._hasUserInteraction = true;
    this._tryResume();
  }

  private _handleStateChange(): void {
    if (!this._ctx) return;
    const state = this._ctx.state as AudioContextState;
    this._emit('statechange', state);

    if (state === 'interrupted') {
      // iOS specific - audio was interrupted (e.g., phone call)
      this._emit('interrupted');
    }
  }

  /**
   * Get or create the global AudioContext
   * Note: On mobile, creation may fail without user interaction
   */
  getContext(): AudioContext | null {
    if (this._ctx && this._ctx.state !== 'closed') {
      return this._ctx;
    }

    try {
      const AudioContextClass = window.AudioContext || window.webkitAudioContext;
      if (!AudioContextClass) {
        console.warn('AudioContextManager: Web Audio API not supported');
        return null;
      }

      // Mobile-optimized settings
      const options: AudioContextOptions = this._isMobile
        ? { latencyHint: 'playback', sampleRate: 44100 }
        : { latencyHint: 'interactive' };

      this._ctx = new AudioContextClass(options);

      // Listen for state changes (iOS interruption support)
      this._ctx.addEventListener('statechange', this._boundHandlers!.stateChange);

      // Attempt immediate resume if we have user interaction
      if (this._hasUserInteraction) {
        this._tryResume();
      }

      return this._ctx;
    } catch (err) {
      console.error('AudioContextManager: Failed to create AudioContext', err);
      return null;
    }
  }

  /**
   * Check if AudioContext is available and running
   */
  isReady(): boolean {
    return this._ctx !== null && this._ctx.state === 'running';
  }

  /**
   * Get current AudioContext state
   */
  getState(): AudioContextState | null {
    return this._ctx?.state as AudioContextState | null;
  }

  /**
   * Whether running on mobile device
   */
  isMobile(): boolean {
    return this._isMobile;
  }

  /**
   * Attempt to resume AudioContext
   * Returns a promise that resolves when resumed or rejects on failure
   */
  async resume(): Promise<void> {
    if (!this._ctx) {
      throw new Error('AudioContext not initialized');
    }

    if (this._ctx.state === 'running') {
      return;
    }

    // Avoid multiple concurrent resume attempts
    if (this._resumePromise) {
      return this._resumePromise;
    }

    this._resumePromise = this._ctx.resume()
      .then(() => {
        this._emit('resumed');
      })
      .finally(() => {
        this._resumePromise = null;
      });

    return this._resumePromise;
  }

  /**
   * Try to resume without throwing errors
   */
  private _tryResume(): void {
    if (!this._ctx || this._ctx.state === 'running' || this._ctx.state === 'closed') {
      return;
    }

    this._ctx.resume().then(() => {
      this._emit('resumed');
    }).catch(() => {
      // Ignore resume errors - will retry on next user interaction
    });
  }

  /**
   * Subscribe to events
   */
  on<K extends keyof AudioContextManagerEvents>(
    event: K,
    callback: AudioContextManagerEvents[K]
  ): void {
    if (!this._listeners.has(event)) {
      this._listeners.set(event, new Set());
    }
    this._listeners.get(event)!.add(callback);
  }

  /**
   * Unsubscribe from events
   */
  off<K extends keyof AudioContextManagerEvents>(
    event: K,
    callback: AudioContextManagerEvents[K]
  ): void {
    this._listeners.get(event)?.delete(callback);
  }

  private _emit<K extends keyof AudioContextManagerEvents>(
    event: K,
    ...args: Parameters<AudioContextManagerEvents[K]>
  ): void {
    this._listeners.get(event)?.forEach(cb => {
      try {
        (cb as Function)(...args);
      } catch (e) {
        console.error(`AudioContextManager: Error in ${event} listener`, e);
      }
    });
  }

  /**
   * Close and cleanup AudioContext
   * Only call this when completely done with audio (e.g., app shutdown)
   */
  async close(): Promise<void> {
    if (!this._ctx) return;

    try {
      this._ctx.removeEventListener('statechange', this._boundHandlers!.stateChange);
      await this._ctx.close();
    } catch (e) {
      // Ignore close errors
    }
    this._ctx = null;
  }

  /**
   * Cleanup all listeners (for testing/hot reload)
   */
  destroy(): void {
    if (typeof document !== 'undefined' && this._boundHandlers) {
      document.removeEventListener('visibilitychange', this._boundHandlers.visibilityChange);
      const interactionEvents = ['touchstart', 'touchend', 'click', 'keydown'];
      interactionEvents.forEach(event => {
        document.removeEventListener(event, this._boundHandlers!.userInteraction);
      });
    }
    this._listeners.clear();
    this.close();
  }
}

export const AudioContextManager = new AudioContextManagerClass();
