/**
 * AMLL TTML DB — HTTP API client.
 *
 * Reaches the community lyric library by *metadata* instead of by platform id,
 * which is the only thing local files can offer: they carry tags, never an NCM
 * id. The static `ncm-lyrics/{id}.ttml` chain in `LyricsProcessor/service.ts`
 * cannot answer for them at all — nor for the ~5% of the library that carries no
 * `ncmMusicId` (周杰伦《东风破》has only QQ and Apple ids).
 *
 * Deliberately narrow: search takes the field-specific parameters and nothing
 * else. See `buildSearchParams` for why `q` is not offered.
 *
 * Docs: https://amll.dev/reference/http-api/overview
 */

/**
 * Mirror first, official second.
 *
 * The mirror is the one to prefer on traffic grounds, but it is not equivalent:
 * it lags the official host (3122 vs 3262 entries when last measured, which
 * `/v1/status` reports as `lyricCount`), and both hosts have been seen to answer
 * a 200 with an empty body. The fallback covers both cases.
 */
const HOSTS = ["https://amll-ttml-api.gbclstudio.cn", "https://api.amll.dev"] as const;

/** Same per-attempt budget the static-mirror chain in `service.ts` already uses. */
const REQUEST_TIMEOUT_MS = 8000;

/**
 * Cap on remembered lyric bodies. One TTML is tens of KB, so this is a real
 * memory decision rather than a formality.
 */
const MAX_CACHE_SIZE = 30;

/**
 * One library entry.
 *
 * Every name field is an array because one file may map to several titles,
 * artists or platform ids. `lyrics` is absent from search results and present
 * only on a `get`.
 */
export interface AmllSongItem {
  id: number;
  filename: string;
  musicNames: string[];
  artistNames: string[];
  albumNames: string[];
  ncmMusicIds: string[];
  qqMusicIds: string[];
  appleMusicIds: string[];
  spotifyIds: string[];
  isrcs: string[];
  authorIds: string[];
  authorUsernames: string[];
  lyrics?: string;
  format: "ttml";
  /** Present only when the hit came from the lyric body; `snippet` carries `<mark>`. */
  matchContext?: { snippet: string };
}

export interface AmllPagination {
  page: number;
  pageSize: number;
  /** Total matches, independent of `page`/`pageSize`. */
  total: number;
  totalPages: number;
  hasMore: boolean;
}

export interface AmllSearchResult {
  items: AmllSongItem[];
  pagination: AmllPagination;
}

/**
 * Search terms.
 *
 * All optional individually, but the API answers 400 unless at least one is set.
 * Several combine as an AND intersection, which is what makes title + artist a
 * usable auto-match.
 */
export interface AmllSearchQuery {
  musicName?: string;
  artistName?: string;
  albumName?: string;
  page?: number;
  pageSize?: number;
}

/** Why a call produced nothing, which the two cases need different wording for. */
export type AmllFailure =
  /** Every host answered 404 — the library really does not have it. */
  | "missing"
  /** No host could be reached, or all of them erred. Retrying may work. */
  | "failed";

/**
 * What a call produced.
 *
 * Discriminated on a string rather than on a boolean `ok`, because this project
 * compiles with `strict: false`. That widens `true`/`false` to `boolean`, so a
 * boolean discriminant never narrows — `if (o.ok) … else o.reason` fails to
 * compile — while a string one narrows under either setting.
 */
export type AmllOutcome<T> = { result: "ok"; data: T } | { result: AmllFailure };

/**
 * The API's discriminated envelope. Only `/v1/lyrics/*` uses it.
 *
 * The error arm lists the documented codes as literals rather than `number`, so
 * that `status !== 200` actually narrows — with `number` on both arms it cannot,
 * and every field access afterwards needs a cast.
 */
type AmllEnvelope<T> =
  | { status: 200; data: T }
  | { status: 400 | 401 | 404 | 405 | 429 | 500 | 502; error?: string; message?: string };

/** `id` → item, insertion-ordered so the oldest key is the first one. */
const detailCache = new Map<number, AmllSongItem>();

/**
 * Try each host in turn until one gives a usable answer.
 *
 * Note there is no `Accept-Encoding` header here: it is a forbidden header name
 * for `fetch`, so setting it would be silently dropped. The browser negotiates
 * gzip on its own, which takes a single TTML from ~46 KB down to ~9 KB on the
 * wire — worth knowing, not worth asking for.
 */
async function call<T>(path: string, params: URLSearchParams): Promise<AmllOutcome<T>> {
  // Only a 404 from *every* host is a real miss. One host answering 404 while
  // another times out says nothing about whether the library has the entry.
  let missing = 0;

  for (const host of HOSTS) {
    const url = `${host}${path}?${params.toString()}`;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);

    try {
      const response = await fetch(url, { signal: controller.signal });

      if (response.status === 404) {
        missing += 1;
        continue;
      }
      if (!response.ok) {
        console.warn(`[amll] ${host} answered ${response.status} for ${path}`);
        continue;
      }

      const text = await response.text();
      // Both hosts have been observed answering 200 with an empty body. Letting
      // that reach JSON.parse would report a syntax error for what is really a
      // transport hiccup the next host can cover.
      if (!text.trim()) {
        console.warn(`[amll] ${host} answered 200 with an empty body for ${path}`);
        continue;
      }

      const envelope = JSON.parse(text) as AmllEnvelope<T>;
      if (envelope.status !== 200) {
        if (envelope.status === 404) {
          missing += 1;
          continue;
        }
        console.warn(`[amll] ${host} rejected ${path}: ${envelope.message ?? envelope.status}`);
        continue;
      }
      if (envelope.data === undefined) {
        console.warn(`[amll] ${host} answered 200 with no data for ${path}`);
        continue;
      }

      return { result: "ok", data: envelope.data };
    } catch (error) {
      const aborted = error instanceof Error && error.name === "AbortError";
      console.warn(`[amll] ${host} ${aborted ? "timed out" : "failed"} for ${path}`, error);
    } finally {
      clearTimeout(timer);
    }
  }

  return { result: missing === HOSTS.length ? "missing" : "failed" };
}

/**
 * Spell the search terms.
 *
 * `q` is deliberately not offered. It is a fuzzy scorer rather than a matcher and
 * it does not index platform ids: measured against both hosts, `q=1952182912`
 * answers with Adele's "Rolling in the Deep" — whose NCM id is 16435051 — a
 * fabricated `q=1952182913` answers with the same entry, and `q=16435051`, that
 * entry's real NCM id, answers with nothing at all. It also reaches `authorId`,
 * which is where the numeric false positives come from. So a lookup keyed on an
 * id has to go through `/v1/lyrics/get`, and a text search has to name the field
 * it means. Exposing `q` would return confidently wrong songs.
 */
function buildSearchParams(query: AmllSearchQuery): URLSearchParams | null {
  const params = new URLSearchParams();
  const put = (key: string, value?: string) => {
    const trimmed = value?.trim();
    if (trimmed) params.set(key, trimmed);
  };

  put("musicName", query.musicName);
  put("artistName", query.artistName);
  put("albumName", query.albumName);

  // The API answers 400 — not "everything" — when no term is given, and
  // pagination alone does not satisfy it. Answer an empty form here rather than
  // spending a round trip to be told so.
  if (params.size === 0) return null;

  if (query.page !== undefined) params.set("page", String(query.page));
  if (query.pageSize !== undefined) params.set("pageSize", String(query.pageSize));
  return params;
}

/** Search the library. Terms combine as an AND intersection. */
export const searchAmllLyrics = async (
  query: AmllSearchQuery,
): Promise<AmllOutcome<AmllSearchResult>> => {
  const params = buildSearchParams(query);
  if (!params) return { result: "missing" };
  return call<AmllSearchResult>("/v1/lyrics/search", params);
};

/**
 * Separators a file's artist tag may use to hold more than one name.
 *
 * The set is deliberately generous: under-splitting is what actually breaks a
 * search, and over-splitting is cheap because `artistName` matches by substring
 * — measured, `artistName=aylor Swif` returns every Taylor Swift entry — so a
 * truncated "Simon" still finds "Simon & Garfunkel".
 *
 * That escape hatch is Latin-only. CJK is tokenized rather than matched by
 * substring (`周杰伦` matches, `周杰` returns nothing), so anything added here must
 * still cut between names and never inside one.
 */
const ARTIST_SEPARATORS = /[、,;/&|]|\sfeat\.?\s|\sft\.?\s|\swith\s|\sx\s/i;

/**
 * Reduce an artist tag to the one name worth sending.
 *
 * `artistName` is an AND term compared against a single entry of the library's
 * `artistNames[]` at a time, so handing it a joined tag value matches nothing at
 * all: no entry contains the whole string "Steam Phunk、Lucy Neville". The
 * separator comes from whoever tagged the file, not from us, which is why this
 * guesses rather than parses.
 */
export const amllPrimaryArtist = (artist: string): string =>
  artist.split(ARTIST_SEPARATORS)[0]?.trim() ?? "";

/**
 * Fetch one entry's full TTML.
 *
 * Cached without expiry: the docs guarantee the content behind a given `id` never
 * changes, because a correction is published as a new file under a new id rather
 * than overwriting the old one.
 */
export const getAmllLyricById = async (id: number): Promise<AmllOutcome<AmllSongItem>> => {
  const cached = detailCache.get(id);
  if (cached) return { result: "ok", data: cached };

  const outcome = await call<AmllSongItem>(
    "/v1/lyrics/get",
    new URLSearchParams({ id: String(id) }),
  );
  if (outcome.result !== "ok") return outcome;
  // An entry with no body cannot be imported as a lyric, and remembering it
  // would make the miss permanent for the rest of the session.
  if (!outcome.data.lyrics?.trim()) return { result: "missing" };

  if (detailCache.size >= MAX_CACHE_SIZE) {
    const oldest = detailCache.keys().next().value;
    if (oldest !== undefined) detailCache.delete(oldest);
  }
  detailCache.set(id, outcome.data);
  return outcome;
};
