/**
 * NativeQueuePrefill — feeds the Rust audio-backend a bounded playback window
 * (current track + pre-resolved next tracks) so the backend's own
 * `NextSongGapless` advance plays the REAL next song even when the JS runtime
 * is frozen (Android background).
 *
 * Tauri-only: the Web/WASM backend never auto-advances (playback is owned by
 * the browser media host) and pure-web playback keeps using AudioPreloader.
 *
 * Window semantics (see docs/native-queue-background-playback-plan.md):
 * - normal mode:  [cur@i, next@(i+1)%L, ...] windowed=true (stop when exhausted)
 * - random mode:  [cur@i, prePicked@k]       windowed=true (depth 1 — duplicate
 *                 origOrder entries would break the backend's identity matching)
 * - single mode / single-song list: [cur@i]  windowed=false (native wrap = repeat)
 * - personal FM / listen-together: no prefill (transitions need live JS)
 */

import { isTauri } from "@/utils/tauri/core/runtime";
import type { SongData as QueueSongData } from "@/utils/tauri/audio/protocol";
import { NativeRustSound } from "@/utils/tauri/audio/nativeRustSound";
import { resolveSongUrl } from "./resolveSongUrl";
import { toTrackDisplay } from "./NativeManifestPublisher";
// Import stores directly to avoid circular dependency through barrel exports
import useMusicDataStore from "@/store/musicData";
import useListenTogetherStore from "@/store/listenTogether";
import { SoundManager } from "./SoundManager";

const IS_DEV = import.meta.env?.DEV ?? false;

/** How many upcoming tracks to resolve and hand to the backend per window. */
const PREFILL_DEPTH = 2;

const REGISTRY_STORAGE_KEY = "gmplayer:nativeQueueRegistry";
const REGISTRY_MAX_ENTRIES = 40;

export interface NativeQueueRegistryEntry {
  songId: number;
  index: number;
}

/**
 * musicId (`local:<url>`) → song identity. Lets adoption / attach paths map a
 * backend-reported track back to a store song even though resolved CDN URLs
 * differ between resolutions. Mirrored to localStorage so a killed-and-restarted
 * WebView can still reconcile with a backend that advanced while it was dead.
 */
const registry = new Map<string, NativeQueueRegistryEntry>();
let registryLoaded = false;

let prefillGeneration = 0;
let abortController: AbortController | null = null;

const loadRegistryFromStorage = (): void => {
  if (registryLoaded) return;
  registryLoaded = true;
  try {
    const raw = localStorage.getItem(REGISTRY_STORAGE_KEY);
    if (!raw) return;
    const parsed = JSON.parse(raw) as [string, NativeQueueRegistryEntry][];
    if (!Array.isArray(parsed)) return;
    for (const [musicId, entry] of parsed) {
      if (typeof musicId === "string" && entry && Number.isFinite(entry.songId)) {
        registry.set(musicId, { songId: entry.songId, index: entry.index });
      }
    }
  } catch {
    /* corrupted registry is disposable */
  }
};

const persistRegistry = (): void => {
  try {
    while (registry.size > REGISTRY_MAX_ENTRIES) {
      const oldest = registry.keys().next().value;
      if (oldest === undefined) break;
      registry.delete(oldest);
    }
    localStorage.setItem(REGISTRY_STORAGE_KEY, JSON.stringify([...registry.entries()]));
  } catch {
    /* quota/unavailable — registry is best-effort */
  }
};

export const getNativeQueueRegistryEntry = (
  musicId: string | undefined | null,
): NativeQueueRegistryEntry | null => {
  if (!musicId) return null;
  loadRegistryFromStorage();
  return registry.get(musicId) ?? null;
};

export const cancelNativeQueuePrefill = (): void => {
  prefillGeneration++;
  if (abortController) {
    abortController.abort();
    abortController = null;
  }
};

/**
 * Resolve the upcoming window for the current play mode and push it to the
 * backend queue. Call whenever the active track (re)starts: `handleNativePlay`
 * and after a native-advance adoption.
 */
export async function prefillNativeQueue(): Promise<void> {
  if (!isTauri()) return;
  const sound = window.$player;
  if (!(sound instanceof NativeRustSound) || sound.isDestroyed()) return;

  const music = useMusicDataStore();
  const listenTogether = useListenTogetherStore();
  cancelNativeQueuePrefill();
  if (music.persistData.personalFmMode) return;
  if (listenTogether.isInRoom) return;

  const playlists = music.persistData.playlists;
  const listLength = playlists.length;
  if (listLength === 0) return;

  const currentIndex = music.persistData.playSongIndex;
  const currentSong = playlists[currentIndex];
  if (!currentSong?.id) return;
  const mode = music.persistData.playSongMode;

  const generation = prefillGeneration;
  abortController = new AbortController();
  const signal = abortController.signal;

  // The current entry must carry the exact URL the sound was created with:
  // the backend re-anchors the replaced playlist by `local:<url>` identity.
  const currentEntry: QueueSongData = {
    type: "local",
    filePath: sound.getSourceUrl(),
    origOrder: currentIndex,
    // Sent with the track so the OS media session is correct on the first
    // frame. The backend downloads this https path to a temp file before
    // decoding, so without it the only name available at load time is that
    // temp file's random stem.
    display: toTrackDisplay(currentSong),
  };

  let nextIndices: number[] = [];
  let windowed = true;
  if (mode === "single" || listLength === 1) {
    // Native wrap-around on a single-entry queue IS repeat: keep it.
    windowed = false;
  } else if (mode === "random") {
    let pick = Math.floor(Math.random() * listLength);
    if (pick === currentIndex) pick = (pick + 1) % listLength;
    nextIndices = [pick];
  } else if (mode === "normal") {
    const seen = new Set<number>([currentIndex]);
    for (let i = 1; i <= PREFILL_DEPTH; i++) {
      const nextIndex = (currentIndex + i) % listLength;
      if (seen.has(nextIndex)) break;
      seen.add(nextIndex);
      nextIndices.push(nextIndex);
    }
  } else {
    return;
  }

  const resolved = await Promise.all(
    nextIndices.map(async (index) => {
      const songData = playlists[index];
      if (!songData?.id) return null;
      // A local file *is* its own source: there is nothing to resolve, and
      // handing its negative id to `resolveSongUrl` would ask Netease about a
      // track that does not exist there. The locator goes into the window
      // verbatim, which is exactly what the backend's `SongData::Local` wants.
      const localUri = songData.local?.uri;
      if (typeof localUri === "string" && localUri) {
        return {
          index,
          songId: songData.id as number,
          url: localUri,
          display: toTrackDisplay(songData),
        };
      }
      try {
        const result = await resolveSongUrl(songData, undefined, { signal });
        if (!result?.url) return null;
        return {
          index,
          songId: songData.id as number,
          url: result.url,
          display: toTrackDisplay(songData),
        };
      } catch {
        return null;
      }
    }),
  );

  if (signal.aborted || generation !== prefillGeneration) return;
  if (sound.isDestroyed() || window.$player !== sound) return;
  if (music.persistData.personalFmMode || listenTogether.isInRoom) return;
  // Bail if the store moved on while URLs were resolving — the new track's
  // own play handler re-runs the prefill against fresh state.
  const livePlaylists = music.persistData.playlists;
  const liveCurrentSong = livePlaylists[currentIndex];
  if (
    livePlaylists.length === 0 ||
    music.persistData.playSongIndex !== currentIndex ||
    liveCurrentSong?.id !== currentSong.id ||
    music.playingSongId !== currentSong.id ||
    SoundManager.getSongId(sound) !== Number(currentSong.id)
  ) {
    return;
  }

  loadRegistryFromStorage();
  const entries: QueueSongData[] = [currentEntry];
  registry.set(`local:${currentEntry.filePath}`, {
    songId: currentSong.id,
    index: currentIndex,
  });
  for (const entry of resolved) {
    // Truncate at the first failure: a gap would make the backend jump the
    // playback order (entries advance positionally).
    if (!entry) break;
    entries.push({
      type: "local",
      filePath: entry.url,
      origOrder: entry.index,
      display: entry.display,
    });
    registry.set(`local:${entry.url}`, { songId: entry.songId, index: entry.index });
  }
  persistRegistry();

  const armed = sound.applyNativeQueueWindow(entries, { windowed });
  if (IS_DEV) {
    console.log(
      `[NativeQueuePrefill] window sent: ${entries.length} entr${entries.length > 1 ? "ies" : "y"}, ` +
        `mode=${mode}, windowed=${windowed}, armed=${armed}, indices=[${entries
          .map((entry) => entry.origOrder)
          .join(", ")}]`,
    );
  }
}
