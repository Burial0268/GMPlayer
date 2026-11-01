/**
 * LyricsProcessor - 歌词处理核心模块 (优化版)
 *
 * 主要功能：
 * 1. 将歌词源数据转换为 AMLL 格式
 * 2. 严格的行索引匹配（第N行翻译对应第N行歌词）
 * 3. 缓存处理结果
 */

import { toRaw } from "vue";
import type { LyricLine as AMLLLine } from "@applemusic-like-lyrics/core";
import type { SongLyric, ProcessingSettings, InputLyricLine, StoredLyricLine } from "./types";
import { parseLrcToEntries } from "./parser/entryParser";
import { buildIndexMatching } from "./alignment";
import { convertToAMLL, splitRomaToWords } from "./parser/formatParser";

const PROCESSING_CACHE_VERSION = "lyrics-processor-v3";

/**
 * 生成设置状态的哈希值
 */
function generateSettingsFlags(settings: ProcessingSettings): number {
  return (
    (settings.showRuby ? 8 : 0) |
    (settings.showYrc ? 4 : 0) |
    (settings.showRoma ? 2 : 0) |
    (settings.showTransl ? 1 : 0)
  );
}

function finiteTime(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function sourceLineStart(line: InputLyricLine | StoredLyricLine | undefined): number {
  if (!line) return 0;
  return finiteTime(line.startTime) || finiteTime(line.words?.[0]?.startTime);
}

function sourceLineEnd(line: InputLyricLine | StoredLyricLine | undefined): number {
  if (!line) return 0;
  const explicitEnd = finiteTime(line.endTime);
  if (explicitEnd) return explicitEnd;

  const words = line.words;
  if (!words?.length) return 0;

  let endTime = 0;
  for (let i = 0; i < words.length; i++) {
    const wordEnd = finiteTime(words[i].endTime);
    if (wordEnd > endTime) endTime = wordEnd;
  }
  return endTime;
}

function buildSourceFingerprint(source: readonly (InputLyricLine | StoredLyricLine)[]): string {
  const len = source.length;
  if (len === 0) return "0";

  const first = source[0];
  const last = source[len - 1];
  return `${len}:${Math.round(sourceLineStart(first))}:${Math.round(sourceLineEnd(last))}`;
}

function selectSourceForHash(
  songLyric: SongLyric,
  settings: ProcessingSettings,
): { kind: string; source: readonly (InputLyricLine | StoredLyricLine)[] } {
  if (songLyric.hasTTML && songLyric.ttml?.length) {
    return { kind: "ttml", source: songLyric.ttml };
  }
  if (settings.showYrc && songLyric.yrcAMData?.length) {
    return { kind: "yrc", source: songLyric.yrcAMData };
  }
  if (songLyric.lrcAMData?.length) {
    return { kind: "lrc", source: songLyric.lrcAMData };
  }
  return { kind: "empty", source: [] };
}

function generateProcessingHash(songLyric: SongLyric, settings: ProcessingSettings): string {
  const selected = selectSourceForHash(songLyric, settings);
  return `${PROCESSING_CACHE_VERSION}:${generateSettingsFlags(settings)}:${selected.kind}:${buildSourceFingerprint(selected.source)}`;
}

/**
 * 从 SongLyric 中提取翻译文本 (优化版 - 减少对象访问)
 */
function extractTranslationText(songLyric: SongLyric): string | undefined {
  const tlyric = songLyric.tlyric;
  if (tlyric?.lyric) return tlyric.lyric;

  const translation = songLyric.translation;
  if (typeof translation === "string") return translation;
  if (translation && typeof translation === "object" && "lyric" in translation) {
    return translation.lyric;
  }
  return undefined;
}

/**
 * 从 SongLyric 中提取音译文本 (优化版 - 减少对象访问)
 */
function extractRomajiText(songLyric: SongLyric): string | undefined {
  const romalrc = songLyric.romalrc;
  if (romalrc?.lyric) return romalrc.lyric;

  const romaji = songLyric.romaji;
  if (typeof romaji === "string") return romaji;
  if (romaji && typeof romaji === "object" && "lyric" in romaji) {
    return romaji.lyric;
  }
  return undefined;
}

function hasRenderableWords(line: AMLLLine): boolean {
  const words = line.words;
  if (!words || words.length === 0) return false;

  for (let i = 0; i < words.length; i++) {
    const word = words[i].word;
    if (word && word.trim().length > 0) return true;
  }

  return false;
}

function hasWordRomanization(line: AMLLLine): boolean {
  const words = line.words;
  for (let i = 0; i < words.length; i++) {
    if (words[i].romanWord) return true;
  }
  return false;
}

function getMedianPositiveGap(lines: readonly AMLLLine[]): number {
  const starts: number[] = [];
  starts.length = lines.length;

  let count = 0;
  for (let i = 0; i < lines.length; i++) {
    const start = finiteTime(lines[i].startTime) || finiteTime(lines[i].words?.[0]?.startTime);
    if (start > 0) starts[count++] = start;
  }
  starts.length = count;

  if (count < 4) return 0;

  starts.sort((a, b) => a - b);
  const gaps: number[] = [];
  gaps.length = count - 1;

  let gapCount = 0;
  for (let i = 1; i < count; i++) {
    const gap = starts[i] - starts[i - 1];
    if (gap > 0) gaps[gapCount++] = gap;
  }
  gaps.length = gapCount;

  if (gapCount < 3) return 0;

  gaps.sort((a, b) => a - b);
  return gaps[gapCount >> 1];
}

function shouldRescaleInflatedLrcTimeline(lines: readonly AMLLLine[]): boolean {
  const medianGap = getMedianPositiveGap(lines);
  if (medianGap <= 0) return false;

  // Some LRC sources encode [ss:cc.xx], but AMLL parses it as [mm:ss.xx].
  // That inflates adjacent line gaps by roughly 60x, e.g. 11.003s -> 663000ms.
  const scaledMedianGap = medianGap / 100;
  return medianGap >= 30000 && scaledMedianGap >= 300 && scaledMedianGap <= 15000;
}

function recoverInflatedLrcTime(timeMs: number): number {
  const normalized = Math.max(0, Math.round(timeMs));
  const secondsPart = Math.floor(normalized / 60000);
  const fractionPart = Math.round((normalized % 60000) / 1000);
  const fractionMs = fractionPart < 10 ? fractionPart : fractionPart * 10;
  return secondsPart * 1000 + fractionMs;
}

function recoverInflatedLrcLine(line: AMLLLine): AMLLLine {
  return {
    ...line,
    startTime: recoverInflatedLrcTime(line.startTime),
    endTime: recoverInflatedLrcTime(line.endTime),
    words: line.words.map((word) => ({
      ...word,
      startTime: recoverInflatedLrcTime(word.startTime),
      endTime: recoverInflatedLrcTime(word.endTime),
    })),
  };
}

function normalizeLrcTimeline(lines: AMLLLine[]): AMLLLine[] {
  if (!shouldRescaleInflatedLrcTimeline(lines)) return lines;

  const result: AMLLLine[] = [];
  result.length = lines.length;
  for (let i = 0; i < lines.length; i++) {
    result[i] = recoverInflatedLrcLine(lines[i]);
  }
  return result;
}

/**
 * 整首歌一起决定逐字音译：只要有一行自带逐字音译，所有行都只显示逐字音译
 * （没有的行就不显示，不用行级音译补）；否则只有每一行的行级音译都能拆成
 * 逐字时才拆，否则整首歌保持行级。逐行判断会让同一首歌里两种样式混着出现。
 */
function unifyRomanization(lines: AMLLLine[], own: (i: number) => AMLLLine): void {
  let nativeWord = false;
  let anyLine = false;
  for (let i = 0; i < lines.length; i++) {
    if (hasWordRomanization(lines[i])) nativeWord = true;
    if (lines[i].romanLyric) anyLine = true;
  }

  if (nativeWord) {
    for (let i = 0; i < lines.length; i++) {
      if (lines[i].romanLyric) own(i).romanLyric = "";
    }
    return;
  }
  if (!anyLine) return;

  const splits = new Map<number, string[]>();
  for (let i = 0; i < lines.length; i++) {
    const roma = lines[i].romanLyric;
    if (!roma) continue;
    const perWord = splitRomaToWords(lines[i].words, roma);
    if (!perWord) return;
    splits.set(i, perWord);
  }
  for (const [i, perWord] of splits) {
    const line = own(i);
    for (let j = 0; j < line.words.length; j++) line.words[j].romanWord = perWord[j] || "";
    line.romanLyric = "";
  }
}

/**
 * 处理歌词数据 - 使用行索引匹配
 *
 * TTML 的行已是毫秒时间的规范 AMLL 行，不再重建或重新对齐，但和其他来源走同一段
 * 后处理（翻译/音译开关、tlyric/romalrc 补全、整首歌统一逐字音译）。TTML 行是
 * store 里的原数据，只在要改动时才复制那一行。
 *
 * @param songLyric 歌曲歌词
 * @param settings 设置状态
 * @returns 处理后的歌词行数组
 */
export function processLyrics(songLyric: SongLyric, settings: ProcessingSettings): AMLLLine[] {
  let validLines: AMLLLine[];
  // 非 TTML 来源的行由 convertToAMLL 新建，可以直接改
  let ownsLines = true;

  if (songLyric.hasTTML && songLyric.ttml && songLyric.ttml.length > 0) {
    validLines = (toRaw(songLyric.ttml) as unknown as AMLLLine[]).filter(hasRenderableWords);
    ownsLines = false;
  } else {
    // 选择歌词源：YRC > LRC
    let rawLyricsSource: InputLyricLine[];

    const hasYrc = settings.showYrc && songLyric.yrcAMData && songLyric.yrcAMData.length > 0;
    const hasLrc = songLyric.lrcAMData && songLyric.lrcAMData.length > 0;
    let sourceKind: "yrc" | "lrc";

    if (hasYrc) {
      rawLyricsSource = toRaw(songLyric.yrcAMData) as InputLyricLine[];
      sourceKind = "yrc";
    } else if (hasLrc) {
      rawLyricsSource = toRaw(songLyric.lrcAMData) as InputLyricLine[];
      sourceKind = "lrc";
    } else {
      return [];
    }

    const amllLines =
      sourceKind === "lrc"
        ? normalizeLrcTimeline(convertToAMLL(rawLyricsSource))
        : convertToAMLL(rawLyricsSource);

    validLines = amllLines.filter(hasRenderableWords);
  }

  const owned = new Set<AMLLLine>();
  const own = (i: number): AMLLLine => {
    const line = validLines[i];
    if (ownsLines || owned.has(line)) return line;
    const copy = { ...line, words: line.words.map((word) => ({ ...word })) };
    owned.add(copy);
    validLines[i] = copy;
    return copy;
  };

  if (!settings.showRuby) {
    for (let i = 0; i < validLines.length; i++) {
      if (!validLines[i].words.some((word) => word.ruby)) continue;
      for (const word of own(i).words) delete word.ruby;
    }
  }

  // 根据设置清除源数据中的音译/翻译（TTML等格式可能自带这些数据）
  if (!settings.showRoma) {
    for (let i = 0; i < validLines.length; i++) {
      if (!validLines[i].romanLyric && !hasWordRomanization(validLines[i])) continue;
      const line = own(i);
      line.romanLyric = "";
      for (const word of line.words) word.romanWord = "";
    }
  }

  if (!settings.showTransl) {
    for (let i = 0; i < validLines.length; i++) {
      if (validLines[i].translatedLyric) own(i).translatedLyric = "";
    }
  }

  if (!settings.showTransl && !settings.showRoma) return validLines;

  // 解析翻译和音译（只在需要时解析）
  let translationMatchMap: Map<number, string> | null = null;
  let romajiMatchMap: Map<number, string> | null = null;

  // 来源自带翻译/音译（TTML）就整首歌都以它为准：TTML 导出的 tlyric/romalrc 只含
  // 有内容的行，按序号再匹配一遍会把它们错位贴到没有翻译的行上
  if (settings.showTransl && !validLines.some((line) => line.translatedLyric)) {
    const transText = extractTranslationText(songLyric);
    if (transText) {
      const translationEntries = parseLrcToEntries(transText);
      if (translationEntries.length > 0) {
        translationMatchMap = buildIndexMatching(validLines, translationEntries);
      }
    }
  }

  if (
    settings.showRoma &&
    !validLines.some((line) => line.romanLyric || hasWordRomanization(line))
  ) {
    const romaText = extractRomajiText(songLyric);
    if (romaText) {
      const romajiEntries = parseLrcToEntries(romaText);
      if (romajiEntries.length > 0) {
        romajiMatchMap = buildIndexMatching(validLines, romajiEntries);
      }
    }
  }

  for (let i = 0; i < validLines.length; i++) {
    const translated = translationMatchMap?.get(i);
    if (translated) own(i).translatedLyric = translated;

    const roma = romajiMatchMap?.get(i);
    if (roma) own(i).romanLyric = roma;
  }

  if (settings.showRoma) unifyRomanization(validLines, own);

  return validLines;
}

/**
 * 预处理并缓存歌词数据 (优化版)
 *
 * @param songLyric 歌曲歌词
 * @param settings 设置状态
 */
export function preprocessLyrics(songLyric: SongLyric, settings: ProcessingSettings): void {
  const currentHash = generateProcessingHash(songLyric, settings);

  // 检查缓存
  if (
    songLyric.processedLyrics &&
    songLyric.processedLyrics.length > 0 &&
    songLyric.settingsHash === currentHash
  ) {
    return;
  }

  songLyric.processedLyrics = processLyrics(songLyric, settings) as unknown as StoredLyricLine[];
  songLyric.settingsHash = currentHash;
}

/**
 * 获取处理后的歌词行，优先使用缓存 (优化版)
 *
 * @param songLyric 歌曲歌词
 * @param settings 设置状态
 * @returns 处理后的歌词行数组
 */
export function getProcessedLyrics(songLyric: SongLyric, settings: ProcessingSettings): AMLLLine[] {
  const currentHash = generateProcessingHash(songLyric, settings);

  // 检查缓存 - StoredLyricLine 与 AMLLLine 结构兼容
  const cached = songLyric.processedLyrics;
  if (cached && cached.length > 0 && songLyric.settingsHash === currentHash) {
    return cached as unknown as AMLLLine[];
  }

  // 重新处理并缓存
  const processed = processLyrics(songLyric, settings);
  songLyric.processedLyrics = processed as unknown as StoredLyricLine[];
  songLyric.settingsHash = currentHash;

  return processed;
}
