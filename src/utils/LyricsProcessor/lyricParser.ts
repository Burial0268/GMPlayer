/**
 * LyricsProcessor Lyric Parser
 * 歌词解析主模块 (优化版)
 */

import {
  parseLrc as parseAMLLLrc,
  parseYrc as parseAMLLYrc,
  type LyricLine as AMLLParsedLyricLine,
} from "@applemusic-like-lyrics/lyric";
import { musicStore } from "@/store";
import {
  parseLrcLines as convertLrcLines,
  parseYrcLines as convertYrcLines,
  buildAMLLData,
} from "./parser/formatParser";
import { alignByIndex } from "./alignment";
import type { RawLyricData, ParsedLyricResult } from "./types";

type ParsedSourceLine = AMLLParsedLyricLine;

const parseLrcText = (lyricText: string): ParsedSourceLine[] => parseAMLLLrc(lyricText);
const parseYrcText = (lyricText: string): ParsedSourceLine[] => parseAMLLYrc(lyricText);

function finiteNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function lineStartTime(line: ParsedSourceLine): number | undefined {
  return finiteNumber(line.startTime) ?? finiteNumber(line.words?.[0]?.startTime);
}

function detectTTMLTimeScale(lines: ParsedSourceLine[]): 1 | 1000 {
  if (lines.length < 2) return 1;

  const starts: number[] = [];
  let maxTime = 0;

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const start = lineStartTime(line);
    if (start !== undefined) starts.push(start);
    maxTime = Math.max(maxTime, start ?? 0, line.endTime ?? 0);

    const words = line.words;
    for (let j = 0; j < words.length; j++) {
      maxTime = Math.max(maxTime, words[j].startTime, words[j].endTime);
    }
  }

  // AMLL lyric parsers use milliseconds. Only scale when the whole timeline
  // looks like seconds; this avoids multiplying valid short millisecond clips.
  if (maxTime <= 0 || maxTime > 3600 || starts.length < 2) return 1;

  starts.sort((a, b) => a - b);
  const gaps: number[] = [];
  for (let i = 1; i < starts.length; i++) {
    const gap = starts[i] - starts[i - 1];
    if (gap > 0) gaps.push(gap);
  }
  if (!gaps.length) return 1;

  gaps.sort((a, b) => a - b);
  const medianGap = gaps[gaps.length >> 1];
  return medianGap > 0 && medianGap < 30 ? 1000 : 1;
}

function normalizeTTMLLines(lines: ParsedSourceLine[]): ParsedSourceLine[] {
  const scale = detectTTMLTimeScale(lines);
  if (scale === 1) return lines;

  return lines.map((line) => ({
    ...line,
    startTime: line.startTime * scale,
    endTime: line.endTime * scale,
    words: line.words.map((word) => ({
      ...word,
      startTime: word.startTime * scale,
      endTime: word.endTime * scale,
    })),
  }));
}

// Backward compat alias
export type LyricData = RawLyricData;

// Creates the default empty state
export const createEmptyLyricResult = (): ParsedLyricResult => ({
  hasLrcTran: false,
  hasLrcRoma: false,
  hasYrc: false,
  hasYrcTran: false,
  hasYrcRoma: false,
  hasTTML: false,
  lrc: [],
  yrc: [],
  ttml: [],
  lrcAMData: [],
  yrcAMData: [],
  formattedLrc: "",
});

// 恢复默认
export const resetSongLyric = (): void => {
  const music = musicStore();
  const defaultLyric = createEmptyLyricResult();
  // @ts-ignore
  music.songLyric = { ...defaultLyric } as any;
};

/**
 * Parse lyric data from API response (优化版)
 * @param data API response data or null on fetch error
 * @returns Parsed lyric data (always returns a valid object)
 */
export const parseLyricData = (data: RawLyricData | null): ParsedLyricResult => {
  if (!data || data.code !== 200) {
    return createEmptyLyricResult();
  }

  const result: ParsedLyricResult = createEmptyLyricResult();

  try {
    const { lrc, tlyric, romalrc, yrc, ytlrc, yromalrc } = data;
    const lrcData = {
      lrc: lrc?.lyric || null,
      tlyric: tlyric?.lyric || null,
      romalrc: romalrc?.lyric || null,
      yrc: yrc?.lyric || null,
      ytlrc: ytlrc?.lyric || null,
      yromalrc: yromalrc?.lyric || null,
    };
    const directTTMLLines =
      data.hasTTML && Array.isArray(data.ttml) && data.ttml.length > 0
        ? normalizeTTMLLines(data.ttml as ParsedSourceLine[])
        : [];

    // --- LAAPI data parsing ---
    let laapiTranslationLyricLines: ParsedSourceLine[] | null = null;
    const laapiTranslation = (data as any).translation;
    if (laapiTranslation && typeof laapiTranslation === "string" && laapiTranslation.trim()) {
      try {
        const laapiTranslationText = laapiTranslation.replace(/\\n/g, "\n").replace(/\r/g, "");
        const parsedLines = parseLrcText(laapiTranslationText);
        if (parsedLines && parsedLines.length > 0) {
          laapiTranslationLyricLines = parsedLines;
        }
      } catch {
        // Silently fail for LAAPI parsing
      }
    }

    let laapiRomajiLyricLines: ParsedSourceLine[] | null = null;
    const laapiRomaji = (data as any).romaji;
    if (laapiRomaji && typeof laapiRomaji === "string" && laapiRomaji.trim()) {
      try {
        const laapiRomajiText = laapiRomaji.replace(/\\n/g, "\n").replace(/\r/g, "");
        laapiRomajiLyricLines = parseLrcText(laapiRomajiText);
      } catch {
        // Silently fail for LAAPI parsing
      }
    }

    // --- Determine effective sources and update flags ---
    result.hasYrc = !!lrcData.yrc || directTTMLLines.length > 0;

    // Effective LRC translation source
    let effectiveLrcTranSource: ParsedSourceLine[] = [];
    if (lrcData.tlyric?.trim()) {
      effectiveLrcTranSource = parseLrcText(lrcData.tlyric);
    } else if (laapiTranslationLyricLines?.length) {
      effectiveLrcTranSource = laapiTranslationLyricLines;
    }
    result.hasLrcTran = effectiveLrcTranSource.length > 0;

    // Effective LRC romaji source
    let effectiveLrcRomaSource: ParsedSourceLine[] = [];
    if (lrcData.romalrc?.trim()) {
      effectiveLrcRomaSource = parseLrcText(lrcData.romalrc);
    } else if (laapiRomajiLyricLines?.length) {
      effectiveLrcRomaSource = laapiRomajiLyricLines;
    }
    result.hasLrcRoma = effectiveLrcRomaSource.length > 0;

    // Effective YRC translation source
    let effectiveYrcTranSource: ParsedSourceLine[] = [];
    if (lrcData.ytlrc?.trim()) {
      effectiveYrcTranSource = parseLrcText(lrcData.ytlrc);
    } else if (lrcData.tlyric?.trim()) {
      effectiveYrcTranSource = parseLrcText(lrcData.tlyric);
    } else if (laapiTranslationLyricLines?.length) {
      effectiveYrcTranSource = laapiTranslationLyricLines;
    }
    result.hasYrcTran = effectiveYrcTranSource.length > 0;

    // Effective YRC romaji source
    let effectiveYrcRomaSource: ParsedSourceLine[] = [];
    if (lrcData.yromalrc?.trim()) {
      effectiveYrcRomaSource = parseLrcText(lrcData.yromalrc);
    } else if (lrcData.romalrc?.trim()) {
      effectiveYrcRomaSource = parseLrcText(lrcData.romalrc);
    } else if (laapiRomajiLyricLines?.length) {
      effectiveYrcRomaSource = laapiRomajiLyricLines;
    }
    result.hasYrcRoma = effectiveYrcRomaSource.length > 0;

    // Parse normal lyrics (LRC)
    if (lrcData.lrc) {
      try {
        const lrcParsedRaw = parseLrcText(lrcData.lrc);
        result.lrc = convertLrcLines(lrcParsedRaw);

        if (effectiveLrcTranSource.length > 0) {
          result.lrc = alignByIndex(result.lrc, convertLrcLines(effectiveLrcTranSource), "tran");
        }
        if (effectiveLrcRomaSource.length > 0) {
          result.lrc = alignByIndex(result.lrc, convertLrcLines(effectiveLrcRomaSource), "roma");
        }

        result.lrcAMData = buildAMLLData(
          lrcParsedRaw,
          effectiveLrcTranSource,
          effectiveLrcRomaSource,
        );
      } catch {
        result.lrc = [
          { time: 0, content: "LRC解析出错" },
          { time: 999, content: "Error parsing LRC" },
        ];
      }
    }

    // Parse YRC lyrics or handle pre-parsed TTML lyrics.
    if (lrcData.yrc || directTTMLLines.length > 0) {
      let yrcParsedRawLines: ParsedSourceLine[] = directTTMLLines;

      if (directTTMLLines.length > 0) {
        result.hasTTML = true;
        result.ttml = directTTMLLines;
      } else {
        const TTML_PREFIX = "___PARSED_LYRIC_LINES___";
        if (lrcData.yrc?.startsWith(TTML_PREFIX)) {
          // Compatibility with old in-memory results that carried parsed TTML via yrc.lyric.
          // New TTML data should arrive through data.ttml directly.
          try {
            const jsonPart = lrcData.yrc.substring(TTML_PREFIX.length);
            yrcParsedRawLines = normalizeTTMLLines(JSON.parse(jsonPart) as ParsedSourceLine[]);
            result.hasTTML = true;
            result.ttml = yrcParsedRawLines;
          } catch {
            yrcParsedRawLines = [];
          }
        } else if (lrcData.yrc) {
          yrcParsedRawLines = parseYrcText(lrcData.yrc);
        }
      }

      result.yrc = convertYrcLines(yrcParsedRawLines);

      if (effectiveYrcTranSource.length > 0) {
        try {
          result.yrc = alignByIndex(result.yrc, convertLrcLines(effectiveYrcTranSource), "tran");
        } catch {
          // Fallback: simple index-based assignment
          if (result.yrc.length > 0 && effectiveYrcTranSource.length > 0) {
            const parsedTran = convertLrcLines(effectiveYrcTranSource);
            const minLength = Math.min(result.yrc.length, parsedTran.length);
            for (let i = 0; i < minLength; i++) {
              result.yrc[i].tran = parsedTran[i].content;
            }
          }
        }
      }

      if (effectiveYrcRomaSource.length > 0) {
        try {
          result.yrc = alignByIndex(result.yrc, convertLrcLines(effectiveYrcRomaSource), "roma");
        } catch {
          // Fallback: simple index-based assignment
          if (result.yrc.length > 0 && effectiveYrcRomaSource.length > 0) {
            const parsedRoma = convertLrcLines(effectiveYrcRomaSource);
            const minLength = Math.min(result.yrc.length, parsedRoma.length);
            for (let i = 0; i < minLength; i++) {
              result.yrc[i].roma = parsedRoma[i].content;
            }
          }
        }
      }

      result.yrcAMData = result.hasTTML
        ? yrcParsedRawLines
        : buildAMLLData(yrcParsedRawLines, effectiveYrcTranSource, effectiveYrcRomaSource);
    }
  } catch {
    return createEmptyLyricResult();
  }

  // Final check: create basic lrc from yrc if needed
  if ((!result.lrc || result.lrc.length === 0) && result.yrc && result.yrc.length > 0) {
    result.lrc = result.yrc.map((yrcLine) => ({
      time: yrcLine.time,
      content: yrcLine.TextContent,
    }));
  }

  // Ensure placeholder lyrics exist
  if (!result.lrc || result.lrc.length === 0) {
    result.lrc = [
      { time: 0, content: "暂无歌词" },
      { time: 999, content: "No Lyrics Available" },
    ];
  }

  return result;
};

/**
 * 将解析后的歌词数据转换为标准LRC格式文本 (优化版)
 * @param parsedLyric 解析后的歌词结果对象
 * @returns 标准LRC格式文本
 */
export const formatAsLrc = (parsedLyric: ParsedLyricResult): string => {
  const lrc = parsedLyric?.lrc;
  if (!lrc || lrc.length === 0) {
    return "";
  }

  const parts: string[] = [];
  parts.length = lrc.length * 3; // Max possible lines (main + tran + roma)
  let count = 0;

  for (let i = 0; i < lrc.length; i++) {
    const line = lrc[i];
    const time = line.time;
    const minutes = (time / 60) | 0;
    const seconds = time % 60;
    const timeStr = `${minutes < 10 ? "0" : ""}${minutes}:${seconds < 10 ? "0" : ""}${seconds.toFixed(2).padStart(5, "0")}`;

    parts[count++] = `[${timeStr}]${line.content}\n`;

    if (parsedLyric.hasLrcTran && line.tran) {
      parts[count++] = `[${timeStr}]${line.tran}\n`;
    }

    if (parsedLyric.hasLrcRoma && line.roma) {
      parts[count++] = `[${timeStr}]${line.roma}\n`;
    }
  }

  parts.length = count;
  return parts.join("");
};

// Backward compatibility aliases
export const parseLyric = parseLyricData;
export const getDefaultLyricResult = createEmptyLyricResult;
export const formatToLrc = formatAsLrc;

export default parseLyricData;
