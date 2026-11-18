/**
 * The one place that turns a song's `album.picUrl` into a URL an `<img>` can load.
 *
 * ## Why this exists
 *
 * `album.picUrl` carried an implicit contract for the whole life of this app:
 * *a Netease CDN URL, possibly plain `http:`, and accepting a `?param=WxH`
 * resize hint*. Roughly forty call sites encoded that contract by hand as
 * `picUrl.replace(/^http:/, "https:") + "?param=60y60"`.
 *
 * An imported local file breaks both halves. Its cover is a real file on disk,
 * referenced through Tauri's asset protocol — which on Windows and Android is
 * `http://asset.localhost/<percent-encoded path>` (the scheme is `https` only
 * when a window opts into `useHttpsScheme`, which this app does not). So the
 * unconditional rewrite turned it into `https://asset.localhost/…`: an origin
 * with **no** protocol handler registered, which the WebView then tried to fetch
 * over the network and failed with `ERR_CONNECTION_REFUSED`. The `?param=` hint
 * is harmless in comparison — Tauri's asset handler reads only `uri().path()` —
 * but it is the same mistake of treating a local file as a CDN object.
 *
 * Keeping the knowledge here rather than at each call site means a third kind of
 * cover only has to teach one function.
 */

/** Shown when a song has no cover at all. */
export const DEFAULT_COVER = "/images/pic/default.png";

/**
 * Whether `url` is a local asset reference rather than a remote image.
 *
 * Covers every form `convertFileSrc` can produce: `asset://localhost/…` on
 * macOS/Linux, and `http(s)://asset.localhost/…` on Windows/Android.
 */
export const isAssetUrl = (url: string): boolean =>
  url.startsWith("asset://") ||
  url.startsWith("http://asset.localhost/") ||
  url.startsWith("https://asset.localhost/");

/**
 * A loadable cover URL.
 *
 * @param url  Raw `album.picUrl`, or anything falsy.
 * @param size Square resize hint in CSS pixels. Applied only to Netease URLs;
 *             a local asset is served as-is.
 */
export const coverUrl = (url: string | null | undefined, size?: number): string => {
  if (typeof url !== "string" || !url) return DEFAULT_COVER;
  // Neither the scheme rewrite nor the resize hint applies to a file on disk,
  // and the rewrite actively breaks it.
  if (isAssetUrl(url)) return url;
  const secure = url.replace(/^http:/, "https:");
  return size ? `${secure}?param=${size}y${size}` : secure;
};

/**
 * Non-square variant, for the few places that ask for one.
 */
export const coverUrlSized = (
  url: string | null | undefined,
  width: number,
  height: number,
): string => {
  if (typeof url !== "string" || !url) return DEFAULT_COVER;
  if (isAssetUrl(url)) return url;
  return `${url.replace(/^http:/, "https:")}?param=${width}x${height}`;
};
