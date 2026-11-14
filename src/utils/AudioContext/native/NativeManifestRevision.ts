/**
 * The frontend's copy of the monotonic manifest revision.
 *
 * A leaf module on purpose: the publisher advances it and the order subscriber
 * reads it, and each needs the other's behaviour in the same file otherwise.
 * Holding the one piece of shared state here keeps them from importing each
 * other.
 */

const REVISION_STORAGE_KEY = "gmplayer:nativeManifestRevision";

/**
 * Monotonic manifest revision. The backend rejects anything not strictly newer.
 *
 * This MUST survive a WebView reload: the Rust `Player` lives for the whole
 * process, so a module-scoped counter restarting at 1 while the backend holds
 * (say) 12 would make it reject every publish — and every clear — for the rest
 * of the session, with the planner still advancing the pre-reload list.
 * Persisting it keeps the sequence monotonic across reloads and restarts.
 */
let revision = (() => {
  try {
    const stored = Number(localStorage.getItem(REVISION_STORAGE_KEY));
    return Number.isFinite(stored) && stored > 0 ? stored : 0;
  } catch {
    return 0;
  }
})();

const persist = (): void => {
  try {
    localStorage.setItem(REVISION_STORAGE_KEY, String(revision));
  } catch {
    /* quota/unavailable — in-memory monotonicity still holds this session */
  }
};

/** Revision of the newest manifest published, or adopted from the backend. */
export const getManifestRevision = (): number => revision;

/** Claim the next revision for a manifest about to be published. */
export const bumpManifestRevision = (): number => {
  revision += 1;
  persist();
  return revision;
};

/** Move up to a revision the backend holds. Returns whether it moved. */
export const raiseManifestRevision = (backendRevision: number): boolean => {
  if (!Number.isFinite(backendRevision) || backendRevision <= revision) return false;
  revision = backendRevision;
  persist();
  return true;
};
