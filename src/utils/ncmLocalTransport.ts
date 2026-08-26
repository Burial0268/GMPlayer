/**
 * In-process NCM transport for Tauri builds.
 *
 * Swaps axios's adapter so the 80-odd call sites in `src/api/*` keep their
 * exact shape — `request({ url: "/song/url/v1", params })` — while the request
 * is served by the embedded protocol layer instead of a deployed
 * NeteaseCloudMusicApi. That removes one full network hop; measured at
 * ~38–48 ms per call. See `docs/native-ncm-api-embedding-plan.md`.
 *
 * Interceptors are untouched: they are attached to the axios instance, and
 * replacing the adapter leaves `$loadingBar`, the error mapping and
 * `response.data` unwrapping exactly as they were.
 */

import { AxiosError, AxiosHeaders } from "axios";
import type { AxiosAdapter, AxiosResponse, InternalAxiosRequestConfig } from "axios";

const decoder = new TextDecoder();

/**
 * `/song/url/v1` → `song_url_v1`, matching the upstream module basenames.
 *
 * Call sites interpolate before we see them (`/comment/${type}`,
 * `/toplist${detail ? "/detail" : ""}`), so this only ever handles a literal
 * path — no need to enumerate the dynamic forms.
 */
export const urlToEndpoint = (url: string): string =>
  url.split("?")[0].replace(/^\/+/, "").replace(/\/+$/, "").replace(/\//g, "_");

/**
 * Whether the embedded transport can represent this request at all.
 *
 * The protocol layer takes a JSON object of parameters, so anything carrying a
 * binary body or wanting upload progress (currently only cloud song upload,
 * whose endpoint is stubbed out of the bundle anyway) has to go over the
 * remote transport instead. Delegating beats failing: the feature keeps
 * working, just on the old path.
 */
const isRepresentable = (config: InternalAxiosRequestConfig): boolean => {
  if (typeof config.onUploadProgress === "function") return false;
  if (typeof config.onDownloadProgress === "function") return false;
  const body = config.data;
  if (body === null || body === undefined) return true;
  if (typeof body === "string") return true;
  if (typeof FormData !== "undefined" && body instanceof FormData) return false;
  if (typeof Blob !== "undefined" && body instanceof Blob) return false;
  if (body instanceof ArrayBuffer || ArrayBuffer.isView(body)) return false;
  return typeof body === "object";
};

/**
 * Where the session cookie comes from.
 *
 * Injected rather than imported: the authoritative copy lives in the `userData`
 * Pinia store, and importing that here would close a cycle
 * (store → api → request → store). `main.ts` registers a provider once Pinia
 * has hydrated.
 */
type CookieProvider = () => string;

let cookieProvider: CookieProvider | null = null;

export const setNcmCookieProvider = (provider: CookieProvider): void => {
  cookieProvider = provider;
};

/**
 * The session cookie.
 *
 * The store is authoritative and the fallback is not equivalent: on desktop
 * `userData` is persisted to a Tauri store *file*, and hydrating from it
 * restores `userData.cookie` without rewriting the standalone `"cookie"`
 * localStorage key. Reading localStorage can therefore come back empty for a
 * perfectly logged-in user — and a cookie-less `/song/url/v1` does not fail, it
 * quietly returns a 30-second trial fragment, which then fails much later as
 * "cannot play this track".
 *
 * localStorage remains the fallback for windows that never run `main.ts`'s
 * setup (slave windows), where it is the only copy available.
 */
const currentCookie = (): string => {
  if (cookieProvider) {
    try {
      return cookieProvider() || "";
    } catch {
      // Fall through: a store that is not ready yet is better served by the
      // stale copy than by sending nothing.
    }
  }
  try {
    return window.localStorage.getItem("cookie") ?? "";
  } catch {
    return "";
  }
};

/** Shape returned by the `ncm_request` command. */
interface Envelope {
  ok: boolean;
  status: number;
  body: unknown;
  cookie: string[];
}

/**
 * Decode whatever `invoke` hands back for a raw-bytes command.
 *
 * A Rust `tauri::ipc::Response` is expected to arrive as an `ArrayBuffer`, but
 * that is a detail of the injected IPC layer rather than a typed contract, and
 * it has differed across transports (custom protocol vs. postMessage). Accept
 * the byte forms and a plain string so a change there degrades into a slower
 * path rather than breaking every request.
 */
const decodeEnvelope = (raw: unknown): Envelope => {
  let text: string;
  if (typeof raw === "string") {
    text = raw;
  } else if (raw instanceof ArrayBuffer) {
    text = decoder.decode(new Uint8Array(raw));
  } else if (ArrayBuffer.isView(raw)) {
    const view = raw as ArrayBufferView;
    text = decoder.decode(new Uint8Array(view.buffer, view.byteOffset, view.byteLength));
  } else if (Array.isArray(raw)) {
    text = decoder.decode(new Uint8Array(raw as number[]));
  } else {
    // Already-parsed object: nothing to decode.
    return raw as Envelope;
  }
  return JSON.parse(text) as Envelope;
};

const buildQuery = (config: InternalAxiosRequestConfig): Record<string, unknown> => {
  const query: Record<string, unknown> = {};

  // The deployed server merges query string and body into one bag before
  // handing it to the endpoint module; mirror that so call sites that split
  // parameters across `params` and `data` behave identically.
  Object.assign(query, config.params ?? {});
  if (config.data && typeof config.data === "object") {
    Object.assign(query, config.data);
  } else if (typeof config.data === "string" && config.data.length > 0) {
    try {
      Object.assign(query, JSON.parse(config.data));
    } catch {
      for (const [k, v] of new URLSearchParams(config.data)) query[k] = v;
    }
  }

  return withCookie(query);
};

/**
 * Attach the session cookie, as every request that crosses to Rust must.
 *
 * Split out from `buildQuery` because the prefetch path in `@/utils/ncmPrefetch`
 * has to produce a byte-identical query: the cache is keyed on the whole
 * parameter set, cookie included, so a hint that spells anything differently
 * warms an entry the real call will never look at. Sharing the code is what
 * makes that a compile-time guarantee rather than a convention nobody can see
 * being broken — a mismatched hint fails silently and simply stops helping.
 */
export const withCookie = (query: Record<string, unknown>): Record<string, unknown> => {
  const cookie = currentCookie();
  if (cookie && query.cookie === undefined) query.cookie = cookie;
  return query;
};

/**
 * `invoke`, resolved once. Shared with the prefetch path.
 */
export const getInvoke = (): Promise<typeof import("@tauri-apps/api/core").invoke> => {
  invokePromise ??= import("@tauri-apps/api/core").then((mod) => mod.invoke);
  return invokePromise;
};

/**
 * `invoke`, resolved once.
 *
 * This used to be a dynamic `import()` inside the adapter, i.e. on every API
 * call. The module registry caches it, so the cost after the first call was a
 * microtask rather than a fetch — but it also put a promise hop in front of
 * every request for no reason. Resolved lazily still, so a non-Tauri build
 * never loads the module at all.
 */
let invokePromise: Promise<typeof import("@tauri-apps/api/core").invoke> | null = null;

/**
 * Build an adapter that serves requests in-process, delegating anything it
 * cannot represent to `fallback`.
 *
 * `onUnavailable` fires once the embedded transport has failed at the *IPC*
 * level enough times in a row to be considered broken — a missing command, a
 * dead isolate, a bootstrap that never completed. Since `local` is the default
 * on Tauri, without this a broken isolate would take down every API call in
 * the app rather than costing one extra hop.
 *
 * An `ok: false` envelope is explicitly *not* a failure: that is Netease
 * answering with a non-200 code, which the caller handles.
 */
export function createLocalNcmAdapter(
  fallback: AxiosAdapter,
  onUnavailable?: (reason: string) => void,
): AxiosAdapter {
  const FAILURE_LIMIT = 3;
  let consecutiveFailures = 0;

  return async (config: InternalAxiosRequestConfig): Promise<AxiosResponse> => {
    if (!config.url || !isRepresentable(config)) return fallback(config);

    const endpoint = urlToEndpoint(config.url);

    let raw: unknown;
    try {
      const invoke = await getInvoke();
      // Raw bytes: the response crosses IPC once instead of being
      // re-serialized into the invoke envelope. `/playlist/track/all` reaches
      // megabytes.
      raw = await invoke<unknown>("ncm_request", {
        endpoint,
        query: JSON.stringify(buildQuery(config)),
      });
      consecutiveFailures = 0;
    } catch (err) {
      consecutiveFailures += 1;
      const reason = err instanceof Error ? err.message : String(err);
      console.warn(
        `[ncm] embedded transport failed for ${endpoint} (${consecutiveFailures}/${FAILURE_LIMIT}): ${reason}`,
      );
      if (consecutiveFailures >= FAILURE_LIMIT) onUnavailable?.(reason);
      return fallback(config);
    }

    const envelope = decodeEnvelope(raw);

    const response: AxiosResponse = {
      data: envelope.body,
      status: envelope.status,
      statusText: "",
      headers: new AxiosHeaders(),
      config,
      request: null,
    };

    // The deployed server answers non-200 NCM codes with the matching HTTP
    // status, so axios rejects and the response interceptor maps it. Reproduce
    // that rather than resolving everything: callers act on 301 (not logged
    // in) through the error path.
    if (!envelope.ok) {
      throw new AxiosError(
        `Request failed with status code ${envelope.status}`,
        String(envelope.status),
        config,
        null,
        response,
      );
    }

    return response;
  };
}
