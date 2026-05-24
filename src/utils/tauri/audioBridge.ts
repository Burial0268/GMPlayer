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
  | { type: "seekAudio"; position: number }
  | { type: "jumpToSong"; songIndex: number }
  | { type: "jumpToSongAt"; songIndex: number; position: number }
  | { type: "prevSong" }
  | { type: "nextSong" }
  | { type: "nextSongGapless" }
  | { type: "setPlaylist"; songs: SongData[] }
  | { type: "setVolume"; volume: number }
  | { type: "setVolumeRelative"; volume: number }
  | { type: "setAudioOutput"; name: string }
  | { type: "setFFT"; enabled: boolean }
  | { type: "setFFTRange"; fromFreq: number; toFreq: number }
  | { type: "syncStatus" }
  | { type: "close" }
  | { type: "setMediaControlsEnabled"; enabled: boolean };

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
   * Monotonic sequence number stamped by the Rust event forwarder. Both
   * the WebSocket transport and the Tauri event channel deliver the same
   * event with the same `seq`; subscribers use it to drop duplicates that
   * arrive via the secondary transport (otherwise state-flip dedup breaks
   * on Pause → Seek → Resume bursts and similar patterns).
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
  | { type: "loadError"; data: { error: string } }
  | { type: "playError"; data: { error: string } }
  | { type: "volumeChanged"; data: { volume: number } }
  | { type: "fftData"; data: { data: number[] } }
  | { type: "lowFrequencyVolume"; data: { volume: number } };

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
 * Listen for push events from the Rust backend.
 * Returns an unlisten function.
 *
 * The Rust side wraps each event in an `AudioThreadEventMessage<AudioThreadEvent>`
 * envelope (with `callbackId` + `data` + `seq`). Some envelopes carry only an
 * ack (data is null) — we filter those out here so consumers see only real
 * events. The envelope's `seq` is forwarded as the second handler argument
 * so consumers can dedup against duplicate deliveries from the WebSocket
 * transport.
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
