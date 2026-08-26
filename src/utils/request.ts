import axios, {
  AxiosRequestConfig,
  AxiosResponse,
  AxiosError,
  InternalAxiosRequestConfig,
  AxiosHeaders,
} from "axios";
import type { AxiosAdapter } from "axios";
import { fetch as tauriFetch } from "@tauri-apps/plugin-http";
import { isMobileDevice } from "@/utils/tauri/platform/mobile";
import { isTauri } from "@/utils/tauri/core/runtime";
import { createLocalNcmAdapter, setNcmCookieProvider } from "@/utils/ncmLocalTransport";

// Extend AxiosRequestConfig to include custom hiddenBar property
export interface CustomAxiosRequestConfig extends AxiosRequestConfig {
  hiddenBar?: boolean;
}

// Extend InternalAxiosRequestConfig for interceptor use
interface CustomInternalAxiosRequestConfig extends InternalAxiosRequestConfig {
  hiddenBar?: boolean;
}

// Global $loadingBar type is declared in src/types/globals.d.ts

let baseURL = "";

// Use Vite's environment detection
if (import.meta.env.PROD) {
  baseURL = import.meta.env.VITE_MUSIC_API as string;
} else {
  baseURL = "/api/ncm";
}

axios.defaults.baseURL = baseURL;
axios.defaults.timeout = 30000;
axios.defaults.headers.common["X-Requested-With"] = "XMLHttpRequest";
axios.defaults.withCredentials = true;

// Android/iOS WebViews enforce browser CORS for production API requests. Use
// Tauri's native HTTP transport on mobile builds so login and its cookie jar
// stay on the same transport. Development keeps using Vite's same-origin proxy.
const useNativeMobileHttp =
  import.meta.env.PROD && typeof window !== "undefined" && isTauri() && isMobileDevice();

if (useNativeMobileHttp) {
  axios.defaults.adapter = "fetch";
  axios.defaults.env = {
    ...axios.defaults.env,
    fetch: tauriFetch,
  };
}

// ── NCM transport ────────────────────────────────────────────────
// `local` serves requests from the embedded protocol layer in the Rust side,
// removing one network hop (~38–48 ms per call, measured — see
// docs/native-ncm-api-embedding-plan.md). `remote` keeps talking to the
// deployed NeteaseCloudMusicApi.
//
// Tauri defaults to `local`; Web has no embedded layer and is forced to
// `remote`. The remote backend is never removed: the win is geography
// dependent — for a client far from Netease the deployed API can be the closer
// hop — and going direct also moves the request's source IP from the server to
// the user's machine, which changes how Netease's risk control sees it.

export type NcmTransport = "remote" | "local";

let currentTransport: NcmTransport = "remote";

/**
 * Adapter axios would have used on its own. Captured before any override so
 * `local` can hand back anything it cannot represent — file uploads, progress
 * callbacks — instead of failing them.
 */
const baseAdapter = axios.getAdapter(axios.defaults.adapter) as AxiosAdapter;

const localAdapter = createLocalNcmAdapter(baseAdapter, (reason) => {
  // The embedded transport is the default, so a broken isolate would otherwise
  // fail every request in the app. Demote once and stay demoted for the
  // session; the user's setting is left alone so a restart retries.
  console.error(`[ncm] falling back to the remote API for this session: ${reason}`);
  setNcmTransport("remote");
});

/**
 * Select the transport. Safe to call repeatedly; forced to `remote` outside
 * Tauri, where there is no embedded protocol layer to talk to.
 */
export const setNcmTransport = (mode: NcmTransport): void => {
  const next = mode === "local" && isTauri() ? "local" : "remote";
  if (next === currentTransport) return;
  currentTransport = next;
  axios.defaults.adapter = next === "local" ? localAdapter : baseAdapter;
};

export const getNcmTransport = (): NcmTransport => currentTransport;

/**
 * Tell the embedded transport where to read the session cookie.
 *
 * Called from `main.ts` after Pinia hydrates. Re-exported here so callers only
 * ever need to know about `@/utils/request`.
 */
export const setNcmCookieSource = setNcmCookieProvider;

// 请求拦截
axios.interceptors.request.use(
  (request: CustomInternalAxiosRequestConfig) => {
    // Ensure headers object exists
    if (!request.headers) {
      request.headers = new AxiosHeaders();
    }

    // 确保使用默认的 baseURL
    request.baseURL = baseURL;
    // 确保凭据设置为 true
    request.withCredentials = true;
    // 确保 X-Requested-With 请求头存在
    if (!request.headers.has("X-Requested-With")) {
      request.headers.set("X-Requested-With", "XMLHttpRequest");
    }

    if (!request.hiddenBar && typeof $loadingBar !== "undefined") $loadingBar.start();
    return request;
  },
  (error: AxiosError) => {
    if (typeof $loadingBar !== "undefined") $loadingBar.error();
    console.error("请求失败，请稍后重试");
    return Promise.reject(error);
  },
);

// 响应拦截
axios.interceptors.response.use(
  (response: AxiosResponse) => {
    if (typeof $loadingBar !== "undefined") $loadingBar.finish();
    return response.data;
  },
  (error: AxiosError) => {
    if (typeof $loadingBar !== "undefined") $loadingBar.error();
    if (error.response) {
      const data = error.response.data as { message?: string };
      switch (error.response.status) {
        case 401:
          console.error(data.message ? data.message : "无权限访问");
          break;
        case 301:
          console.error(data.message ? data.message : "请求发生重定向");
          break;
        case 404:
          console.error(data.message ? data.message : "请求资源不存在");
          break;
        case 500:
          console.error(data.message ? data.message : "内部服务器错误");
          break;
        default:
          console.error(data.message ? data.message : "请求失败，请稍后重试");
          break;
      }
    } else {
      console.error("请求失败，请稍后重试");
    }
    return Promise.reject(error);
  },
);

// Create a typed request function
function request<T = any>(config: CustomAxiosRequestConfig): Promise<T> {
  return axios(config) as Promise<T>;
}

export { request };
export default request;
