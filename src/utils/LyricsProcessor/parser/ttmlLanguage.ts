/**
 * TTML 多语言翻译/音译的整首歌选择。
 *
 * 一个 TTML 可以同时带多种语言的翻译（行内 `x-translation` 或头部
 * `<translations>`）。AMLL 的 `parseTTML` 便捷函数对每一行各取第一条，于是某行
 * 缺了首选语言时会退到别的语言，一首歌里混出两种语言。这里先按整首歌选定一种
 * 语言，再把其余语言从每一行剔除，缺这一语言的行就不显示翻译。
 */

import { TTMLParser, toAmllLyrics } from "@applemusic-like-lyrics/ttml";
import type { AmllLyricResult, SubLyricContent, TTMLResult } from "@applemusic-like-lyrics/ttml";

/** 简繁与地区码折成脚本：zh-CN → zh-hans，zh-TW → zh-hant，其余小写原样 */
function normalizeTag(tag: string): string {
  const t = tag.toLowerCase().replace(/_/g, "-");
  if (t === "zh" || /^zh-(cn|sg|hans)/.test(t)) return "zh-hans";
  if (/^zh-(tw|hk|mo|hant)/.test(t)) return "zh-hant";
  return t;
}

/** 0 = 不相干；1 = 同语言不同文字；2 = 同语言且一方未写文字；3 = 完全一致 */
function languageScore(candidate: string, wanted: string): number {
  const c = normalizeTag(candidate);
  const w = normalizeTag(wanted);
  if (c === w) return 3;
  const [cPrimary, cRest] = [c.split("-")[0], c.split("-").length > 1];
  const [wPrimary, wRest] = [w.split("-")[0], w.split("-").length > 1];
  if (cPrimary !== wPrimary) return 0;
  return cRest && wRest ? 1 : 2;
}

/**
 * 在整首歌出现过的语言里挑一种：最贴近界面语言的；都不相干就取出现行数最多的，
 * 行数相同取先出现的。没有任何语言标注的轨道用 "" 表示。
 */
export function pickLanguage(
  perLine: readonly (readonly string[])[],
  wanted: string,
): string | null {
  const counts = new Map<string, number>();
  for (const languages of perLine) {
    for (const language of new Set(languages))
      counts.set(language, (counts.get(language) ?? 0) + 1);
  }

  let best: string | null = null;
  let bestScore = -1;
  let bestCount = 0;
  for (const [language, count] of counts) {
    const score = language ? languageScore(language, wanted) : 0;
    if (score > bestScore || (score === bestScore && count > bestCount)) {
      best = language;
      bestScore = score;
      bestCount = count;
    }
  }
  return best;
}

const languageOf = (content: SubLyricContent) => content.language ?? "";

function onlyLanguage(list: SubLyricContent[] | undefined, language: string | null) {
  if (!list) return list;
  const kept = list.filter((content) => languageOf(content) === language);
  return kept.length > 0 ? kept : undefined;
}

/** 整首歌出现过的、带标注的翻译语言，按首次出现的顺序 */
export function translationLanguagesOf(result: TTMLResult): string[] {
  const seen = new Set<string>();
  for (const line of result.lines) {
    for (const content of line.translations ?? []) if (content.language) seen.add(content.language);
  }
  return [...seen];
}

/** 按整首歌选定的语言过滤，原地修改解析结果（它是刚解析出来的，没有别的持有者） */
export function keepSongLanguages(
  result: TTMLResult,
  translationWanted: string,
  romanizationWanted: string,
): string | null {
  const trLang = pickLanguage(
    result.lines.map((line) => (line.translations ?? []).map(languageOf)),
    translationWanted,
  );
  const roLang = pickLanguage(
    result.lines.map((line) => (line.romanizations ?? []).map(languageOf)),
    romanizationWanted,
  );

  for (const line of result.lines) {
    line.translations = onlyLanguage(line.translations, trLang);
    line.romanizations = onlyLanguage(line.romanizations, roLang);
    const bg = line.backgroundVocal;
    if (bg) {
      bg.translations = onlyLanguage(bg.translations, trLang);
      bg.romanizations = onlyLanguage(bg.romanizations, roLang);
    }
  }
  return trLang;
}

/**
 * 解析 TTML：用户明确选过且这首歌里有的语言优先，否则跟随界面语言；音译通常只有
 * 一种（`ja-Latn` 之类），取行数最多的。
 */
export function parseTTMLForLanguage(ttmlText: string, uiLanguage: string, chosen: string) {
  const result = TTMLParser.parse(ttmlText);
  const translationLanguages = translationLanguagesOf(result);
  const wanted = chosen && translationLanguages.includes(chosen) ? chosen : uiLanguage;
  const translationLanguage = keepSongLanguages(result, wanted, "") ?? "";
  return { lyric: toAmllLyrics(result), translationLanguages, translationLanguage };
}
