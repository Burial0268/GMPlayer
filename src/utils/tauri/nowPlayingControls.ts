import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isTauri } from "./windowManager";

const PLUGIN = "now-playing-controls";
const MEDIA_ACTION_EVENT = "now-playing-controls:media-action";

export interface NowPlayingStateRequest {
  title?: string;
  artist?: string;
  album?: string;
  isPlaying?: boolean;
  playbackState?: "playing" | "paused" | "buffering" | "stopped";
  position?: number;
  duration?: number;
  artworkUrl?: string;
  trackId?: number;
  playbackRate?: number;
  volume?: number;
}

export interface NowPlayingTimelineRequest {
  position: number;
  duration?: number;
  seeked?: boolean;
}

export interface NowPlayingPlayModeRequest {
  mode: "normal" | "random" | "single";
}

export interface NowPlayingActionPayload {
  action:
    | "play"
    | "pause"
    | "next"
    | "previous"
    | "stop"
    | "seek"
    | "toggleShuffle"
    | "toggleRepeat"
    | "setRate"
    | "setVolume";
  position?: number;
  rate?: number;
  volume?: number;
}

async function call<T = void>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T | undefined> {
  if (!isTauri()) return undefined;
  try {
    return await invoke<T>(`plugin:${PLUGIN}|${command}`, args);
  } catch (err) {
    console.warn(`[NowPlayingControls] "${command}" failed:`, err);
    return undefined;
  }
}

function msToSecs(value: number | undefined): number | undefined {
  return typeof value === "number" ? value / 1_000 : undefined;
}

export function initializeNowPlayingControls(): Promise<void | undefined> {
  return call("initialize");
}

export function updateNowPlayingState(req: NowPlayingStateRequest): Promise<void | undefined> {
  return call("update_state", {
    payload: {
      title: req.title,
      artist: req.artist,
      album: req.album,
      artworkUrl: req.artworkUrl,
      trackId: req.trackId,
      isPlaying: req.isPlaying,
      playbackState: req.playbackState,
      position: msToSecs(req.position),
      duration: msToSecs(req.duration),
      playbackRate: req.playbackRate,
      volume: req.volume,
    },
  });
}

export function updateNowPlayingTimeline(
  req: NowPlayingTimelineRequest,
): Promise<void | undefined> {
  return call("update_timeline", {
    payload: {
      position: msToSecs(req.position) ?? 0,
      duration: msToSecs(req.duration),
      seeked: req.seeked,
    },
  });
}

export function updateNowPlayingPlayMode(
  req: NowPlayingPlayModeRequest,
): Promise<void | undefined> {
  return call("update_play_mode", {
    payload: {
      isShuffling: req.mode === "random",
      repeatMode: req.mode === "single" ? "track" : "list",
    },
  });
}

export function setNowPlayingEnabled(enabled: boolean): Promise<void | undefined> {
  return call("set_enabled", { enabled });
}

export function clearNowPlayingControls(): Promise<void | undefined> {
  return call("clear");
}

export async function listenNowPlayingAction(
  handler: (payload: NowPlayingActionPayload) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};

  try {
    const unlisten = await listen<NowPlayingActionPayload>(MEDIA_ACTION_EVENT, (event) => {
      handler(event.payload);
    });
    return unlisten;
  } catch (err) {
    console.warn("[NowPlayingControls] listenNowPlayingAction failed:", err);
    return () => {};
  }
}
