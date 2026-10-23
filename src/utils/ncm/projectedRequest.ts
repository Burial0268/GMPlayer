/**
 * The two endpoints whose answers are measured in megabytes, projected in Rust
 * before they cross IPC.
 *
 * `playlist_detail` and `song_detail` are what fill every song list in the app,
 * and almost none of what they return is read. Measured from real cached
 * envelopes:
 *
 * | endpoint | as it comes | projected | `JSON.parse` |
 * |---|---|---|---|
 * | `playlist_detail`, 879 tracks | 2,449,577 chars | 520,610 (21.3%) | 39.3 → 9.6 ms |
 * | `song_detail`, 100 ids | 216,329 chars | 37,282 (17.2%) | 1.17 → 0.25 ms |
 *
 * `song_detail` is asked 1000 ids at a time (`HYDRATE_CHUNK`), so a hydrate chunk
 * is **2.06 MB → 0.36 MB**. Where that saving lands is the whole point: the bytes
 * already reach the WebView the cheapest way Tauri offers — `tauri::ipc::Response`
 * → `application/octet-stream` → `arrayBuffer()`, off the main thread — and the
 * `JSON.parse` that follows is the one step in the entire path (network, isolate,
 * transport, IPC) that runs on the thread drawing the UI.
 *
 * Streaming was considered and is the wrong lever; the reasoning is on
 * `src-tauri/src/ncm/projection.rs`. Short version: an eapi body must be whole
 * before it decrypts at all, and a Tauri command has exactly one response.
 *
 * ## This is a view, not a replacement
 *
 * The Rust side runs the *same* `NcmCore::call`, so the cache, `batch` and
 * `inflight` behave identically and what they store is the **full** envelope. A
 * caller that needs a dropped field — `DataModal/DownloadSong.vue` reads
 * `privileges[0].downloadMaxbr` — keeps using the plain `src/api/*` function and
 * is answered from the same cached bytes.
 */

import { getPlayListDetail } from "@/api/playlist";
import { getMusicDetail } from "@/api/song";
import { getNcmTransport } from "@/utils/request";
import { isTauri } from "@/utils/tauri/core/runtime";
import { decodeEnvelope, getInvoke, urlToEndpoint, withCookie } from "@/utils/ncmLocalTransport";

/**
 * One projected call, or `undefined` when this path is not available.
 *
 * `url` and `params` are spelled exactly as the `src/api/*` function spells them,
 * for the same reason `hintNcm` insists on it: the cache is keyed on the whole
 * parameter set, so a query spelled differently is a second entry and the
 * prefetch hint pairs with neither. `timestamp` is left out because the cache
 * strips it (`VOLATILE_KEYS`), and key order does not matter — it canonicalises.
 *
 * `fresh` skips the cache *read* on the Rust side (`NcmCore::call_fresh`). The
 * answer is still stored, so this is one round trip, not a cache bypass.
 */
const requestProjected = async (
  url: string,
  params: Record<string, unknown>,
  fresh = false,
): Promise<any | undefined> => {
  // Web has no embedded layer, and the remote transport answers over HTTP where
  // there is nothing to project on our side.
  if (!isTauri() || getNcmTransport() !== "local") return undefined;
  try {
    const invoke = await getInvoke();
    const raw = await invoke<unknown>("ncm_request_projected", {
      endpoint: urlToEndpoint(url),
      query: JSON.stringify(withCookie({ ...params })),
      fresh,
    });
    const envelope = decodeEnvelope(raw);
    if (envelope.ok) return envelope.body;
    // A non-200 upstream answer falls through rather than being rethrown here:
    // the axios path owns the error mapping every caller is written against (a
    // 301 arrives as a rejected promise via the response interceptor), and
    // reproducing that would be a second copy of it. Costs one extra request,
    // and only when the call was going to fail anyway.
  } catch (err) {
    console.warn(`[ncm] projected ${url} unavailable, using the plain path`, err);
  }
  return undefined;
};

/**
 * Playlist detail. Same return as `getPlayListDetail` — the response body, so
 * callers read `res.playlist` either way.
 *
 * The projected shape differs from upstream's only by omission, with one named
 * exception callers must tolerate because the fallback hands back the raw form:
 * `playlist.trackIds` may be `number[]` instead of `[{ id, … }]`.
 *
 * `fresh` is for the caller that has a reason to believe what is held is behind:
 * a reconcile after a write, or re-reading a list the user has been away from.
 * Without it a post-write refetch can be answered from a stale entry — a write
 * that did not pass through the embedded layer (the `remote` transport, the Rust
 * resolver's own `/like`, another device, the web) invalidates nothing, and
 * `playlist_detail` is served stale for a day. See `PlayListView.reconcileQuietly`.
 *
 * The remote transport and Web ignore it: neither has a cache in front of it.
 */
export const fetchPlaylistDetail = async (
  id: number | string,
  options?: { fresh?: boolean },
): Promise<any> => {
  const numeric = Number(id);
  if (!Number.isFinite(numeric)) return getPlayListDetail(numeric);
  const projected = await requestProjected("/playlist/detail", { id: numeric }, options?.fresh);
  return projected ?? getPlayListDetail(numeric);
};

/**
 * Song detail. Same return as `getMusicDetail` — `{ songs, … }`.
 *
 * `songs[]` carries only the keys `transformSongData` reads, and `privileges` is
 * gone. Anything needing either must call `getMusicDetail` directly.
 */
export const fetchSongDetail = async (ids: string | number | number[]): Promise<any> => {
  const idsStr = Array.isArray(ids) ? ids.join(",") : String(ids);
  const projected = await requestProjected("/song/detail", { ids: idsStr });
  return projected ?? getMusicDetail(ids);
};
