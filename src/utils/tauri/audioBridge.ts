/**
 * Tauri bridge for the native audio-backend.
 *
 * v3: AMLL-style message/event architecture — a single `audio_send_msg`
 * Tauri command replaces all individual playback commands.  Events
 * follow the `AudioThreadEvent` shape matching `amll-player-core`.
 */

declare global {
  interface Window {
    __TAURI__?: {
      core: {
        invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
      };
      event: {
        listen: <T>(event: string, handler: (event: { payload: T }) => void) => Promise<() => void>;
        emit: (event: string, payload?: unknown) => Promise<void>;
        emitTo: (target: string, event: string, payload?: unknown) => Promise<void>;
      };
    };
  }
}

export function isTauri(): boolean {
  return "__TAURI__" in window && window.__TAURI__ !== null && typeof window.__TAURI__ === "object";
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!isTauri()) return null;
  return window.__TAURI__!.core.invoke<T>(cmd, args);
}

// ═══════════════════════════════════════════════════════════════════
//  AMLL-style message types (matches Rust AudioThreadMessage)
// ═══════════════════════════════════════════════════════════════════

export type AudioThreadMessage =
  | { type: "resumeAudio" }
  | { type: "pauseAudio" }
  | { type: "resumeOrPauseAudio" }
  | { type: "seekAudio"; position: number; requestId?: number; expectedMusicId?: string }
  | { type: "jumpToSong"; songIndex: number }
  | { type: "jumpToSongAt"; songIndex: number; position: number }
  | { type: "prevSong" }
  | { type: "nextSong" }
  | { type: "nextSongGapless" }
  | { type: "setPlaylist"; songs: SongData[]; windowed?: boolean }
  | { type: "setVolume"; volume: number }
  | { type: "setVolumeRelative"; volume: number }
  | { type: "setAudioOutput"; name: string }
  | { type: "setAnalysis"; enabled: boolean }
  | { type: "setFFT"; enabled: boolean }
  | { type: "setFFTRange"; fromFreq: number; toFreq: number }
  | { type: "setEqualizer"; config: EqualizerConfig }
  | { type: "setDsp"; config: DspConfig }
  | { type: "syncStatus" }
  | { type: "close" }
  | { type: "setMediaControlsEnabled"; enabled: boolean }
  | { type: "automixSetEnabled"; enabled: boolean }
  | { type: "automixConfigure"; config: AutoMixConfig }
  | {
      type: "automixPrepareNext";
      currentIndex: number;
      nextIndex: number;
      nextSong: SongData;
      transitionId?: number | null;
    }
  | { type: "automixCancel" }
  | { type: "automixForceStart"; generation?: number | null }
  | { type: "automixCompleteNative"; generation: number; currentIndex: number; position: number };

export interface AutoMixConfig {
  enabled: boolean;
  crossfadeDuration: number;
  bpmMatch: boolean;
  beatAlign: boolean;
  volumeNorm: boolean;
  smartCurve: boolean;
  transitionStyle: "linear" | "equalPower" | "sCurve";
  transitionEffects: boolean;
  vocalGuard: boolean;
}

export interface DspConfig {
  enabled: boolean;
  inputGainDb?: number;
  equalizer?: EqualizerConfig;
  outputGainDb?: number;
  limiter?: LimiterConfig;
}

export interface EqualizerConfig {
  enabled: boolean;
  preampDb?: number;
  bands?: EqualizerBand[];
}

export interface EqualizerBand {
  enabled?: boolean;
  filterType: "peaking" | "lowShelf" | "highShelf";
  frequency: number;
  gainDb: number;
  q: number;
}

export interface LimiterConfig {
  enabled: boolean;
  thresholdDb?: number;
  ceilingDb?: number;
  releaseMs?: number;
}

export interface SongData {
  type: "local" | "custom";
  filePath?: string;
  id?: string;
  songJsonData?: string;
  origOrder: number;
}

export interface AudioThreadEventMessage<T> {
  callbackId: string;
  data: T | null;
  /**
   * Monotonic sequence number stamped by the Rust event forwarder. The
   * primary Tauri `Channel` and the global-emit fallback deliver the same
   * event with the same `seq`; subscribers use it to drop duplicates that
   * arrive via the secondary transport during a fallback transition
   * (otherwise state-flip dedup breaks on Pause → Seek → Resume bursts and
   * similar patterns).
   *
   * `0` (or missing) means the event was not stamped — fall back to the
   * legacy "no dedup" behavior in that case.
   */
  seq?: number;
}

// ═══════════════════════════════════════════════════════════════════
//  AMLL-style event types (matches Rust AudioThreadEvent)
// ═══════════════════════════════════════════════════════════════════

export interface AudioQuality {
  bitrate: number;
  sampleRate: number;
  channels: number;
}

export interface DisplayAudioInfo {
  name: string;
  artist: string;
  album: string;
  lyric: string;
  coverMediaType: string;
  cover: number[] | null;
  comment: string;
  duration: number;
  position: number;
}

export type AudioThreadEvent =
  | { type: "playPosition"; data: { position: number } }
  | { type: "loadProgress"; data: { position: number } }
  | {
      type: "loadAudio";
      data: {
        musicId: string;
        musicInfo: DisplayAudioInfo;
        quality: AudioQuality;
        currentPlayIndex: number;
      };
    }
  | { type: "loadingAudio"; data: { musicId: string; currentPlayIndex: number } }
  | { type: "audioPlayFinished"; data: { musicId: string } }
  | {
      type: "syncStatus";
      data: {
        musicId: string;
        musicInfo: DisplayAudioInfo;
        isPlaying: boolean;
        duration: number;
        position: number;
        volume: number;
        loadPosition: number;
        playlist: SongData[];
        currentPlayIndex: number;
        playlistInited: boolean;
        quality: AudioQuality;
      };
    }
  | {
      type: "playListChanged";
      data: { playlist: SongData[]; currentPlayIndex: number };
    }
  | { type: "playStatus"; data: { isPlaying: boolean } }
  | { type: "seekCommitted"; data: { requestId?: number | null; position: number } }
  | {
      type: "seekFailed";
      data: { requestId?: number | null; position: number; error: string };
    }
  | { type: "loadError"; data: { error: string } }
  | { type: "playError"; data: { error: string } }
  | { type: "volumeChanged"; data: { volume: number } }
  | {
      type: "audioOutputChanged";
      data: {
        deviceName: string;
        isDefault: boolean;
        channels: number;
        sampleRate: number;
        sampleFormat: string;
      };
    }
  | { type: "audioOutputError"; data: { error: string; recoverable: boolean } }
  | { type: "fftData"; data: { data: number[] } }
  | { type: "lowFrequencyVolume"; data: { volume: number } }
  | { type: "automixStatus"; data: { status: AutoMixNativeStatus } }
  | {
      type: "automixAnalysisReady";
      data: { currentId: string; nextId: string; transitionId?: number | null };
    }
  | {
      type: "automixCrossfadeStarted";
      data: { fromId: string; toId: string; duration: number; transitionId?: number | null };
    }
  | {
      type: "automixCrossfadeComplete";
      data: {
        currentIndex: number;
        musicId?: string;
        position?: number;
        duration?: number;
        transitionId?: number | null;
      };
    }
  | { type: "automixError"; data: { error: string; recoverable: boolean } };

export type AutoMixNativeState =
  | "idle"
  | "preparing"
  | "waiting"
  | "crossfading"
  | "finishing"
  | "failed";

export interface AutoMixNativeStatus {
  state: AutoMixNativeState;
  enabled: boolean;
  transitionId?: number | null;
  currentIndex: number;
  nextIndex?: number | null;
  currentId?: string | null;
  nextId?: string | null;
  crossfadeStart?: number | null;
  crossfadeDuration?: number | null;
  error?: string | null;
}

// ═══════════════════════════════════════════════════════════════════
//  Query types (sync reads, no round-trip through message loop)
// ═══════════════════════════════════════════════════════════════════

export interface AudioStateResponse {
  state: "stopped" | "playing" | "paused" | "ended";
  is_playing: boolean;
  position: number;
  duration: number;
}

// ═══════════════════════════════════════════════════════════════════
//  Playback timeline sync
// ═══════════════════════════════════════════════════════════════════

const DEFAULT_SEEK_POSITION_GUARD_MS = 2500;
const DEFAULT_ATOMIC_SEEK_GUARD_MS = 5000;
const DEFAULT_SEEK_POSITION_ACCEPT_EPSILON_SECONDS = 0.35;
const DEFAULT_SEEK_POSITION_FORWARD_TOLERANCE_SECONDS = 0.2;
const DEFAULT_SMOOTHING_BACKSTEP_TOLERANCE_SECONDS = 0.2;

export interface AudioTimelineSyncOptions {
  seekGuardMs?: number;
  atomicSeekGuardMs?: number;
  acceptEpsilonSeconds?: number;
  forwardToleranceSeconds?: number;
  smoothingBackstepToleranceSeconds?: number;
  nowMs?: () => number;
}

export interface AudioTimelineSetPositionOptions {
  guardSeek?: boolean;
  requestId?: number;
  atomicSeek?: boolean;
}

interface AudioTimelineSeekAnchor {
  position: number;
  previousPosition: number;
  requestedAt: number;
  guardUntil: number;
  requestId?: number;
  atomicSeek: boolean;
}

/**
 * Small, allocation-light timeline reconciler shared by native and web-backed
 * transports. It keeps the UI clock optimistic after seeks, rejects delayed
 * backend position packets that belong to the pre-seek timeline, and smooths
 * tiny heartbeat regressions without hiding real backwards seeks.
 */
export class AudioTimelineSync {
  private _position = 0;
  private _duration = 0;
  private _isPlaying = false;
  private _pendingPlayCommand = false;
  private _lastPositionEvent: { position: number; receivedAt: number } | null = null;
  private _smoothedPosition: number | null = null;
  private _pendingSeekAnchor: AudioTimelineSeekAnchor | null = null;

  private readonly _seekGuardMs: number;
  private readonly _atomicSeekGuardMs: number;
  private readonly _acceptEpsilonSeconds: number;
  private readonly _forwardToleranceSeconds: number;
  private readonly _smoothingBackstepToleranceSeconds: number;
  private readonly _nowMs: () => number;

  constructor(options: AudioTimelineSyncOptions = {}) {
    this._seekGuardMs = options.seekGuardMs ?? DEFAULT_SEEK_POSITION_GUARD_MS;
    this._atomicSeekGuardMs = options.atomicSeekGuardMs ?? DEFAULT_ATOMIC_SEEK_GUARD_MS;
    this._acceptEpsilonSeconds =
      options.acceptEpsilonSeconds ?? DEFAULT_SEEK_POSITION_ACCEPT_EPSILON_SECONDS;
    this._forwardToleranceSeconds =
      options.forwardToleranceSeconds ?? DEFAULT_SEEK_POSITION_FORWARD_TOLERANCE_SECONDS;
    this._smoothingBackstepToleranceSeconds =
      options.smoothingBackstepToleranceSeconds ?? DEFAULT_SMOOTHING_BACKSTEP_TOLERANCE_SECONDS;
    this._nowMs = options.nowMs ?? (() => Date.now());
  }

  get position(): number {
    return this._position;
  }

  get duration(): number {
    return this._duration;
  }

  setDuration(duration: number): void {
    this._duration = Number.isFinite(duration) && duration > 0 ? duration : 0;
    if (this._duration > 0 && this._position > this._duration) {
      this.setLocalPosition(this._duration);
    }
  }

  reset(position = 0, duration = this._duration): void {
    this._duration = Number.isFinite(duration) && duration > 0 ? duration : 0;
    this._pendingSeekAnchor = null;
    this._pendingPlayCommand = false;
    this._isPlaying = false;
    this.reanchor(position);
  }

  setPlaybackState(isPlaying: boolean, pendingPlayCommand = false): void {
    const changed =
      this._isPlaying !== isPlaying || this._pendingPlayCommand !== pendingPlayCommand;
    this._isPlaying = isPlaying;
    this._pendingPlayCommand = pendingPlayCommand;
    if (changed) this.reanchor();
  }

  setPendingPlayCommand(pendingPlayCommand: boolean): void {
    this.setPlaybackState(this._isPlaying, pendingPlayCommand);
  }

  reanchor(position = this._position): void {
    const nextPosition = this._normalizePosition(position);
    const now = this._nowMs();
    this._position = nextPosition;
    this._lastPositionEvent = { position: nextPosition, receivedAt: now };
    this._smoothedPosition = nextPosition;
  }

  setLocalPosition(position: number, options: AudioTimelineSetPositionOptions = {}): number {
    const now = this._nowMs();
    const nextPosition = this._normalizePosition(position);
    const previousPosition = this._position;

    this._position = nextPosition;
    this._lastPositionEvent = { position: nextPosition, receivedAt: now };
    this._smoothedPosition = nextPosition;

    if (options.guardSeek) {
      const atomicSeek = options.atomicSeek === true;
      this._pendingSeekAnchor = {
        position: nextPosition,
        previousPosition,
        requestedAt: now,
        guardUntil: now + (atomicSeek ? this._atomicSeekGuardMs : this._seekGuardMs),
        requestId: options.requestId,
        atomicSeek,
      };
    } else {
      this._pendingSeekAnchor = null;
    }

    return nextPosition;
  }

  acceptIncomingPosition(position: number): number {
    const now = this._nowMs();
    const nextPosition = this._normalizePosition(position);
    const anchor = this._pendingSeekAnchor;

    if (anchor) {
      if (now >= anchor.guardUntil) {
        this._pendingSeekAnchor = null;
      } else {
        const expected = this._expectedAnchorPosition(anchor, now);
        if (this._isStaleAgainstSeek(anchor, nextPosition, expected)) {
          return this._position;
        }

        this._applyAcceptedPosition(nextPosition, now, true);
        if (!anchor.atomicSeek || Math.abs(nextPosition - expected) <= this._acceptEpsilonSeconds) {
          this._pendingSeekAnchor = null;
        }
        return this._position;
      }
    }

    this._applyAcceptedPosition(nextPosition, now);
    return this._position;
  }

  commitSeek(requestId: number | null | undefined, position: number): number | null {
    const anchor = this._pendingSeekAnchor;
    if (!this._matchesSeekRequest(requestId)) return null;
    return this.setLocalPosition(position, {
      guardSeek: true,
      requestId: anchor?.requestId,
      atomicSeek: anchor?.atomicSeek,
    });
  }

  rejectSeek(requestId: number | null | undefined): boolean {
    if (!this._matchesSeekRequest(requestId)) return false;
    this._pendingSeekAnchor = null;
    return true;
  }

  readPosition(): number {
    if (this._isPlaying && this._lastPositionEvent) {
      const elapsed = (this._nowMs() - this._lastPositionEvent.receivedAt) / 1000;
      const extrapolated = this._lastPositionEvent.position + Math.max(0, elapsed);
      const clamped = this._duration > 0 ? Math.min(extrapolated, this._duration) : extrapolated;
      const position = this._smoothExtrapolatedPosition(clamped);
      this._position = position;
      return position;
    }

    this._smoothedPosition = this._position;
    return this._position;
  }

  private _applyAcceptedPosition(position: number, now: number, allowBackstep = false): void {
    let nextPosition = position;

    if (!allowBackstep && this._isPlaying && this._position > 0 && position < this._position) {
      const backstep = this._position - position;
      if (backstep > this._smoothingBackstepToleranceSeconds) {
        this._lastPositionEvent = { position: this._position, receivedAt: now };
        this._smoothedPosition = this._position;
        return;
      }
      nextPosition = this._position;
    }

    this._position = nextPosition;
    this._lastPositionEvent = { position: nextPosition, receivedAt: now };
    this._smoothedPosition = nextPosition;
  }

  private _isStaleAgainstSeek(
    anchor: AudioTimelineSeekAnchor,
    position: number,
    expected: number,
  ): boolean {
    const forwardSeek = anchor.position >= anchor.previousPosition;
    if (forwardSeek) {
      if (position < anchor.position - this._forwardToleranceSeconds) return true;
      return position > expected + this._acceptEpsilonSeconds;
    }

    if (position > anchor.position + this._acceptEpsilonSeconds) return true;
    return position < expected - this._acceptEpsilonSeconds;
  }

  private _expectedAnchorPosition(anchor: AudioTimelineSeekAnchor, now: number): number {
    if (!this._isPlaying && !this._pendingPlayCommand) return anchor.position;
    const elapsed = Math.max(0, (now - anchor.requestedAt) / 1000);
    const expected = anchor.position + elapsed;
    return this._duration > 0 ? Math.min(expected, this._duration) : expected;
  }

  private _matchesSeekRequest(requestId: number | null | undefined): boolean {
    const anchor = this._pendingSeekAnchor;
    if (anchor?.requestId !== undefined) return requestId === anchor.requestId;
    return requestId === undefined || requestId === null;
  }

  private _normalizePosition(position: number): number {
    if (!Number.isFinite(position) || position <= 0) return 0;
    return this._duration > 0 ? Math.min(position, this._duration) : position;
  }

  private _smoothExtrapolatedPosition(extrapolated: number): number {
    const last = this._smoothedPosition;
    if (
      last !== null &&
      extrapolated < last &&
      extrapolated >= last - this._smoothingBackstepToleranceSeconds
    ) {
      return last;
    }
    this._smoothedPosition = extrapolated;
    return extrapolated;
  }
}

// ═══════════════════════════════════════════════════════════════════
//  AMLL-style single-message command
// ═══════════════════════════════════════════════════════════════════

let _callbackCounter = 0;
function newCallbackId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  _callbackCounter = (_callbackCounter + 1) >>> 0;
  return `${Date.now()}-${_callbackCounter}`;
}

/**
 * Send an `AudioThreadMessage` to the Rust player.
 * Returns a promise resolved when the invoke round-trip completes
 * (i.e. the message has been handed off to the player thread).
 *
 * Example:
 * ```
 * audioSendMsg({ type: "resumeAudio" });
 * audioSendMsg({ type: "seekAudio", position: 30.5 });
 * audioSendMsg({ type: "setVolume", volume: 0.8 });
 * ```
 */
export function audioSendMsg(msg: AudioThreadMessage): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return window
    .__TAURI__!.core.invoke<void>("audio_send_msg", {
      msg: { callbackId: newCallbackId(), data: msg },
    })
    .catch((err) => {
      console.error("[audioBridge] audio_send_msg failed", msg.type, err);
    });
}

/** Backward-compat alias — both forms now return Promise<void>. */
export const audioSendMsgAsync = audioSendMsg;

// ═══════════════════════════════════════════════════════════════════
//  Sync query command
// ═══════════════════════════════════════════════════════════════════

export async function audioPreheat(): Promise<void> {
  await invoke("audio_preheat");
}

export async function audioGetState(): Promise<AudioStateResponse | null> {
  return invoke<AudioStateResponse>("audio_get_state");
}

// ═══════════════════════════════════════════════════════════════════
//  Session-based event polling (kept for backward compat during migration)
// ═══════════════════════════════════════════════════════════════════

export async function audioSetSession(sessionId: number): Promise<void> {
  await invoke("audio_set_session", { sessionId });
}

export async function audioPollEvents(sessionId: number): Promise<AudioThreadEvent[]> {
  return (await invoke<AudioThreadEvent[]>("audio_poll_events", { sessionId })) ?? [];
}

// ═══════════════════════════════════════════════════════════════════
//  Tauri event listener
// ═══════════════════════════════════════════════════════════════════

const EVENT_CHANNEL = "audio-player://event";

export type AudioThreadEventCallback = (event: AudioThreadEvent, seq?: number) => void;

/**
 * Listen for push events from the Rust backend over the global
 * `audio-player://event` emit stream. This is the fallback path used when the
 * primary `Channel` transport (see `audioIpc.ts`) is unavailable; the Rust
 * forwarder emits here whenever no channel is registered.
 * Returns an unlisten function.
 *
 * The Rust side wraps each event in an `AudioThreadEventMessage<AudioThreadEvent>`
 * envelope (with `callbackId` + `data` + `seq`). Some envelopes carry only an
 * ack (data is null) — we filter those out here so consumers see only real
 * events. The envelope's `seq` is forwarded as the second handler argument
 * so consumers can dedup against duplicate deliveries from the primary
 * Channel transport during a fallback transition.
 */
export async function listenPlayerEvents(handler: AudioThreadEventCallback): Promise<() => void> {
  if (!isTauri()) {
    return () => {};
  }
  return window.__TAURI__!.event.listen<AudioThreadEventMessage<AudioThreadEvent>>(
    EVENT_CHANNEL,
    (e) => {
      const payload = e.payload;
      if (payload && payload.data) {
        handler(payload.data, payload.seq);
      }
    },
  );
}
