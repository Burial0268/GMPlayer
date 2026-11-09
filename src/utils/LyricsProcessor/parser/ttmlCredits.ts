import type { AmllMetadata } from "@applemusic-like-lyrics/ttml";
import type { LyricCredits } from "../types";

/** 从 toAmllLyrics 的元数据里取尾栏署名，两项都没有时返回 undefined */
export function ttmlCreditsOf(metadata: readonly AmllMetadata[]): LyricCredits | undefined {
  const values = (key: string) =>
    metadata
      .filter(([name]) => name === key)
      .flatMap(([, list]) => list)
      .map((value) => value.trim())
      .filter(Boolean);
  const authors = [...new Set(values("ttmlAuthorGithubLogin"))];
  const songwriters = [...new Set(values("songwriters"))];
  return authors.length || songwriters.length ? { authors, songwriters } : undefined;
}
