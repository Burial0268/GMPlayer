/**
 * LyricsProcessor Lyric Parser
 * 歌词解析主模块 (优化版)
 */

import { parseLrc as parseCoreLrc, parseYrc as parseCoreYrc } from "@applemusic-like-lyrics/lyric";
import { musicStore } from "@/store";
import { parseLrcLines, parseYrcLines, buildAMLLData } from './formatParser';
import { alignByIndex } from './alignment';
import type {
  LyricLine,
  RawLyricData,
  ParsedLrcLine,
  ParsedYrcLine,
  ParsedLyricResult,
  AMLLLine,
} from './types';

// Debug flag - disable in production
const DEBUG = false;

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
  formattedLrc: ""
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
      yromalrc: yromalrc?.lyric || null
    };

    // --- LAAPI data parsing ---
    let laapiTranslationLyricLines: LyricLine[] | null = null;
    const laapiTranslation = (data as any).translation;
    if (laapiTranslation && typeof laapiTranslation === 'string' && laapiTranslation.trim()) {
      try {
        const laapiTranslationText = laapiTranslation.replace(/\\n/g, '\n').replace(/\r/g, '');
        const parsedLines = parseCoreLrc(laapiTranslationText);
        if (parsedLines && parsedLines.length > 0) {
          laapiTranslationLyricLines = parsedLines;
        }
      } catch (e) {
        // Silently fail for LAAPI parsing
      }
    }

    let laapiRomajiLyricLines: LyricLine[] | null = null;
    const laapiRomaji = (data as any).romaji;
    if (laapiRomaji && typeof laapiRomaji === 'string' && laapiRomaji.trim()) {
      try {
        const laapiRomajiText = laapiRomaji.replace(/\\n/g, '\n').replace(/\r/g, '');
        laapiRomajiLyricLines = parseCoreLrc(laapiRomajiText);
      } catch (e) {
        // Silently fail for LAAPI parsing
      }
    }

    // --- Determine effective sources and update flags ---
    result.hasYrc = !!lrcData.yrc;

    // Effective LRC translation source
    let effectiveLrcTranSource: LyricLine[] = [];
    if (lrcData.tlyric?.trim()) {
      effectiveLrcTranSource = parseCoreLrc(lrcData.tlyric);
    } else if (laapiTranslationLyricLines?.length) {
      effectiveLrcTranSource = laapiTranslationLyricLines;
    }
    result.hasLrcTran = effectiveLrcTranSource.length > 0;

    // Effective LRC romaji source
    let effectiveLrcRomaSource: LyricLine[] = [];
    if (lrcData.romalrc?.trim()) {
      effectiveLrcRomaSource = parseCoreLrc(lrcData.romalrc);
    } else if (laapiRomajiLyricLines?.length) {
      effectiveLrcRomaSource = laapiRomajiLyricLines;
    }
    result.hasLrcRoma = effectiveLrcRomaSource.length > 0;

    // Effective YRC translation source
    let effectiveYrcTranSource: LyricLine[] = [];
    if (lrcData.ytlrc?.trim()) {
      effectiveYrcTranSource = parseCoreLrc(lrcData.ytlrc);
    } else if (lrcData.tlyric?.trim()) {
      effectiveYrcTranSource = parseCoreLrc(lrcData.tlyric);
    } else if (laapiTranslationLyricLines?.length) {
      effectiveYrcTranSource = laapiTranslationLyricLines;
    }
    result.hasYrcTran = effectiveYrcTranSource.length > 0;

    // Effective YRC romaji source
    let effectiveYrcRomaSource: LyricLine[] = [];
    if (lrcData.yromalrc?.trim()) {
      effectiveYrcRomaSource = parseCoreLrc(lrcData.yromalrc);
    } else if (lrcData.romalrc?.trim()) {
      effectiveYrcRomaSource = parseCoreLrc(lrcData.romalrc);
    } else if (laapiRomajiLyricLines?.length) {
      effectiveYrcRomaSource = laapiRomajiLyricLines;
    }
    result.hasYrcRoma = effectiveYrcRomaSource.length > 0;

    // Parse normal lyrics (LRC)
    if (lrcData.lrc) {
      try {
        const lrcParsedRaw = parseCoreLrc(lrcData.lrc);
        result.lrc = parseLrcLines(lrcParsedRaw);

        if (effectiveLrcTranSource.length > 0) {
          result.lrc = alignByIndex(result.lrc, parseLrcLines(effectiveLrcTranSource), "tran");
        }
        if (effectiveLrcRomaSource.length > 0) {
          result.lrc = alignByIndex(result.lrc, parseLrcLines(effectiveLrcRomaSource), "roma");
        }

        result.lrcAMData = buildAMLLData(lrcParsedRaw, effectiveLrcTranSource, effectiveLrcRomaSource);
      } catch (error) {
        result.lrc = [
          { time: 0, content: "LRC解析出错" },
          { time: 999, content: "Error parsing LRC" }
        ];
      }
    }

    // Parse YRC lyrics or handle pre-parsed TTML lyrics
    if (lrcData.yrc) {
      let yrcParsedRawLines: LyricLine[] = [];
      const TTML_PREFIX = '___PARSED_LYRIC_LINES___';

      if (lrcData.yrc.startsWith(TTML_PREFIX)) {
        try {
          const jsonPart = lrcData.yrc.substring(TTML_PREFIX.length);
          yrcParsedRawLines = JSON.parse(jsonPart) as LyricLine[];
          result.hasTTML = true;
          result.ttml = yrcParsedRawLines;
        } catch (error) {
          yrcParsedRawLines = [];
        }
      } else {
        yrcParsedRawLines = parseCoreYrc(lrcData.yrc);
      }

      result.yrc = parseYrcLines(yrcParsedRawLines);

      if (effectiveYrcTranSource.length > 0) {
        try {
          result.yrc = alignByIndex(result.yrc, parseLrcLines(effectiveYrcTranSource), "tran");
        } catch (error) {
          // Fallback: simple index-based assignment
          if (result.yrc.length > 0 && effectiveYrcTranSource.length > 0) {
            const parsedTran = parseLrcLines(effectiveYrcTranSource);
            const minLength = Math.min(result.yrc.length, parsedTran.length);
            for (let i = 0; i < minLength; i++) {
              result.yrc[i].tran = parsedTran[i].content;
            }
          }
        }
      }

      if (effectiveYrcRomaSource.length > 0) {
        try {
          result.yrc = alignByIndex(result.yrc, parseLrcLines(effectiveYrcRomaSource), "roma");
        } catch (error) {
          // Fallback: simple index-based assignment
          if (result.yrc.length > 0 && effectiveYrcRomaSource.length > 0) {
            const parsedRoma = parseLrcLines(effectiveYrcRomaSource);
            const minLength = Math.min(result.yrc.length, parsedRoma.length);
            for (let i = 0; i < minLength; i++) {
              result.yrc[i].roma = parsedRoma[i].content;
            }
          }
        }
      }

      result.yrcAMData = buildAMLLData(yrcParsedRawLines, effectiveYrcTranSource, effectiveYrcRomaSource);
    }

  } catch (error) {
    return createEmptyLyricResult();
  }

  // Final check: create basic lrc from yrc if needed
  if ((!result.lrc || result.lrc.length === 0) && result.yrc && result.yrc.length > 0) {
    result.lrc = result.yrc.map(yrcLine => ({
      time: yrcLine.time,
      content: yrcLine.TextContent
    }));
  }

  // Ensure placeholder lyrics exist
  if (!result.lrc || result.lrc.length === 0) {
    result.lrc = [
      { time: 0, content: "暂无歌词" },
      { time: 999, content: "No Lyrics Available" }
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
    return '';
  }

  const parts: string[] = [];
  parts.length = lrc.length * 3; // Max possible lines (main + tran + roma)
  let count = 0;

  for (let i = 0; i < lrc.length; i++) {
    const line = lrc[i];
    const time = line.time;
    const minutes = (time / 60) | 0;
    const seconds = time % 60;
    const timeStr = `${minutes < 10 ? '0' : ''}${minutes}:${seconds < 10 ? '0' : ''}${seconds.toFixed(2).padStart(5, '0')}`;

    parts[count++] = `[${timeStr}]${line.content}\n`;

    if (parsedLyric.hasLrcTran && line.tran) {
      parts[count++] = `[${timeStr}]${line.tran}\n`;
    }

    if (parsedLyric.hasLrcRoma && line.roma) {
      parts[count++] = `[${timeStr}]${line.roma}\n`;
    }
  }

  parts.length = count;
  return parts.join('');
};

// Backward compatibility aliases
export const parseLyric = parseLyricData;
export const getDefaultLyricResult = createEmptyLyricResult;
export const formatToLrc = formatAsLrc;

export default parseLyricData;
