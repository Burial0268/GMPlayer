/**
 * resolveSongUrl — Unified song URL resolution with NCM + UNM fallback.
 *
 * Consolidates the duplicated URL resolution logic from:
 * - Player/index.vue (getPlaySongData + getMusicNumUrlData)
 * - AudioPreloader.ts (_resolveAndPreload)
 * - PreBufferManager.ts (doPreBuffer)
 * - musicData.ts (preloadUpcomingSongs)
 *
 * Handles: quality level, VIP pre-check, trial version detection,
 * UNM fallback, and kuwo proxy URL — and short-circuits an imported local file,
 * whose locator is already the answer.
 */

import { getMusicUrl, getMusicNumUrl } from "@/api/song";
import type { MusicLevel } from "@/api/types";
import useSettingDataStore from "@/store/settingData";

const IS_DEV = import.meta.env?.DEV ?? false;
const useUnmServerHas = !!import.meta.env.VITE_UNM_API;

export interface SongUrlInput {
  id: number;
  fee?: number;
  pc?: any;
  name?: string;
  /**
   * Set for an imported local file. Its presence — never `id < 0` — is the
   * dispatch key, and it must be carried by every caller that passes a narrowed
   * object instead of the store row.
   */
  local?: { uri?: string | null } | null;
}

export interface ResolveSongUrlResult {
  url: string;
  source: "ncm" | "unm" | "local";
}

export interface ResolveSongUrlOptions {
  signal?: AbortSignal;
}

/**
 * In-flight resolution sharing.
 *
 * The same upcoming track is resolved concurrently by several independent
 * subsystems the moment a track starts — `AudioPreloader`, `NativeQueuePrefill`
 * and AutoMix's `PreBufferManager` all reach for the *next* song, and
 * `musicData` resolves it again when it actually plays. Each of those was a
 * separate network round-trip for an identical answer.
 *
 * Only in-flight requests are shared, deliberately: resolved Netease CDN URLs
 * expire, so caching a completed result risks handing back a dead link, which
 * would surface as an unplayable track. Sharing the pending promise removes the
 * duplicate requests with no staleness window at all.
 *
 * Each caller races the shared promise against its own `AbortSignal`, so one
 * caller aborting (e.g. the preloader being cancelled on a track change)
 * rejects only that caller and never cancels the work the others are awaiting.
 *
 * The key omits `fee`/`pc`/`useUnmServer`, which also steer resolution: every
 * caller passes the same store song object for a given id, so they cannot
 * disagree in practice. Pass a *partial* song (id only) and you would be
 * sharing an answer resolved down the wrong branch.
 */
const inFlight = new Map<string, Promise<ResolveSongUrlResult | null>>();

function abortRejection(signal: AbortSignal): Promise<never> {
  return new Promise((_, reject) => {
    if (signal.aborted) {
      reject(signal.reason ?? new DOMException("Aborted", "AbortError"));
      return;
    }
    signal.addEventListener(
      "abort",
      () => reject(signal.reason ?? new DOMException("Aborted", "AbortError")),
      { once: true },
    );
  });
}

/**
 * Resolve a playable URL for a song via NCM official API, with UNM fallback.
 *
 * @param song  - Song metadata (id required; fee/pc used for VIP detection)
 * @param level - Quality level (defaults to store setting or "exhigh")
 * @param options - Optional AbortSignal. NOTE: aborting **rejects** with
 *   `AbortError`; it does not resolve `null`. It also does not cancel the
 *   underlying request, which is shared with other callers — it only detaches
 *   this caller. Callers passing a signal must catch.
 * @returns `{ url, source }` or `null` if no URL could be resolved
 */
export async function resolveSongUrl(
  song: SongUrlInput,
  level?: MusicLevel | string,
  options?: ResolveSongUrlOptions,
): Promise<ResolveSongUrlResult | null> {
  // An imported local file *is* its own source: the locator is the answer, so
  // there is nothing to resolve and nothing to share in flight.
  //
  // This has to live here rather than at each call site. Six subsystems resolve
  // upcoming tracks — the preloader, `musicData.preloadUpcomingSongs`,
  // `NativeQueuePrefill`, AutoMix's pre-buffer and its native prepare, plus the
  // player itself — and a local track that slips past one of them does not fail
  // loudly: `song_url_v1` answers nothing for a negative id, the UNM fallback
  // then asks a *third-party* server about a track id that exists nowhere, and
  // AutoMix retries the prepare for as long as the track plays. That is the
  // steady stream of UNM requests seen while playing local files.
  const localUri = song.local?.uri;
  if (typeof localUri === "string" && localUri) {
    return { url: localUri, source: "local" };
  }

  // Backstop for a row that lost its `local` marker (an older persisted queue,
  // a caller that narrowed the song object). A Netease id is always positive, so
  // there is no question to ask here — and asking it is exactly what the
  // negative-id scheme exists to prevent.
  const numericId = Number(song.id);
  if (!Number.isFinite(numericId) || numericId <= 0) {
    if (IS_DEV) {
      console.warn(`[resolveSongUrl] ${song.name ?? song.id}: not a Netease id, refusing to ask`);
    }
    return null;
  }

  const settingStoreForKey = useSettingDataStore();
  const keyLevel = (level || settingStoreForKey.songLevel || "exhigh") as MusicLevel;
  const key = `${song.id}:${keyLevel}`;

  let shared = inFlight.get(key);
  if (!shared) {
    // The shared request intentionally carries no caller signal — see above.
    shared = resolveSongUrlUncached(song, level).finally(() => {
      inFlight.delete(key);
    });
    inFlight.set(key, shared);
  }

  const signal = options?.signal;
  if (!signal) return shared;
  return Promise.race([shared, abortRejection(signal)]);
}

/**
 * The shared request body. Deliberately takes no `AbortSignal`: it is awaited by
 * every caller for this id+level, so no single caller may cancel it. Detaching
 * an individual caller is `resolveSongUrl`'s race above.
 */
async function resolveSongUrlUncached(
  song: SongUrlInput,
  level?: MusicLevel | string,
): Promise<ResolveSongUrlResult | null> {
  const logPrefix = `[resolveSongUrl] ${song.name ?? song.id}`;

  // Resolve effective level
  const settingStore = useSettingDataStore();
  const effectiveLevel = (level || settingStore.songLevel || "exhigh") as MusicLevel;
  const unmEnabled = useUnmServerHas && settingStore.useUnmServer;

  // VIP pre-check: fee=1 (VIP) or fee=4 (paid album), no PC (cloud-uploaded) bypass
  if (unmEnabled && !song.pc && (song.fee === 1 || song.fee === 4)) {
    if (IS_DEV) {
      console.log(`${logPrefix}: VIP/paid song, going directly to UNM`);
    }
    return resolveViaUnm(song.id, logPrefix);
  }

  // Step 1: Try NCM official API
  let url: string | null = null;
  try {
    const res = await getMusicUrl(song.id, effectiveLevel);

    const rawUrl = res?.data?.[0]?.url;
    if (rawUrl) {
      url = rawUrl.replace(/^http:/, "https:");

      // Trial version detection (jd-musicrep-ts = trial/preview clip)
      if (url.includes("jd-musicrep-ts")) {
        if (IS_DEV) {
          console.log(`${logPrefix}: trial version detected, nullifying NCM URL`);
        }
        url = null;
      }
    }
  } catch (err) {
    if (IS_DEV) {
      console.warn(`${logPrefix}: getMusicUrl failed`, err);
    }
    url = null;
  }

  // Step 2: If NCM succeeded, return it
  if (url) {
    return { url, source: "ncm" };
  }

  // Step 3: UNM fallback
  if (unmEnabled) {
    if (IS_DEV) {
      console.log(`${logPrefix}: no NCM URL, trying UNM fallback`);
    }
    return resolveViaUnm(song.id, logPrefix);
  }

  if (IS_DEV) {
    console.warn(`${logPrefix}: no URL resolved (UNM disabled)`);
  }
  return null;
}

/**
 * Resolve URL via UNM (UnblockNeteaseMusic) API, handling kuwo proxy.
 */
async function resolveViaUnm(id: number, logPrefix: string): Promise<ResolveSongUrlResult | null> {
  try {
    const unmRes = await getMusicNumUrl(id);

    if (unmRes?.code === 200 && unmRes?.data?.url) {
      let url: string = unmRes.data.url.replace(/^http:/, "https:");

      // kuwo.cn proxy handling: use proxyUrl if available
      if (/kuwo\.cn/i.test(url) && unmRes.data.proxyUrl) {
        url = unmRes.data.proxyUrl;
        if (IS_DEV) {
          console.log(`${logPrefix}: using kuwo proxy URL`);
        }
      }

      if (IS_DEV) {
        console.log(`${logPrefix}: resolved via UNM`);
      }
      return { url, source: "unm" };
    }
  } catch (err) {
    if (IS_DEV) {
      console.warn(`${logPrefix}: UNM fallback failed`, err);
    }
  }
  return null;
}
