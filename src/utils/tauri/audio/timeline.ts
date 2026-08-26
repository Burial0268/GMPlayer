/**
 * Playback timeline reconciliation.
 *
 * Pure logic — no Tauri, no DOM. Shared by the native and web-backed
 * transports so both resolve the same race: a seek moves the UI clock
 * immediately, but position packets already in flight still describe the
 * pre-seek timeline and must not be allowed to rewind it.
 */

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

  /**
   * Adopt the backend's anchor for a track it has just loaded.
   *
   * Every other guard on this type is an *intra-track* heuristic: the seek
   * anchor rejects packets that describe the pre-seek timeline, and
   * `_applyAcceptedPosition`'s backstep tolerance refuses a rewind nothing
   * asked for. Neither means anything across a track boundary — a fresh track
   * legitimately starts behind the one it replaced, which as a position packet
   * is indistinguishable from a stale one, so it was rejected and the clock
   * kept extrapolating the *previous* track's elapsed time into the new one.
   *
   * Unlike `reset` this keeps the playback state: the track changed, the
   * transport did not.
   */
  beginTrack(position: number, duration = this._duration): number {
    this._duration = Number.isFinite(duration) && duration > 0 ? duration : 0;
    this._pendingSeekAnchor = null;
    this.reanchor(position);
    return this._position;
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
