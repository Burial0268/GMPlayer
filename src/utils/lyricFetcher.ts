import { getUnifiedLyric } from "@/api/song";
import { parseLyricData } from "@/utils/LyricsProcessor";
import type { ParsedLyricResult } from "@/utils/LyricsProcessor";
import useSettingDataStore from "@/store/settingData";

// Cached entries are handed to the store by reference, and the processing layer
// attaches derived data (processedLyrics) to whatever object it is given — so a
// cached slot would otherwise keep a full word-level object graph alive for
// every track ever played, on top of its own parse output. releaseDerived()
// strips that graph from every slot except the live track, which keeps the
// steady-state cost proportional to *one* song rather than to MAX_CACHE_SIZE.
const MAX_CACHE_SIZE = 20;

interface CacheEntry {
  result: ParsedLyricResult;
  useTTMLRepo: boolean;
}

/** Fields the processing layer attaches to a lyric object after parsing. */
interface DerivedLyricFields {
  processedLyrics?: unknown;
  settingsHash?: string;
}

class LyricFetcher {
  /** LRU cache: songId → parsed result */
  private _cache = new Map<number, CacheEntry>();

  /** Currently in-flight promises (dedup same-id concurrent calls) */
  private _pending = new Map<number, Promise<ParsedLyricResult>>();

  /** Monotonic counter — only the latest call's result gets applied */
  private _generation = 0;

  /**
   * Fetch, parse, and cache lyrics for a song.
   * Returns `{ result, stale }`:
   *   - `result`: the parsed lyric data (from cache or network)
   *   - `stale`: true if a newer fetchLyric() call was made while this one was in-flight
   */
  async fetchLyric(id: number): Promise<{ result: ParsedLyricResult; stale: boolean }> {
    const generation = ++this._generation;
    const settingStore = useSettingDataStore();
    const useTTMLRepo = settingStore.useTTMLRepo;

    // 1. Cache hit (setting must match)
    const cached = this._cache.get(id);
    if (cached && cached.useTTMLRepo === useTTMLRepo) {
      // LRU touch: delete + re-insert to move to end
      this._cache.delete(id);
      this._cache.set(id, cached);
      this._releaseDerived(id);
      return { result: cached.result, stale: generation !== this._generation };
    }

    // 2. In-flight dedup: if the same id is already being fetched, reuse its promise
    const pending = this._pending.get(id);
    if (pending) {
      const result = await pending;
      this._releaseDerived(id);
      return { result, stale: generation !== this._generation };
    }

    // 3. Network fetch + parse
    const promise = this._doFetch(id, useTTMLRepo);
    this._pending.set(id, promise);

    try {
      const result = await promise;

      // Cache the result (evict oldest if over limit)
      if (this._cache.size >= MAX_CACHE_SIZE) {
        const oldest = this._cache.keys().next().value;
        if (oldest !== undefined) {
          this._cache.delete(oldest);
        }
      }
      this._cache.set(id, { result, useTTMLRepo });
      this._releaseDerived(id);

      return { result, stale: generation !== this._generation };
    } finally {
      this._pending.delete(id);
    }
  }

  /**
   * Drop processing-layer output from every cached entry except `keepId`.
   *
   * processedLyrics is a full word-level object graph — for a word-by-word track
   * it dwarfs the parse output it hangs off. Only the track being played needs
   * it; for any other slot it is recoverable in a few milliseconds by
   * re-running processLyrics(), which is a far better trade than keeping up to
   * MAX_CACHE_SIZE of them resident. Clearing settingsHash alongside it is what
   * makes the drop safe: getProcessedLyrics() treats a missing hash as a miss
   * and rebuilds, so a re-visited track cannot read a stale graph.
   */
  private _releaseDerived(keepId: number | null): void {
    for (const [id, entry] of this._cache) {
      if (id === keepId) continue;
      const derived = entry.result as ParsedLyricResult & DerivedLyricFields;
      if (derived.processedLyrics === undefined && derived.settingsHash === undefined) continue;
      derived.processedLyrics = undefined;
      derived.settingsHash = undefined;
    }
  }

  /**
   * Drop derived data from every cached entry, including the most recent one.
   * Called when the queue is cleared — no track is live, so nothing needs its
   * processed graph kept warm.
   */
  releaseDerived(): void {
    this._releaseDerived(null);
  }

  private async _doFetch(id: number, useTTMLRepo: boolean): Promise<ParsedLyricResult> {
    const lyricData = await getUnifiedLyric(id, useTTMLRepo);
    return parseLyricData(lyricData);
  }

  /** Invalidate a specific song's cached lyrics */
  invalidate(id: number): void {
    this._cache.delete(id);
  }

  /** Clear entire cache */
  clear(): void {
    this._cache.clear();
  }
}

/** Singleton */
export const lyricFetcher = new LyricFetcher();
