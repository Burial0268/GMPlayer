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

const PROCESSING_CACHE_VERSION = "lyrics-processor-v2";

/**
 * 生成设置状态的哈希值
 */
function generateSettingsFlags(settings: ProcessingSettings): number {
  // Use bit flags for faster comparison (3 booleans = 3 bits)
  return (settings.showYrc ? 4 : 0) | (settings.showRoma ? 2 : 0) | (settings.showTransl ? 1 : 0);
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

function getCanonicalTTMLLines(songLyric: SongLyric, settings: ProcessingSettings): AMLLLine[] {
  const source = toRaw(songLyric.ttml) as unknown as AMLLLine[];
  const result: AMLLLine[] = [];
  result.length = source.length;

  let count = 0;
  const keepTransl = settings.showTransl;
  const keepRoma = settings.showRoma;

  for (let i = 0; i < source.length; i++) {
    const line = source[i];
    if (!hasRenderableWords(line)) continue;

    const shouldHideTransl = !keepTransl && Boolean(line.translatedLyric);
    const shouldHideLineRoma = !keepRoma && Boolean(line.romanLyric);
    const shouldHideWordRoma = !keepRoma && hasWordRomanization(line);

    if (!shouldHideTransl && !shouldHideLineRoma && !shouldHideWordRoma) {
      result[count++] = line;
      continue;
    }

    result[count++] = {
      ...line,
      translatedLyric: keepTransl ? (line.translatedLyric ?? "") : "",
      romanLyric: keepRoma ? (line.romanLyric ?? "") : "",
      words: shouldHideWordRoma
        ? line.words.map((word) => ({
            ...word,
            romanWord: "",
          }))
        : line.words,
    };
  }

  result.length = count;
  return result;
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
 * 处理歌词数据 - 使用行索引匹配 (优化版)
 *
 * @param songLyric 歌曲歌词
 * @param settings 设置状态
 * @returns 处理后的歌词行数组
 */
export function processLyrics(songLyric: SongLyric, settings: ProcessingSettings): AMLLLine[] {
  // TTML parser already returns canonical AMLL lines with millisecond timings.
  // Do not rebuild, realign, or infer timeline/content here.
  if (songLyric.hasTTML && songLyric.ttml && songLyric.ttml.length > 0) {
    return getCanonicalTTMLLines(songLyric, settings);
  }

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

  // 转换为 AMLL 格式
  const amllLines =
    sourceKind === "lrc"
      ? normalizeLrcTimeline(convertToAMLL(rawLyricsSource))
      : convertToAMLL(rawLyricsSource);

  // 过滤完全空白的行 - 使用更高效的过滤
  const validLines: AMLLLine[] = [];
  validLines.length = amllLines.length;
  let validCount = 0;

  for (let i = 0; i < amllLines.length; i++) {
    const line = amllLines[i];
    const words = line.words;
    if (!words || words.length === 0) continue;

    if (hasRenderableWords(line)) {
      validLines[validCount++] = line;
    }
  }
  validLines.length = validCount;

  // 根据设置清除源数据中的音译/翻译（TTML等格式可能自带这些数据）
  if (!settings.showRoma) {
    for (let i = 0; i < validCount; i++) {
      const line = validLines[i];
      line.romanLyric = "";
      const words = line.words;
      for (let j = 0; j < words.length; j++) {
        words[j].romanWord = "";
      }
    }
  }

  if (!settings.showTransl) {
    for (let i = 0; i < validCount; i++) {
      validLines[i].translatedLyric = "";
    }
  }

  // 早期返回：如果不需要翻译和音译
  if (!settings.showTransl && !settings.showRoma) {
    return validLines;
  }

  // 解析翻译和音译（只在需要时解析）
  let translationMatchMap: Map<number, string> | null = null;
  let romajiMatchMap: Map<number, string> | null = null;

  if (settings.showTransl) {
    const transText = extractTranslationText(songLyric);
    if (transText) {
      const translationEntries = parseLrcToEntries(transText);
      if (translationEntries.length > 0) {
        translationMatchMap = buildIndexMatching(validLines, translationEntries);
      }
    }
  }

  if (settings.showRoma) {
    const romaText = extractRomajiText(songLyric);
    if (romaText) {
      const romajiEntries = parseLrcToEntries(romaText);
      if (romajiEntries.length > 0) {
        romajiMatchMap = buildIndexMatching(validLines, romajiEntries);
      }
    }
  }

  // 如果没有任何匹配，直接返回
  if (!translationMatchMap && !romajiMatchMap) {
    return validLines;
  }

  // 应用翻译和音译 - 修改原数组以减少内存分配
  for (let i = 0; i < validCount; i++) {
    const line = validLines[i];

    // 处理翻译
    if (translationMatchMap) {
      // 优先使用行内已有的翻译（来自 TTML 等格式）
      if (!line.translatedLyric) {
        const matched = translationMatchMap.get(i);
        if (matched) {
          line.translatedLyric = matched;
        }
      }
    }

    // 处理音译
    if (romajiMatchMap) {
      if (!line.romanLyric) {
        const matched = romajiMatchMap.get(i);
        if (matched) {
          line.romanLyric = matched;
          // 尝试将匹配到的行级音译拆分为逐字
          const perWord = splitRomaToWords(line.words, matched);
          if (perWord) {
            for (let j = 0; j < line.words.length; j++) {
              line.words[j].romanWord = perWord[j] || "";
            }
            line.romanLyric = ""; // 避免与逐字音译重复
          }
        }
      }
    }
  }

  // 逐字音译去重：若逐字音译已存在（来自 TTML 等），清除行级音译
  if (settings.showRoma) {
    for (let i = 0; i < validCount; i++) {
      const line = validLines[i];
      if (!line.romanLyric || line.words.length === 0) continue;

      // 检查是否已有逐字音译
      let hasPerWordRoma = false;
      for (let j = 0; j < line.words.length; j++) {
        if (line.words[j].romanWord) {
          hasPerWordRoma = true;
          break;
        }
      }

      if (hasPerWordRoma) {
        line.romanLyric = ""; // 避免重复
      } else {
        // 尝试将行级音译拆分为逐字
        const perWord = splitRomaToWords(line.words, line.romanLyric);
        if (perWord) {
          for (let j = 0; j < line.words.length; j++) {
            line.words[j].romanWord = perWord[j] || "";
          }
          line.romanLyric = "";
        }
      }
    }
  }

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
