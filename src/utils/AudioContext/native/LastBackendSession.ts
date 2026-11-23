/**
 * Where a cold boot resumes from the backend's own record of what it last
 * played (`last_session_get`, see `src-tauri/src/last_session.rs`). A leaf so
 * the decision is testable without the store.
 */
import type { TrackIdentity } from "@/utils/tauri/audio/protocol";

export interface LastBackendSession {
  identity: TrackIdentity;
  position: number;
  duration: number;
  savedAt: number;
}

type PlayMode = "normal" | "random" | "single";

/** See `LastQueue` in `last_session.rs`. */
export interface LastBackendQueue {
  playMode: PlayMode | null;
  /** A permutation the backend drew while the page could not mirror it. */
  order: { manifestRevision: number; order: TrackIdentity[]; shuffleId: string | null } | null;
}

/** What `last_session_get` answers. */
export interface LastBackendResume {
  track: LastBackendSession | null;
  queue: LastBackendQueue;
}

/**
 * How to bring the queue in line with the backend's record, before the track
 * is located (mirroring an order moves the cursor's row). An order only exists
 * for random mode; one without it is ignored rather than applied to a mode that
 * does not shuffle.
 */
export const planQueueAdoption = (
  queue: LastBackendQueue | null | undefined,
): {
  mode: PlayMode | null;
  backendOrdered: boolean;
  order: LastBackendQueue["order"];
} => {
  const mode = queue?.playMode ?? null;
  const order = mode === "random" && queue?.order?.order.length ? queue.order : null;
  return { mode, backendOrdered: order !== null, order };
};

interface QueueRow {
  id?: number | string;
  local?: { uri?: string };
}

/** Whether a queue row is the track a backend identity names. */
export const rowMatches = (row: QueueRow | undefined, identity: TrackIdentity): boolean =>
  identity.provider === "netease"
    ? String(row?.id) === identity.id
    : row?.local?.uri === identity.path;

/** Store row for a backend identity: a Netease id, or a local file's locator. */
export const findQueueIndex = (rows: readonly QueueRow[], identity: TrackIdentity): number =>
  rows.findIndex((row) => rowMatches(row, identity));

/**
 * The row and clock to resume, or `null` to keep the page's own.
 *
 * The record wins over the page's cursor, which is stale whenever the backend
 * advanced while the page was frozen — including within the same track, which
 * kept playing past the position the page saved. When the queue lists the track
 * more than once, the row the page is already on is preferred.
 */
export const pickResume = (
  last: LastBackendSession | null,
  rows: readonly QueueRow[],
  currentIndex: number,
): { index: number; position: number; duration: number } | null => {
  if (!last?.identity) return null;
  const index = rowMatches(rows[currentIndex], last.identity)
    ? currentIndex
    : findQueueIndex(rows, last.identity);
  if (index < 0) return null;
  const finite = (value: number) => (Number.isFinite(value) && value > 0 ? value : 0);
  const duration = finite(last.duration);
  const position = finite(last.position);
  return { index, position: duration > 0 ? Math.min(position, duration) : position, duration };
};
