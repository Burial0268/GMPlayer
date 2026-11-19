/**
 * The md5 Netease declared for each audio URL it handed out (`song_url_v1`'s `md5`).
 *
 * The native backend keeps finished downloads in its audio cache keyed by this hash, because the
 * URL itself is re-signed on every resolve and cannot identify the file. Rust checks the downloaded
 * bytes against it before keeping anything, so a wrong or missing value only costs a re-download.
 *
 * A leaf module on purpose: `resolveSongUrl` writes it and the places that build native `SongData`
 * read it, and `NativeRustSound` must not import the resolver (and through it the stores and API)
 * just to look a value up.
 */

const declared = new Map<string, string>();
// Upcoming tracks are resolved a few at a time; this only has to outlive the gap to playback.
const LIMIT = 64;

export function recordDeclaredMd5(url: string, md5: unknown): void {
  declared.delete(url);
  if (typeof md5 !== "string" || !/^[0-9a-f]{32}$/i.test(md5)) return;
  declared.set(url, md5);
  if (declared.size > LIMIT) declared.delete(declared.keys().next().value as string);
}

export function declaredMd5For(url: string | undefined): string | undefined {
  return url ? declared.get(url) : undefined;
}
