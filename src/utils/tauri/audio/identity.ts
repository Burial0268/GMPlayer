/**
 * Runtime helpers for `TrackIdentity`.
 *
 * Kept out of `./protocol` deliberately — that barrel is types-only so either
 * transport can import it for free. These are the only two operations the
 * frontend needs, and both must stay byte-identical to Rust's
 * `TrackIdentity::key()` in `src-tauri/crates/audio-backend/src/types.rs`:
 * reconciliation across a WebView reload compares these strings.
 */
import type { TrackIdentity } from "./protocol";

/** Canonical string form. MUST match Rust's `TrackIdentity::key()`. */
export const trackIdentityKey = (identity: TrackIdentity | null | undefined): string | null => {
  if (!identity) return null;
  switch (identity.provider) {
    case "netease":
      return identity.id ? `netease:${identity.id}` : null;
    case "local":
      return identity.path ? `local-file:${identity.path}` : null;
    default:
      return null;
  }
};

/** Whether two identities denote the same track. `null` never matches. */
export const sameTrackIdentity = (
  a: TrackIdentity | null | undefined,
  b: TrackIdentity | null | undefined,
): boolean => {
  const keyA = trackIdentityKey(a);
  if (keyA === null) return false;
  return keyA === trackIdentityKey(b);
};

/** Identity for a store song, or `null` when it is not plannable. */
export const identityForSongId = (songId: unknown): TrackIdentity | null => {
  const id = Number(songId);
  if (!Number.isFinite(id) || id <= 0) return null;
  return { provider: "netease", id: String(id) };
};
