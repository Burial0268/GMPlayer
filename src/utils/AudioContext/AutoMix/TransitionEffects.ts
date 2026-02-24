/**
 * TransitionEffects — Reverb tail and noise riser for crossfade transitions
 *
 * Reverb tail: Programmatic convolution reverb applied to outgoing track
 *   - Generates noise*exp(-decay*t) IR (no external files)
 *   - Connected in PARALLEL from outgoingGain (preserves spectral EQ chain)
 *   - Audio graph: outgoingGain → ConvolverNode → reverbGain → destination
 *
 * Noise riser: Subtle white noise sweep before incoming track
 *   - White noise → bandpass filter (frequency sweep 200→2000Hz) → riserGain → destination
 *   - Very subtle: -24dB → -12dB amplitude envelope
 *   - Plays 1-2s before incoming track, beat-synced if BPM available
 */

import { AudioContextManager } from '../AudioContextManager';

const IS_DEV = import.meta.env?.DEV ?? false;

export class TransitionEffects {
  // Reverb tail nodes
  private _convolver: ConvolverNode | null = null;
  private _reverbGain: GainNode | null = null;
  private _reverbConnectedTo: GainNode | null = null;

  // Noise riser nodes
  private _noiseSource: AudioBufferSourceNode | null = null;
  private _noiseFilter: BiquadFilterNode | null = null;
  private _riserGain: GainNode | null = null;

  // Filter sweep nodes
  private _outSweepFilter: BiquadFilterNode | null = null;
  private _inSweepFilter: BiquadFilterNode | null = null;
  private _outSweepGainRef: GainNode | null = null;
  private _inSweepGainRef: GainNode | null = null;
  private _sweepTargets: { outCutoff: number; inStart: number; duration: number } | null = null;

  // Tracking state
  private _isActive: boolean = false;
  private _isPaused: boolean = false;

  /**
   * Create and schedule a reverb tail on the outgoing track.
   * The reverb is connected in parallel from outgoingGain, preserving the spectral EQ chain.
   *
   * @param ctx - AudioContext
   * @param outgoingGain - GainNode of the outgoing track
   * @param decayTime - Reverb decay time in seconds (1.5-3.0)
   * @param startTime - AudioContext time when reverb should begin
   * @param crossfadeDuration - Total crossfade duration (reverb fades out over last 20%)
   */
  createReverbTail(
    ctx: AudioContext,
    outgoingGain: GainNode,
    decayTime: number,
    startTime: number,
    crossfadeDuration: number
  ): void {
    // Clamp decay time
    decayTime = Math.max(1.5, Math.min(3.0, decayTime));

    // Generate programmatic impulse response: noise * exp(-decay * t)
    const irLength = Math.ceil(ctx.sampleRate * decayTime);
    const irBuffer = ctx.createBuffer(2, irLength, ctx.sampleRate);

    for (let channel = 0; channel < 2; channel++) {
      const channelData = irBuffer.getChannelData(channel);
      for (let i = 0; i < irLength; i++) {
        const t = i / ctx.sampleRate;
        const envelope = Math.exp(-3 * t / decayTime); // -3 for ~95% decay at decayTime
        channelData[i] = (Math.random() * 2 - 1) * envelope;
      }
    }

    // Create convolver and gain nodes
    this._convolver = ctx.createConvolver();
    this._convolver.buffer = irBuffer;

    this._reverbGain = ctx.createGain();
    this._reverbGain.gain.setValueAtTime(0.15, startTime); // subtle reverb

    // Connect in parallel: outgoingGain → convolver → reverbGain → destination
    outgoingGain.connect(this._convolver);
    this._convolver.connect(this._reverbGain);
    this._reverbGain.connect(ctx.destination);
    this._reverbConnectedTo = outgoingGain;

    // Schedule reverb gain: hold at 80%, fade in last 20%
    const holdEnd = startTime + crossfadeDuration * 0.8;
    const fadeEnd = startTime + crossfadeDuration;
    this._reverbGain.gain.setValueAtTime(0.15, holdEnd);
    this._reverbGain.gain.linearRampToValueAtTime(0, fadeEnd);

    this._isActive = true;

    if (IS_DEV) {
      console.log(
        `TransitionEffects: Reverb tail created — decay=${decayTime.toFixed(1)}s, ` +
        `duration=${crossfadeDuration.toFixed(1)}s`
      );
    }
  }

  /**
   * Create and schedule a DJ-style filter sweep: outgoing LPF + incoming HPF.
   * Partitions the frequency spectrum so outgoing "recedes" (muffled) while incoming "fills in".
   *
   * Audio graph after setup:
   *   Outgoing: gainNode → LPF(20kHz→cutoff) → destination
   *   Incoming: gainNode → HPF(startCutoff→20Hz) → destination
   *
   * @param ctx - AudioContext
   * @param outgoingGain - GainNode of the outgoing track
   * @param incomingGain - GainNode of the incoming track
   * @param intensity - 0-1, controls cutoff aggressiveness
   * @param startTime - AudioContext time when sweep should begin
   * @param duration - Total crossfade duration
   * @param fadeInOnly - If true, halve outgoing LPF intensity (song already declining)
   */
  createFilterSweep(
    ctx: AudioContext,
    outgoingGain: GainNode,
    incomingGain: GainNode,
    intensity: number,
    startTime: number,
    duration: number,
    fadeInOnly: boolean
  ): void {
    // Guard: clean up any previous filter sweep nodes to prevent orphaned graph nodes
    if (this._outSweepFilter || this._inSweepFilter) {
      if (IS_DEV) {
        console.warn('TransitionEffects: Previous filter sweep still active, cleaning up first');
      }
      if (this._outSweepFilter) {
        try { this._outSweepFilter.disconnect(); } catch { /* ok */ }
        if (this._outSweepGainRef) {
          try { this._outSweepGainRef.disconnect(this._outSweepFilter); } catch { /* ok */ }
          try { this._outSweepGainRef.connect(ctx.destination); } catch { /* ok */ }
        }
        this._outSweepFilter = null;
      }
      if (this._inSweepFilter) {
        try { this._inSweepFilter.disconnect(); } catch { /* ok */ }
        if (this._inSweepGainRef) {
          try { this._inSweepGainRef.disconnect(this._inSweepFilter); } catch { /* ok */ }
          try { this._inSweepGainRef.connect(ctx.destination); } catch { /* ok */ }
        }
        this._inSweepFilter = null;
      }
      this._outSweepGainRef = null;
      this._inSweepGainRef = null;
      this._sweepTargets = null;
    }
    // For fadeInOnly, halve the outgoing filter intensity
    const outIntensity = fadeInOnly ? intensity * 0.5 : intensity;

    // Outgoing LPF: sweeps from 20kHz down to cutoff (making outgoing "recede")
    // intensity 0 → cutoff 2000Hz, intensity 1 → cutoff 400Hz
    const outCutoff = 2000 - outIntensity * 1600; // lerp(2000, 400, outIntensity)

    this._outSweepFilter = ctx.createBiquadFilter();
    this._outSweepFilter.type = 'lowpass';
    this._outSweepFilter.Q.value = 0.707; // Butterworth: flat passband
    this._outSweepFilter.frequency.setValueAtTime(20000, startTime);
    this._outSweepFilter.frequency.exponentialRampToValueAtTime(outCutoff, startTime + duration);

    // Insert LPF inline: outgoingGain → LPF → destination
    // Note: outgoingGain already connects to destination via CrossfadeScheduler.
    // We insert by connecting outgoingGain → LPF → destination and disconnecting
    // outgoingGain's direct connection to destination.
    try { outgoingGain.disconnect(ctx.destination); } catch { /* may not be directly connected */ }
    outgoingGain.connect(this._outSweepFilter);
    this._outSweepFilter.connect(ctx.destination);
    this._outSweepGainRef = outgoingGain;

    // Incoming HPF: sweeps from startCutoff down to 20Hz (letting incoming "fill in")
    // intensity 0 → startCutoff 300Hz, intensity 1 → startCutoff 1200Hz
    const inStart = 300 + intensity * 900; // lerp(300, 1200, intensity)

    this._inSweepFilter = ctx.createBiquadFilter();
    this._inSweepFilter.type = 'highpass';
    this._inSweepFilter.Q.value = 0.707;
    this._inSweepFilter.frequency.setValueAtTime(inStart, startTime);
    this._inSweepFilter.frequency.exponentialRampToValueAtTime(20, startTime + duration);

    // Insert HPF inline: incomingGain → HPF → destination
    try { incomingGain.disconnect(ctx.destination); } catch { /* may not be directly connected */ }
    incomingGain.connect(this._inSweepFilter);
    this._inSweepFilter.connect(ctx.destination);
    this._inSweepGainRef = incomingGain;

    // Store targets for pause/resume
    this._sweepTargets = { outCutoff, inStart, duration };

    this._isActive = true;

    if (IS_DEV) {
      console.log(
        `TransitionEffects: Filter sweep created — ` +
        `outLPF: 20kHz→${outCutoff.toFixed(0)}Hz, ` +
        `inHPF: ${inStart.toFixed(0)}Hz→20Hz, ` +
        `intensity=${intensity.toFixed(2)}, ` +
        `duration=${duration.toFixed(1)}s` +
        (fadeInOnly ? ' (fadeInOnly, halved out intensity)' : '')
      );
    }
  }

  /**
   * Create and schedule a noise riser before the incoming track.
   * Subtle white noise sweep with bandpass filter frequency sweep.
   *
   * @param ctx - AudioContext
   * @param duration - Riser duration in seconds (1.0-2.0)
   * @param startTime - AudioContext time when riser should begin
   * @param bpm - Optional BPM for beat-synced duration
   * @param targetFreq - Optional target frequency for filter sweep end (default 2000Hz)
   */
  createNoiseRiser(
    ctx: AudioContext,
    duration: number,
    startTime: number,
    bpm?: number,
    targetFreq: number = 2000
  ): void {
    // Beat-sync duration if BPM available
    if (bpm && bpm > 0) {
      const beatDuration = 60 / bpm;
      const beats = Math.round(duration / beatDuration);
      duration = Math.max(beatDuration, beats * beatDuration);
    }
    duration = Math.max(1.0, Math.min(2.0, duration));

    // Create white noise buffer
    const noiseLength = Math.ceil(ctx.sampleRate * duration);
    const noiseBuffer = ctx.createBuffer(1, noiseLength, ctx.sampleRate);
    const noiseData = noiseBuffer.getChannelData(0);
    for (let i = 0; i < noiseLength; i++) {
      noiseData[i] = Math.random() * 2 - 1;
    }

    // Create noise source
    this._noiseSource = ctx.createBufferSource();
    this._noiseSource.buffer = noiseBuffer;

    // Create bandpass filter with frequency sweep
    this._noiseFilter = ctx.createBiquadFilter();
    this._noiseFilter.type = 'bandpass';
    this._noiseFilter.Q.value = 2; // moderate resonance
    this._noiseFilter.frequency.setValueAtTime(200, startTime);
    this._noiseFilter.frequency.exponentialRampToValueAtTime(targetFreq, startTime + duration);

    // Create gain envelope: -24dB → -12dB
    this._riserGain = ctx.createGain();
    const startGain = Math.pow(10, -24 / 20); // ~0.063
    const endGain = Math.pow(10, -12 / 20);   // ~0.25
    this._riserGain.gain.setValueAtTime(startGain, startTime);
    this._riserGain.gain.exponentialRampToValueAtTime(endGain, startTime + duration * 0.9);
    this._riserGain.gain.linearRampToValueAtTime(0, startTime + duration);

    // Connect: noiseSource → filter → gain → destination
    this._noiseSource.connect(this._noiseFilter);
    this._noiseFilter.connect(this._riserGain);
    this._riserGain.connect(ctx.destination);

    // Schedule playback
    this._noiseSource.start(startTime);
    this._noiseSource.stop(startTime + duration);

    this._isActive = true;

    if (IS_DEV) {
      console.log(
        `TransitionEffects: Noise riser created — duration=${duration.toFixed(1)}s, ` +
        `sweep=200→${targetFreq}Hz` +
        (bpm ? `, beat-synced to ${bpm} BPM` : '')
      );
    }
  }

  /**
   * Pause all active effects.
   * Note: ConvolverNode doesn't support pausing directly —
   * we reduce gain to 0 as a workaround.
   */
  pause(ctx: AudioContext): void {
    if (!this._isActive || this._isPaused) return;
    this._isPaused = true;

    const now = ctx.currentTime;
    if (this._reverbGain) {
      this._reverbGain.gain.cancelScheduledValues(now);
      this._reverbGain.gain.setValueAtTime(0, now);
    }
    if (this._riserGain) {
      this._riserGain.gain.cancelScheduledValues(now);
      this._riserGain.gain.setValueAtTime(0, now);
    }
    // Freeze filter sweep automation
    if (this._outSweepFilter) {
      this._outSweepFilter.frequency.cancelScheduledValues(now);
      // Hold at current value (freeze)
    }
    if (this._inSweepFilter) {
      this._inSweepFilter.frequency.cancelScheduledValues(now);
    }
  }

  /**
   * Resume paused effects.
   * Restores gain levels (approximate — scheduled automation is lost).
   */
  resume(ctx: AudioContext): void {
    if (!this._isActive || !this._isPaused) return;
    this._isPaused = false;

    const now = ctx.currentTime;
    if (this._reverbGain) {
      this._reverbGain.gain.setValueAtTime(0.15, now);
    }
    if (this._riserGain) {
      // Resume at a mid-level gain (exact scheduled ramp is lost)
      const midGain = Math.pow(10, -18 / 20); // ~0.125
      this._riserGain.gain.setValueAtTime(midGain, now);
    }
    // Re-ramp filter sweep from current position to target
    if (this._sweepTargets) {
      const remainDuration = this._sweepTargets.duration * 0.5; // approximate remaining
      if (this._outSweepFilter) {
        this._outSweepFilter.frequency.exponentialRampToValueAtTime(
          this._sweepTargets.outCutoff, now + remainDuration
        );
      }
      if (this._inSweepFilter) {
        this._inSweepFilter.frequency.exponentialRampToValueAtTime(
          20, now + remainDuration
        );
      }
    }
  }

  /**
   * Clean up all effect nodes, disconnecting from the audio graph.
   */
  cleanup(): void {
    // Get AudioContext for reconnecting gain nodes after sweep filter removal
    const ctx = AudioContextManager.getContext();

    if (this._convolver) {
      try { this._convolver.disconnect(); } catch { /* ok */ }
      this._convolver = null;
    }
    if (this._reverbGain) {
      try { this._reverbGain.disconnect(); } catch { /* ok */ }
      this._reverbGain = null;
    }
    // Disconnect reverb parallel connection from outgoing gain
    // (don't disconnect outgoingGain entirely — it's still used by the main chain)
    this._reverbConnectedTo = null;

    if (this._noiseSource) {
      try { this._noiseSource.stop(); } catch { /* already stopped */ }
      try { this._noiseSource.disconnect(); } catch { /* ok */ }
      this._noiseSource = null;
    }
    if (this._noiseFilter) {
      try { this._noiseFilter.disconnect(); } catch { /* ok */ }
      this._noiseFilter = null;
    }
    if (this._riserGain) {
      try { this._riserGain.disconnect(); } catch { /* ok */ }
      this._riserGain = null;
    }

    // Clean up filter sweep: disconnect filters and reconnect gain nodes → destination
    // Note: outgoing gain is NOT reconnected — it's about to be stopped/unloaded
    // by _onCrossfadeComplete. Only the incoming gain needs reconnection.
    if (this._outSweepFilter) {
      try { this._outSweepFilter.disconnect(); } catch { /* ok */ }
      if (this._outSweepGainRef) {
        try { this._outSweepGainRef.disconnect(this._outSweepFilter); } catch { /* ok */ }
        // Don't reconnect outgoing to destination — sound is being destroyed
      }
      this._outSweepFilter = null;
    }
    this._outSweepGainRef = null;

    if (this._inSweepFilter) {
      try { this._inSweepFilter.disconnect(); } catch { /* ok */ }
      if (this._inSweepGainRef && ctx) {
        try { this._inSweepGainRef.disconnect(this._inSweepFilter); } catch { /* ok */ }
        try { this._inSweepGainRef.connect(ctx.destination); } catch { /* ok */ }
      }
      this._inSweepFilter = null;
    }
    this._inSweepGainRef = null;
    this._sweepTargets = null;

    this._isActive = false;
    this._isPaused = false;

    if (IS_DEV) {
      console.log('TransitionEffects: Cleaned up');
    }
  }

  get isActive(): boolean {
    return this._isActive;
  }
}
