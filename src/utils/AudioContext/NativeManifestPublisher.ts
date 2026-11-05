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
 * Traversal order is the *playlist* order in every mode but random. In random
 * mode the backend owns the permutation (`ManifestStore::set_owning_order`):
 * `entries` is a set, it carries the permutation over by identity across
 * republishes, and it reports a different order back as
 * `nativeQueueOrderChanged`, which `musicData.adoptBackendQueueOrder` mirrors
 * into the queue. That is one writer instead of two, and it is what stops the
 * screen naming a song other than the one playing after the WebView was away.
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
import { trackIdentityKey } from "@/utils/tauri/audio/identity";
import { logNative } from "@/utils/tauri/core/nativeLog";
import { installQueueOrderSubscriber } from "./NativeQueueOrderSync";
import { bumpManifestRevision, raiseManifestRevision } from "./NativeManifestRevision";

/** Last published fingerprint, to skip redundant IPC on unrelated store churn. */
let lastFingerprint = "";
let publishScheduled = false;
let pendingForce = false;
/**
 * The next manifest replaces the list (or enters random for the first time the
 * backend hears of it), so the backend should draw a fresh permutation. Sticky
 * until a manifest is actually built: a request made before any sound exists
 * must survive the early returns in `flushNativeManifest`.
 */
let pendingReshuffle = false;

/** Ask the backend for a new random permutation with the next manifest. */
export const requestNativeReshuffle = (): void => {
  if (!isTauri()) return;
  pendingReshuffle = true;
  publishNativeManifest({ force: true });
};

/**
 * Adopt a revision the backend reports. Covers the case where our persisted
 * counter is behind the live backend (cleared storage, a different profile):
 * without this the backend would reject everything we publish.
 */
export const reconcileNativeManifestRevision = (backendRevision: number): void => {
  if (!raiseManifestRevision(backendRevision)) return;
  // Our last-published state no longer describes what the backend holds.
  lastFingerprint = "";
  plannerEnabled = null;
};

// Adopt the backend's revision as soon as it reports one, so a reload can never
// leave us publishing manifests the backend rejects as stale.
setPlannerRevisionObserver(reconcileNativeManifestRevision);

/**
 * Stable seed for the backend's shuffles. One per session is enough: the backend
 * mixes it with the revision and the pass.
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
 * choose the next one myself". Listen-together has its next track decided by a
 * server, so it keeps the manifest (the backend still needs identity →
 * resolvable source, e.g. to honour a remote GOTO) but loses the planner.
 *
 * Personal FM keeps *both*: it pre-fetches a buffer into the playlist, so the
 * planner advancing through that buffer is exactly the desired behaviour (and
 * the only way the next track can play while the Android JS worker is frozen).
 */
const applyPlannerGate = (sound: NativeRustSound, enabled: boolean): void => {
  if (plannerEnabled === enabled) return;
  plannerEnabled = enabled;
  sound.setNativePlannerEnabled(enabled);
  logNative("info", `[NativeManifest] planner ${enabled ? "enabled" : "gated off"}`);
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
  // Hear the backend's order before we publish anything it could answer.
  installQueueOrderSubscriber();

  const music = useMusicDataStore();
  if (music.persistData.personalFmMode) return;
  const listenTogether = useListenTogetherStore();

  const playlists = music.persistData.playlists;
  if (!playlists?.length) {
    clearNativeManifest();
    return;
  }

  // Listen-together keeps its manifest but cannot advance without the room.
  // Native FM returned above: Rust owns both its reservoir and its manifest.
  const serverDrivenOrder = listenTogether.isInRoom;
  applyPlannerGate(sound, !serverDrivenOrder);

  const entries: NativeManifestEntry[] = [];
  let cursorPosition = -1;
  const cursorSongId = music.playingSongId ?? music.getPlaySongData?.id;

  // Probe before building: bail on the no-op path without allocating entries.
  const signature = playlistSignature(playlists, music.persistData.playSongMode, cursorSongId);
  if (!force && !pendingReshuffle && signature === lastFingerprint) return;

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

  // Consumed here, past every early return, so a request is never lost.
  const reshuffle = pendingReshuffle;
  pendingReshuffle = false;
  const pinSong = reshuffle ? music.getPlaySongData : null;
  const shufflePin = pinSong ? toIdentity(pinSong) : null;

  const manifest: NativePlaybackManifest = {
    schemaVersion: 1,
    revision: bumpManifestRevision(),
    entries,
    // Empty = natural order. In random mode the backend decides the permutation
    // and ignores this; it tells us when ours is stale rather than us guessing.
    order: [],
    cursorIdentity,
    cursorIndex,
    mode: mode === "single" ? "single" : mode === "random" ? "random" : "normal",
    repeatList: true,
    randomSeed,
    reshuffle,
    shufflePin,
  };

  sound.setNativeManifest(manifest);
  logNative(
    "info",
    `[NativeManifest] published rev=${manifest.revision} entries=${entries.length} ` +
      `mode=${manifest.mode} cursor=${cursorIndex}/${trackIdentityKey(cursorIdentity)} ` +
      `reshuffle=${reshuffle} pin=${trackIdentityKey(shufflePin)}`,
  );
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
  const cleared = bumpManifestRevision();
  sound.clearNativeManifest(cleared);
  logNative("info", `[NativeManifest] cleared at rev=${cleared}`);
};
