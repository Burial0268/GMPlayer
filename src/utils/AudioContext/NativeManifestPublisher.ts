/**
 * NativeManifestPublisher — publishes the full playback manifest to the Rust
 * backend so it can plan and resolve track advances entirely on its own.
 *
 * This replaces the bounded-window approach of `NativeQueuePrefill`. The old
 * design pre-resolved N upcoming CDN URLs in JS; that could not survive a
 * frozen Android WebView for longer than the window (and, for random mode, was
 * hard-capped at depth 1 because duplicate `origOrder` entries broke the
 * backend's identity matching). The manifest carries stable *identities* only —
 * no temporary URLs — and the backend resolves each track when it needs it.
 *
 * Division of responsibility:
 * - JS owns policy: list contents, traversal order, play mode, credentials.
 * - Rust owns execution: when to resolve, what to play next, retry on failure.
 *
 * Traversal order is the *playlist* order, in every mode. Random mode shuffles
 * `persistData.playlists` itself (`musicData.shufflePlaylistOrder`) instead of
 * shipping a separate permutation, so the order is persisted with the queue,
 * survives a killed WebView, and a republish can never reshuffle it under the
 * backend mid-pass.
 *
 * Tauri-only. The web backend accepts and ignores these messages (playback is
 * owned by the browser media host and JS is never frozen), so callers do not
 * need to branch — but we still gate here to avoid pointless IPC.
 */

import { isTauri } from "@/utils/tauri/core/runtime";
import type {
  NativeManifestEntry,
  NativePlaybackManifest,
  TrackIdentity,
} from "@/utils/tauri/audio/protocol";
import { NativeRustSound, setPlannerRevisionObserver } from "@/utils/tauri/audio/nativeRustSound";
// Import stores directly to avoid circular dependency through barrel exports
import useMusicDataStore from "@/store/musicData";
import useListenTogetherStore from "@/store/listenTogether";
import type { SongData } from "@/store/musicTypes";
import type { TrackDisplay } from "@/utils/tauri/audio/protocol/messages";

const IS_DEV = import.meta.env?.DEV ?? false;

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

const nextRevision = (): number => {
  revision += 1;
  try {
    localStorage.setItem(REVISION_STORAGE_KEY, String(revision));
  } catch {
    /* quota/unavailable — in-memory monotonicity still holds this session */
  }
  return revision;
};

/** Last published fingerprint, to skip redundant IPC on unrelated store churn. */
let lastFingerprint = "";
let publishScheduled = false;
let pendingForce = false;

/**
 * Adopt a revision the backend reports. Covers the case where our persisted
 * counter is behind the live backend (cleared storage, a different profile):
 * without this the backend would reject everything we publish.
 */
export const reconcileNativeManifestRevision = (backendRevision: number): void => {
  if (!Number.isFinite(backendRevision) || backendRevision <= revision) return;
  revision = backendRevision;
  try {
    localStorage.setItem(REVISION_STORAGE_KEY, String(revision));
  } catch {
    /* best effort */
  }
  // Our last-published state no longer describes what the backend holds.
  lastFingerprint = "";
  plannerEnabled = null;
};

// Adopt the backend's revision as soon as it reports one, so a reload can never
// leave us publishing manifests the backend rejects as stale.
setPlannerRevisionObserver(reconcileNativeManifestRevision);

/**
 * Stable seed for the backend's *own* reshuffle.
 *
 * The traversal order is not seeded from here any more: random mode reorders the
 * store playlist itself (`musicData.shufflePlaylistOrder`), so the manifest ships
 * natural order and both sides just walk positions. What still needs a seed is
 * the one case where the backend has to invent an order — a play-mode press on
 * the OS notification with no WebView alive (`ManifestStore::set_mode`). One seed
 * per session is enough: the backend mixes it with the revision and the pass.
 */
const randomSeed = Math.floor(Math.random() * 0xffffffff);

/**
 * Derive a backend identity from a store song.
 *
 * Two providers. A song carrying `local` is an imported file and its identity is
 * the source locator (an absolute path, or an Android `content://` document
 * URI); everything else is Netease and its identity is the numeric id.
 *
 * The local branch is load-bearing rather than cosmetic: without it a local
 * track is dropped from the manifest, the planner never learns about it, and
 * playback simply stops at the end of the current song whenever the WebView is
 * not alive to drive the JS advance path — which on Android is most of the time.
 * In the foreground the JS fallback hides it completely.
 *
 * Returning `null` means "not plannable" — the manifest omits it rather than
 * shipping an entry the resolver would always fail on.
 */
const toIdentity = (song: SongData): TrackIdentity | null => {
  if (!song) return null;
  const localUri = song.local?.uri;
  if (typeof localUri === "string" && localUri) return { provider: "local", path: localUri };
  const id = Number(song.id);
  if (!Number.isFinite(id) || id <= 0) return null;
  return { provider: "netease", id: String(id) };
};

/**
 * Best-effort track length in milliseconds.
 *
 * The store drops the raw `dt` during normalization (`transformSongData`) and
 * keeps only a formatted `mm:ss` string, so parse that back when no numeric
 * field survived. This is a *hint* for the media session before the track is
 * decoded — the backend replaces it with the real duration on load — so the
 * `mm:ss` wrap past one hour is acceptable here.
 */
const toDurationMs = (song: SongData): number | null => {
  const numeric = Number(song?.dt ?? song?.duration);
  if (Number.isFinite(numeric) && numeric > 0) return Math.round(numeric);

  const time = song?.time;
  if (typeof time !== "string") return null;
  const parts = time.split(":");
  if (parts.length !== 2) return null;
  const minutes = Number(parts[0]);
  const seconds = Number(parts[1]);
  if (!Number.isFinite(minutes) || !Number.isFinite(seconds)) return null;
  const total = (minutes * 60 + seconds) * 1000;
  return total > 0 ? total : null;
};

/** Cover art URL, normalized to https and sized for a media notification. */
const toArtworkUrl = (song: SongData): string | null => {
  // A local track's cover was extracted by the scan and lives on disk. The
  // *backend* is what fetches this (SMTC / MPRIS / the Android notification), so
  // it must be the real path, not the `asset://` URL the WebView uses — nothing
  // outside the webview can resolve that host.
  const coverPath = song?.local?.coverPath;
  if (typeof coverPath === "string" && coverPath) return coverPath;

  const picUrl = song?.album?.picUrl;
  if (typeof picUrl !== "string" || !picUrl) return null;
  return `${picUrl.replace(/^http:/, "https:")}?param=512y512`;
};

/**
 * Display metadata for one track, in the shape the backend's `TrackDisplay`
 * expects.
 *
 * Exported because the *queue* needs it too, not just the manifest: a queued
 * track arrives as `local` with an https path that Rust downloads to a temp
 * file, so without this the media session shows that temp file's random stem
 * until a decoded tag or a `/song/detail` round trip corrects it. Same
 * normalization in both places on purpose — two extractors would drift.
 */
export const toTrackDisplay = (song: SongData): TrackDisplay | undefined => {
  const artist = Array.isArray(song?.artist)
    ? song.artist
        .map((a: any) => a?.name)
        .filter(Boolean)
        .join(" / ") || undefined
    : undefined;
  const display: TrackDisplay = {
    title: song?.name || undefined,
    artist,
    album: song?.album?.name || undefined,
    artworkUrl: toArtworkUrl(song) ?? undefined,
  };
  // Omit entirely when there is nothing to say, so the backend's "no display
  // info" path stays distinguishable from "sent an empty one".
  return display.title || display.artist || display.album || display.artworkUrl
    ? display
    : undefined;
};

/**
 * Last planner gate pushed to the backend, so repeated publishes do not spam
 * IPC. `null` means "never pushed this session" — a WebView reload resets this
 * while the backend keeps its own value, so the first publish after a reload
 * must always send one.
 */
let plannerEnabled: boolean | null = null;

/**
 * Gate whether the backend may pick the next track itself.
 *
 * Manifest and planner are two different powers and must not be conflated:
 * the manifest is "tracks I may be *told* to play", the planner is "may I
 * choose the next one myself". Personal FM and listen-together have their
 * next track decided by a server, so they keep the manifest (the backend
 * still needs identity → resolvable source, e.g. to honour a remote GOTO)
 * but lose the planner.
 */
const applyPlannerGate = (sound: NativeRustSound, enabled: boolean): void => {
  if (plannerEnabled === enabled) return;
  plannerEnabled = enabled;
  sound.setNativePlannerEnabled(enabled);
  if (IS_DEV) console.log(`[NativeManifest] planner ${enabled ? "enabled" : "gated off"}`);
};

/**
 * Publish the current playlist as a manifest.
 *
 * Coalesced: callers fire this from many independent store mutations (adding a
 * song, moving the cursor, changing mode), and a single user action commonly
 * triggers several in one tick. Only the last one in a tick does any work, and
 * `force` is sticky across the batch so a forced publish is never swallowed by
 * a plain one scheduled after it.
 */
export const publishNativeManifest = (options: { force?: boolean } = {}): void => {
  if (!isTauri()) return;
  pendingForce ||= options.force === true;
  if (publishScheduled) return;
  publishScheduled = true;
  // Microtask, not a timer: the manifest still lands within the same task that
  // mutated the store, so nothing can observe a stale backend cursor.
  queueMicrotask(flushNativeManifest);
};

/**
 * Cheap change probe.
 *
 * Runs over the playlist *without allocating* the entry objects, so the common
 * "nothing material moved" case costs one pass and no garbage. Previously the
 * dedup happened only after building every entry (plus a 3-allocation artist
 * join per song) and an N-element fingerprint string — i.e. the expensive part
 * ran even when the result was discarded.
 *
 * Two independent 32-bit hashes are combined rather than one: a collision here
 * means a skipped publish, which would leave the backend planning on a stale
 * list, so ~2^-64 is the right risk level, whereas a single 32-bit hash is not.
 */
const playlistSignature = (
  playlists: SongData[],
  mode: string,
  cursorSongId: number | null | undefined,
): string => {
  let h1 = 0x811c9dc5;
  let h2 = 5381;
  let counted = 0;
  for (let index = 0; index < playlists.length; index++) {
    const song = playlists[index];
    const id = Number(song?.id);
    // Mirror `toIdentity`: unplannable songs are omitted from entries, so they
    // must not contribute to the signature either. A local track *is*
    // plannable despite its negative id — leaving it out here would freeze the
    // signature for an all-local playlist, so edits to it would never publish.
    const localUri = song?.local?.uri;
    const plannable =
      typeof localUri === "string" && localUri ? true : Number.isFinite(id) && id > 0;
    if (!plannable) continue;
    counted++;
    // A local row always carries the Rust-assigned id, but hash the locator's
    // length as a backstop so a row built without one cannot make two different
    // files hash the same.
    const token = Number.isFinite(id) && id !== 0 ? id : (localUri?.length ?? 0) + 1;
    h1 = Math.imul(h1 ^ token, 0x01000193) >>> 0;
    h1 = Math.imul(h1 ^ index, 0x01000193) >>> 0;
    h2 = ((Math.imul(h2, 33) ^ token) >>> 0) + index;
    h2 >>>= 0;
  }
  // No seed term: the traversal order is the playlist order (the index is mixed
  // into both hashes above), so a reshuffle *is* a playlist change.
  return `${mode}|${cursorSongId ?? "?"}|${counted}|${h1.toString(36)}:${h2.toString(36)}`;
};

const flushNativeManifest = (): void => {
  publishScheduled = false;
  const force = pendingForce;
  pendingForce = false;

  const sound = window.$player;
  if (!(sound instanceof NativeRustSound) || sound.isDestroyed()) return;

  const music = useMusicDataStore();
  const listenTogether = useListenTogetherStore();

  const playlists = music.persistData.playlists;
  if (!playlists?.length) {
    clearNativeManifest();
    return;
  }

  // Personal FM and listen-together have their next track chosen by a server.
  // Keep publishing the manifest — the backend still needs to resolve tracks it
  // is told to play — but take away its right to advance on its own. This runs
  // before the dedup check below because the gate can change while the playlist
  // (and therefore the signature) stays identical.
  const serverDrivenOrder = music.persistData.personalFmMode || listenTogether.isInRoom;
  applyPlannerGate(sound, !serverDrivenOrder);

  const entries: NativeManifestEntry[] = [];
  let cursorPosition = -1;
  const cursorSongId = music.playingSongId ?? music.getPlaySongData?.id;

  // Probe before building: bail on the no-op path without allocating entries.
  const signature = playlistSignature(playlists, music.persistData.playSongMode, cursorSongId);
  if (!force && signature === lastFingerprint) return;

  for (let index = 0; index < playlists.length; index++) {
    const song = playlists[index];
    const identity = toIdentity(song);
    if (!identity) continue;
    if (cursorSongId !== null && cursorSongId !== undefined && song.id === cursorSongId) {
      cursorPosition = entries.length;
    }
    entries.push({
      identity,
      playlistIndex: index,
      title: song.name ?? null,
      artist: Array.isArray(song.artist)
        ? song.artist
            .map((a: any) => a?.name)
            .filter(Boolean)
            .join(" / ") || null
        : null,
      album: song.album?.name ?? null,
      artworkUrl: toArtworkUrl(song),
      durationMs: toDurationMs(song),
      fee: Number.isFinite(song.fee) ? Number(song.fee) : null,
      hasPc: song.pc !== null && song.pc !== undefined,
    });
  }

  if (!entries.length) {
    clearNativeManifest();
    return;
  }

  const mode = music.persistData.playSongMode;
  const cursorIdentity = cursorPosition >= 0 ? entries[cursorPosition].identity : null;
  const cursorIndex = cursorPosition >= 0 ? entries[cursorPosition].playlistIndex : 0;

  lastFingerprint = signature;

  const manifest: NativePlaybackManifest = {
    schemaVersion: 1,
    revision: nextRevision(),
    entries,
    // Empty = natural order, for every mode. Random mode's permutation is the
    // playlist itself, so there is no separate order to ship — and nothing for a
    // republish to reshuffle, which is what used to restart the pass (and repeat
    // tracks) every time the Android WebView was killed and came back.
    order: [],
    cursorIdentity,
    cursorIndex,
    // `single` repeats one track; list repeat is the normal/random default.
    mode: mode === "single" ? "single" : mode === "random" ? "random" : "normal",
    repeatList: true,
    randomSeed,
  };

  sound.setNativeManifest(manifest);
  if (IS_DEV) {
    console.log(
      `[NativeManifest] published rev=${manifest.revision}, ${entries.length} entries, ` +
        `mode=${manifest.mode}, cursor=${cursorIndex}`,
    );
  }
};

/**
 * Drop the backend manifest, returning advancement to the JS-driven path.
 *
 * Unconditional: after a WebView reload `lastFingerprint` is empty even though
 * the backend still holds a manifest from the previous session, so guarding on
 * it would leave the planner advancing a stale list (e.g. through personal FM).
 */
export const clearNativeManifest = (): void => {
  if (!isTauri()) return;
  const sound = window.$player;
  if (!(sound instanceof NativeRustSound) || sound.isDestroyed()) return;
  lastFingerprint = "";
  // `ClearNativeManifest` rebuilds the backend `Planner` from scratch, which
  // re-enables it. Drop our cached gate so the next publish always re-asserts.
  plannerEnabled = null;
  const cleared = nextRevision();
  sound.clearNativeManifest(cleared);
  if (IS_DEV) console.log(`[NativeManifest] cleared at rev=${cleared}`);
};
