/**
 * LyricsProcessor Lyric Parser
 * 歌词解析主模块
 */

import { parseLrc as parseCoreLrc, parseYrc as parseCoreYrc } from "@applemusic-like-lyrics/lyric";
import { msToS, msToTime } from "@/utils/timeTools";
import { musicStore } from "@/store";
import { parseLrcLines, parseYrcLines, buildAMLLData } from './formatParser';
import { alignByIndex } from './alignment';
import type {
  LyricLine,
  LyricWord,
  RawLyricData,
  ParsedLrcLine,
  ParsedYrcLine,
  ParsedLyricResult,
  AMLLLine,
} from './types';

// Backward compat alias
export type LyricData = RawLyricData;

// Creates the default empty state
export const createEmptyLyricResult = (): ParsedLyricResult => ({
  hasLrcTran: false,
  hasLrcRoma: false,
  hasYrc: false,
  hasYrcTran: false,
  hasYrcRoma: false,
  hasTTML: false,    // 默认没有TTML歌词
  lrc: [] as ParsedLrcLine[],
  yrc: [] as ParsedYrcLine[],
  ttml: [] as any[],          // TTML歌词数据默认为空数组
  lrcAMData: [] as AMLLLine[],
  yrcAMData: [] as AMLLLine[],
  formattedLrc: "" // 确保所有 ParsedLyricResult 的字段都被初始化
});

// 恢复默认 - Now uses the factory function
export const resetSongLyric = (): void => {
  const music = musicStore();
  const defaultLyric = createEmptyLyricResult();
  // @ts-ignore // 保留 ts-ignore 以避免因 store 类型问题导致的连锁错误，但理想情况下应修复 store 定义
  music.songLyric = {
    ...defaultLyric, // 直接扩展 defaultLyric 对象
    // 如果 music.songLyric 有其他 store 特有的字段，在这里单独添加
  } as any; // 使用 as any 作为临时措施，理想情况下 music.songLyric 应有正确类型
};

/**
 * Parse lyric data from API response
 * @param data API response data or null on fetch error
 * @returns Parsed lyric data (always returns a valid object)
 */
export const parseLyricData = (data: RawLyricData | null): ParsedLyricResult => {
  // Always return a valid default object on failure or invalid data
  if (!data || data.code !== 200) {
    return createEmptyLyricResult();
  }

  console.log("[parseLyricData] 开始解析歌词数据", data);

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
    if ((data as any).translation && typeof (data as any).translation === 'string' && (data as any).translation.trim() !== '') {
      console.log("[parseLyricData] 检测到LAAPI新格式翻译数据 (data.translation)，尝试解析。");
      const laapiTranslationText = (data as any).translation.replace(/\\n/g, '\n').replace(/\r/g, '');
      console.log("[parseLyricData] 将要用于解析的LAAPI translation文本 (前200字符):", laapiTranslationText.substring(0, 200));
      try {
        const parsedLines = parseCoreLrc(laapiTranslationText);
        if (parsedLines && parsedLines.length > 0) {
          laapiTranslationLyricLines = parsedLines;
          console.log(`[parseLyricData] LAAPI 'translation' 解析成功，共 ${laapiTranslationLyricLines.length} 行`);
        } else {
          laapiTranslationLyricLines = null;
          console.warn(`[parseLyricData] LAAPI 'translation' 解析后为空数组或返回非预期值 (如null)。解析得到的行数: ${parsedLines ? parsedLines.length : 'null/undefined'}`);
        }
      } catch (e) {
        console.error("[parseLyricData] 解析LAAPI 'translation' 字段或处理其结果时出错:", e);
        laapiTranslationLyricLines = null;
      }
    } else {
      console.log("[parseLyricData] LAAPI 'translation' 字段不存在或是空字符串。");
    }

    let laapiRomajiLyricLines: LyricLine[] | null = null;
    if ((data as any).romaji && typeof (data as any).romaji === 'string' && (data as any).romaji.trim() !== '') {
      console.log("[parseLyricData] 检测到LAAPI新格式音译数据 (data.romaji)，尝试解析。");
      try {
        // Process LAAPI romaji: replace literal \n with newline, and remove carriage returns.
        const laapiRomajiText = (data as any).romaji.replace(/\\n/g, '\n').replace(/\r/g, '');
        laapiRomajiLyricLines = parseCoreLrc(laapiRomajiText);
        console.log(`[parseLyricData] LAAPI 'romaji' 解析完成，共 ${laapiRomajiLyricLines.length} 行`);
      } catch (e) {
        console.error("[parseLyricData] 解析LAAPI 'romaji' 字段出错:", e);
        laapiRomajiLyricLines = null;
      }
    }

    // --- Determine effective sources and update flags ---
    result.hasYrc = !!lrcData.yrc;

    let effectiveLrcTranSource: LyricLine[] = [];
    if (lrcData.tlyric && lrcData.tlyric.trim() !== '') {
      effectiveLrcTranSource = parseCoreLrc(lrcData.tlyric.replace(/\n/g, '\n'));
    } else if (laapiTranslationLyricLines && laapiTranslationLyricLines.length > 0) {
      console.log("[parseLyricData] 使用 LAAPI 'translation' 作为 LRC 翻译源");
      effectiveLrcTranSource = laapiTranslationLyricLines;
    }
    result.hasLrcTran = effectiveLrcTranSource.length > 0;

    let effectiveLrcRomaSource: LyricLine[] = [];
    if (lrcData.romalrc && lrcData.romalrc.trim() !== '') {
      effectiveLrcRomaSource = parseCoreLrc(lrcData.romalrc.replace(/\n/g, '\n'));
    } else if (laapiRomajiLyricLines && laapiRomajiLyricLines.length > 0) {
      console.log("[parseLyricData] 使用 LAAPI 'romaji' 作为 LRC 音译源");
      effectiveLrcRomaSource = laapiRomajiLyricLines;
    }
    result.hasLrcRoma = effectiveLrcRomaSource.length > 0;

    let effectiveYrcTranSource: LyricLine[] = [];
    if (lrcData.ytlrc && lrcData.ytlrc.trim() !== '') {
      console.log("[parseLyricData] 使用 'ytlrc' 作为 YRC 翻译源");
      effectiveYrcTranSource = parseCoreLrc(lrcData.ytlrc.replace(/\n/g, '\n'));
    } else if (lrcData.tlyric && lrcData.tlyric.trim() !== '') {
      console.log("[parseLyricData] 使用 'tlyric' 作为 YRC 翻译源 (ytlrc缺失)");
      effectiveYrcTranSource = parseCoreLrc(lrcData.tlyric.replace(/\n/g, '\n'));
    } else if (laapiTranslationLyricLines && laapiTranslationLyricLines.length > 0) {
      console.log("[parseLyricData] 使用 LAAPI 'translation' 作为 YRC 翻译源 (ytlrc 和 tlyric 缺失)");
      effectiveYrcTranSource = laapiTranslationLyricLines;
    }
    result.hasYrcTran = effectiveYrcTranSource.length > 0;

    let effectiveYrcRomaSource: LyricLine[] = [];
    if (lrcData.yromalrc && lrcData.yromalrc.trim() !== '') {
      console.log("[parseLyricData] 使用 'yromalrc' 作为 YRC 音译源");
      effectiveYrcRomaSource = parseCoreLrc(lrcData.yromalrc.replace(/\n/g, '\n'));
    } else if (lrcData.romalrc && lrcData.romalrc.trim() !== '') {
      console.log("[parseLyricData] 使用 'romalrc' 作为 YRC 音译源 (yromalrc缺失)");
      effectiveYrcRomaSource = parseCoreLrc(lrcData.romalrc.replace(/\n/g, '\n'));
    } else if (laapiRomajiLyricLines && laapiRomajiLyricLines.length > 0) {
      console.log("[parseLyricData] 使用 LAAPI 'romaji' 作为 YRC 音译源 (yromalrc 和 romalrc 缺失)");
      effectiveYrcRomaSource = laapiRomajiLyricLines;
    }
    result.hasYrcRoma = effectiveYrcRomaSource.length > 0;

    console.log(`[parseLyricData] 最终标志设置:
      - hasLrcTran: ${result.hasLrcTran} (源: ${effectiveLrcTranSource.length > 0 ? '可用' : '无'})
      - hasLrcRoma: ${result.hasLrcRoma} (源: ${effectiveLrcRomaSource.length > 0 ? '可用' : '无'})
      - hasYrc: ${result.hasYrc}
      - hasYrcTran: ${result.hasYrcTran} (源: ${effectiveYrcTranSource.length > 0 ? '可用' : '无'})
      - hasYrcRoma: ${result.hasYrcRoma} (源: ${effectiveYrcRomaSource.length > 0 ? '可用' : '无'})`
    );

    // Parse normal lyrics (LRC)
    if (lrcData.lrc) {
      try {
        console.log("[parseLyricData] 开始解析LRC歌词");
        let lrcText = lrcData.lrc;
        lrcText = lrcText.replace(/\n/g, '\n'); // Ensure newlines are correct for parser
        const lrcParsedRaw = parseCoreLrc(lrcText);
        console.log(`[parseLyricData] 解析LRC完成，共 ${lrcParsedRaw.length} 行`);
        result.lrc = parseLrcLines(lrcParsedRaw);

        if (effectiveLrcTranSource.length > 0) {
          console.log("[parseLyricData] 对齐LRC翻译");
          result.lrc = alignByIndex(result.lrc, parseLrcLines(effectiveLrcTranSource), "tran");
        }
        if (effectiveLrcRomaSource.length > 0) {
          console.log("[parseLyricData] 对齐LRC音译");
          result.lrc = alignByIndex(result.lrc, parseLrcLines(effectiveLrcRomaSource), "roma");
        }

        result.lrcAMData = buildAMLLData(lrcParsedRaw, effectiveLrcTranSource, effectiveLrcRomaSource);
        console.log(`[parseLyricData] LRC AM格式数据生成完成，共 ${result.lrcAMData.length} 行`);
      } catch (error) {
        console.error("[parseLyricData] LRC解析或AM数据生成出错:", error);
        result.lrc = [
          { time: 0, content: "LRC解析出错" },
          { time: 999, content: "Error parsing LRC" }
        ];
      }
    }

    // Parse YRC lyrics or handle pre-parsed TTML lyrics
    if (lrcData.yrc) {
      let yrcParsedRawLines: LyricLine[] = []; // This is LyricLine[] structure

      if (lrcData.yrc.startsWith('___PARSED_LYRIC_LINES___')) {
        try {
          const jsonPart = lrcData.yrc.substring('___PARSED_LYRIC_LINES___'.length);
          yrcParsedRawLines = JSON.parse(jsonPart) as LyricLine[];
          console.log('[parseLyricData] 成功解析预处理的TTML数据 (作为YRC源)', yrcParsedRawLines.length);
          result.hasTTML = true;
          result.ttml = yrcParsedRawLines; // Store raw LyricLine[] for ttml
        } catch (error) {
          console.error('[parseLyricData] 解析预处理的TTML数据失败:', error);
          yrcParsedRawLines = [];
        }
      } else {
        console.log("[parseLyricData] 开始解析YRC/QRC歌词");
        yrcParsedRawLines = parseCoreYrc(lrcData.yrc);
        console.log(`[parseLyricData] YRC/QRC解析完成，共 ${yrcParsedRawLines.length} 行`);
      }

      result.yrc = parseYrcLines(yrcParsedRawLines); // Converts to ParsedYrcLine[] for display (time in seconds)

      if (effectiveYrcTranSource.length > 0) {
        console.log("[parseLyricData] 对齐YRC翻译");
        try {
          // parseLrcLines converts LyricLine[] (ms times) to ParsedLrcLine[] (ms times)
          // alignByIndex expects main lyric time (ParsedYrcLine seconds, ParsedLrcLine ms) and other lyric time (ParsedLrcLine ms)
          // The previous fix in alignByIndex handles ParsedYrcLine seconds vs ParsedLrcLine ms.
          result.yrc = alignByIndex(result.yrc, parseLrcLines(effectiveYrcTranSource), "tran");
        } catch (error) {
          console.warn('[parseLyricData] 对齐YRC翻译失败，尝试备用方法:', error);
          if (result.yrc && result.yrc.length > 0 && effectiveYrcTranSource.length > 0) {
            const parsedTran = parseLrcLines(effectiveYrcTranSource);
            const minLength = Math.min(result.yrc.length, parsedTran.length);
            for (let i = 0; i < minLength; i++) {
              result.yrc[i].tran = parsedTran[i].content;
            }
          }
        }
      }

      if (effectiveYrcRomaSource.length > 0) {
        console.log("[parseLyricData] 对齐YRC音译");
        try {
          result.yrc = alignByIndex(result.yrc, parseLrcLines(effectiveYrcRomaSource), "roma");
        } catch (error) {
          console.warn('[parseLyricData] 对齐YRC音译失败，尝试备用方法:', error);
           if (result.yrc && result.yrc.length > 0 && effectiveYrcRomaSource.length > 0) {
            const parsedRoma = parseLrcLines(effectiveYrcRomaSource);
            const minLength = Math.min(result.yrc.length, parsedRoma.length);
            for (let i = 0; i < minLength; i++) {
              result.yrc[i].roma = parsedRoma[i].content;
            }
          }
        }
      }

      console.log("[parseLyricData] 开始生成 YRC AM格式数据");
      result.yrcAMData = buildAMLLData(yrcParsedRawLines, effectiveYrcTranSource, effectiveYrcRomaSource);
      console.log(`[parseLyricData] YRC AM格式数据生成完成，共 ${result.yrcAMData.length} 行`);
      if (result.yrcAMData.length > 0 && effectiveYrcTranSource.length > 0) {
        const translatedLines = result.yrcAMData.filter(line => line.translatedLyric && line.translatedLyric.trim() !== "").length;
        console.log(`[parseLyricData] YRC AM格式数据中，包含翻译的行数: ${translatedLines}/${result.yrcAMData.length}`);
        if (translatedLines === 0 && result.yrcAMData.length > 0) {
            console.warn("[parseLyricData] YRC AM数据已生成，但没有行获得翻译，请检查 buildAMLLData 逻辑和时间匹配。");
        }
      }
    }

  } catch (error) {
      console.error("[parseLyricData] 歌词解析过程中发生严重错误:", error);
      return createEmptyLyricResult(); // Return default object on major parsing error
  }

  // 最终检查：如果没有lrc数据但有yrc数据，为了安全起见再次确认lrc数据存在
  if ((!result.lrc || result.lrc.length === 0) && result.yrc && result.yrc.length > 0) {
    console.log('[parseLyricData] 最终检查：创建基本的lrc数据');

    // 创建基本的lrc格式歌词
    result.lrc = result.yrc.map(yrcLine => {
      return {
        time: yrcLine.time,
        content: yrcLine.TextContent
      };
    });
  }

  // 确保即使在数据生成可能失败的情况下也有基本的占位歌词
  if (!result.lrc || result.lrc.length === 0) {
    console.log('[parseLyricData] 没有可用的歌词数据，创建占位歌词');
    result.lrc = [
      { time: 0, content: "暂无歌词" },
      { time: 999, content: "No Lyrics Available" }
    ];
  }

  return result;
};

/**
 * 将解析后的歌词数据转换为标准LRC格式文本
 * @param parsedLyric 解析后的歌词结果对象
 * @returns 标准LRC格式文本
 */
export const formatAsLrc = (parsedLyric: ParsedLyricResult): string => {
  if (!parsedLyric || !parsedLyric.lrc || parsedLyric.lrc.length === 0) {
    return '';
  }

  // 标题、作者等元数据（可选，如果有数据的话）
  let lrcText = '';

  // 转换每一行歌词
  parsedLyric.lrc.forEach(line => {
    // 将秒转换为分:秒格式 (00:00.00)
    const minutes = Math.floor(line.time / 60);
    const seconds = (line.time % 60).toFixed(2);
    const timeStr = `${minutes.toString().padStart(2, '0')}:${seconds.padStart(5, '0')}`;

    // 添加主歌词行
    lrcText += `[${timeStr}]${line.content}\n`;

    // 如果有翻译，添加翻译行
    if (parsedLyric.hasLrcTran && line.tran) {
      lrcText += `[${timeStr}]${line.tran}\n`;
    }

    // 如果有罗马音，添加罗马音行
    if (parsedLyric.hasLrcRoma && line.roma) {
      lrcText += `[${timeStr}]${line.roma}\n`;
    }
  });

  return lrcText;
};

// Backward compatibility aliases
export const parseLyric = parseLyricData;
export const getDefaultLyricResult = createEmptyLyricResult;
export const formatToLrc = formatAsLrc;

export default parseLyricData;
