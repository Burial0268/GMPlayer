/**
 * Native manifest / planner protocol.
 *
 * The manifest is how the frontend hands the Rust planner an authoritative
 * view of the playlist; the planner then owns advancement and resolution.
 * See `src-tauri/crates/audio-backend/src/player/{manifest,planner}.rs`.
 */

/**
 * Stable track identity — deliberately independent of any resolved CDN URL,
 * which expires and gets re-resolved. All frontend/backend reconciliation
 * happens on this, never on `local:<url>`.
 */
export type TrackIdentity =
  | { provider: "netease"; id: string }
  | { provider: "local"; path: string };

export interface NativeManifestEntry {
  identity: TrackIdentity;
  /** UI-facing index. May be sparse; never used for advancement ordering. */
  playlistIndex: number;
  title?: string | null;
  artist?: string | null;
  /**
   * Album name and cover URL. Display-only for the frontend, but the backend
   * needs both: it drives the OS media session for tracks it advanced to on its
   * own, when no JS runtime is alive to describe them.
   */
  album?: string | null;
  artworkUrl?: string | null;
  durationMs?: number | null;
  /** Netease `fee`, so Rust can apply the same VIP pre-check as the frontend. */
  fee?: number | null;
  /** Cloud-uploaded (`pc`) tracks bypass the VIP pre-check. */
  hasPc?: boolean;
}

export interface NativePlaybackManifest {
  schemaVersion: 1;
  /** Monotonic. The backend rejects anything not strictly newer. */
  revision: number;
  entries: NativeManifestEntry[];
  /**
   * Explicit traversal order as indices into `entries`. Empty means natural
   * order, which is what the frontend now ships for *every* mode: random mode
   * shuffles the playlist itself, so the permutation is the entry order and
   * there is nothing separate to send.
   */
  order: number[];
  cursorIdentity: TrackIdentity | null;
  cursorIndex: number;
  mode: "normal" | "single" | "random";
  repeatList: boolean;
  /**
   * Seed for the one shuffle the backend builds on its own: a play-mode press on
   * the OS notification with no WebView alive (`ManifestStore::set_mode`). The
   * frontend's next publish replaces that order with the playlist's own.
   */
  randomSeed?: number | null;
}

/**
 * Endpoints/credentials the Rust resolver needs to call the same deployed
 * NeteaseCloudMusicApi the frontend uses. Never persisted, never logged.
 */
export interface NativeResolverConfig {
  ncmBaseUrl?: string | null;
  unmBaseUrl?: string | null;
  unmEnabled: boolean;
  cookie?: string | null;
  /**
   * Netease user id of the signed-in account.
   *
   * `/likelist` is keyed by `uid`, and the backend fetches it itself so the
   * notification's heart is correct with no page alive — see
   * `player/session_controls.rs`. `null` means signed out.
   */
  userId?: string | null;
  level?: string | null;
  /**
   * Resolve playback URLs through the in-process protocol layer rather than the
   * deployed API. Mirrors the UI's `ncmTransport` setting so both halves use one
   * session.
   */
  useLocalNcm?: boolean;
}

export type NativePlannerPlaybackState = "stopped" | "loading" | "playing" | "paused" | "ended";

export interface NativePlannerStatus {
  manifestRevision: number;
  cursorIdentity: TrackIdentity | null;
  cursorIndex: number | null;
  playbackState: NativePlannerPlaybackState;
  preparedIdentity: TrackIdentity | null;
  failureCount: number;
  exhausted: boolean;
  /**
   * Whether the planner may choose the next track at all.
   *
   * Distinct from "a manifest is loaded": server-driven modes (personal FM,
   * listen-together) still publish a manifest so the backend can resolve a
   * track it is *told* to play, but must not advance on their own. Optional so
   * an older backend (which always advanced) still reads as enabled.
   */
  enabled?: boolean;
}

/**
 * Authoritative snapshot of the live backend playback session, read through the
 * `audio_get_session` command.
 *
 * Exists because the Rust process outlives the WebView: on Android the page is
 * destroyed and reloaded while playback continues. The reloaded frontend
 * rehydrates from persisted storage — which describes the track that was
 * playing when the app was backgrounded — so it must ask here *before*
 * resolving a URL, or it will replace live playback with a stale track.
 */
export interface NativeSessionSnapshot {
  /** `false` when nothing is loaded; the frontend then owns startup as before. */
  hasTrack: boolean;
  /** Transport id (`local:<url>`). Only for re-seeding a controller, never for reconciliation. */
  musicId: string;
  identity: TrackIdentity | null;
  playlistIndex: number;
  position: number;
  duration: number;
  isPlaying: boolean;
  volume: number;
  /** Manifest revision the backend holds, so the frontend can stay ahead of it. */
  manifestRevision: number;
  plannerActive: boolean;
}
