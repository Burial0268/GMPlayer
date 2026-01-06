import { parseLrc as parseCoreLrc, parseYrc as parseCoreYrc } from "@applemusic-like-lyrics/lyric";
import { msToS, msToTime } from "./timeTools";
import { musicStore } from "../store";

interface LyricWord {
  word: string;
  startTime: number;
  endTime: number;
}

interface LyricLine {
  words: LyricWord[];
  isBG?: boolean;
  isDuet?: boolean;
}

interface ParsedLyricLine {
  time: number;
  content: string;
  tran?: string;
  roma?: string;
}

interface YrcLine {
  time: number;
  endTime: number;
  content: {
    time: number;
    endTime: number;
    duration: number;
    content: string;
    endsWithSpace: boolean;
  }[];
  TextContent: string;
  tran?: string;
  roma?: string;
}

interface LyricData {
  lrc?: { lyric: string } | null;
  tlyric?: { lyric: string } | null;
  romalrc?: { lyric: string } | null;
  yrc?: { lyric: string } | null;
  ytlrc?: { lyric: string } | null;
  yromalrc?: { lyric: string } | null;
  code?: number;
}

interface ParsedLyricResult {
  hasLrcTran: boolean;
  hasLrcRoma: boolean;
  hasYrc: boolean;
  hasYrcTran: boolean;
  hasYrcRoma: boolean;
  hasTTML: boolean;  // 是否拥有TTML格式歌词
  lrc: ParsedLyricLine[];
  yrc: YrcLine[];
  ttml: any[];       // parseTTML解析后的数据
  lrcAMData: any[];
  yrcAMData: any[];
  formattedLrc?: string; // 可选字段，格式化后的LRC文本
}

// Creates the default empty state
const getDefaultLyricResult = (): ParsedLyricResult => ({
  hasLrcTran: false,
  hasLrcRoma: false,
  hasYrc: false,
  hasYrcTran: false,
  hasYrcRoma: false,
  hasTTML: false,    // 默认没有TTML歌词
  lrc: [] as ParsedLyricLine[],
  yrc: [] as YrcLine[],
  ttml: [] as any[],          // TTML歌词数据默认为空数组
  lrcAMData: [] as any[],
  yrcAMData: [] as any[],
  formattedLrc: "" // 确保所有 ParsedLyricResult 的字段都被初始化
});

// 恢复默认 - Now uses the factory function
const resetSongLyric = (): void => {
  const music = musicStore();
  const defaultLyric = getDefaultLyricResult();
  // @ts-ignore // 保留 ts-ignore 以避免因 store 类型问题导致的连锁错误，但理想情况下应修复 store 定义
  music.songLyric = {
    ...defaultLyric, // 直接扩展 defaultLyric 对象
    // 如果 music.songLyric 有其他 store 特有的字段，在这里单独添加
  } as any; // 使用 as any 作为临时措施，理想情况下 music.songLyric 应有正确类型
};

/**
 * Parse lyric data from API response
 * @param {LyricData | null} data API response data or null on fetch error
 * @returns {ParsedLyricResult} Parsed lyric data (always returns a valid object)
 */
const parseLyric = (data: LyricData | null): ParsedLyricResult => {
  // Always return a valid default object on failure or invalid data
  if (!data || data.code !== 200) {
    return getDefaultLyricResult();
  }

  console.log("[parseLyric] 开始解析歌词数据", data);
  
  const result: ParsedLyricResult = getDefaultLyricResult();

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
      console.log("[parseLyric] 检测到LAAPI新格式翻译数据 (data.translation)，尝试解析。");
      const laapiTranslationText = (data as any).translation.replace(/\\n/g, '\n').replace(/\r/g, '');
      console.log("[parseLyric] 将要用于解析的LAAPI translation文本 (前200字符):", laapiTranslationText.substring(0, 200));
      try {
        const parsedLines = parseCoreLrc(laapiTranslationText);
        if (parsedLines && parsedLines.length > 0) {
          laapiTranslationLyricLines = parsedLines;
          console.log(`[parseLyric] LAAPI 'translation' 解析成功，共 ${laapiTranslationLyricLines.length} 行`);
        } else {
          laapiTranslationLyricLines = null; 
          console.warn(`[parseLyric] LAAPI 'translation' 解析后为空数组或返回非预期值 (如null)。解析得到的行数: ${parsedLines ? parsedLines.length : 'null/undefined'}`);
        }
      } catch (e) {
        console.error("[parseLyric] 解析LAAPI 'translation' 字段或处理其结果时出错:", e);
        laapiTranslationLyricLines = null;
      }
    } else {
      console.log("[parseLyric] LAAPI 'translation' 字段不存在或是空字符串。");
    }

    let laapiRomajiLyricLines: LyricLine[] | null = null;
    if ((data as any).romaji && typeof (data as any).romaji === 'string' && (data as any).romaji.trim() !== '') {
      console.log("[parseLyric] 检测到LAAPI新格式音译数据 (data.romaji)，尝试解析。");
      try {
        // Process LAAPI romaji: replace literal \n with newline, and remove carriage returns.
        const laapiRomajiText = (data as any).romaji.replace(/\\n/g, '\n').replace(/\r/g, '');
        laapiRomajiLyricLines = parseCoreLrc(laapiRomajiText);
        console.log(`[parseLyric] LAAPI 'romaji' 解析完成，共 ${laapiRomajiLyricLines.length} 行`);
      } catch (e) {
        console.error("[parseLyric] 解析LAAPI 'romaji' 字段出错:", e);
        laapiRomajiLyricLines = null;
      }
    }

    // --- Determine effective sources and update flags ---
    result.hasYrc = !!lrcData.yrc;

    let effectiveLrcTranSource: LyricLine[] = [];
    if (lrcData.tlyric && lrcData.tlyric.trim() !== '') {
      effectiveLrcTranSource = parseCoreLrc(lrcData.tlyric.replace(/\n/g, '\n'));
    } else if (laapiTranslationLyricLines && laapiTranslationLyricLines.length > 0) {
      console.log("[parseLyric] 使用 LAAPI 'translation' 作为 LRC 翻译源");
      effectiveLrcTranSource = laapiTranslationLyricLines;
    }
    result.hasLrcTran = effectiveLrcTranSource.length > 0;

    let effectiveLrcRomaSource: LyricLine[] = [];
    if (lrcData.romalrc && lrcData.romalrc.trim() !== '') {
      effectiveLrcRomaSource = parseCoreLrc(lrcData.romalrc.replace(/\n/g, '\n'));
    } else if (laapiRomajiLyricLines && laapiRomajiLyricLines.length > 0) {
      console.log("[parseLyric] 使用 LAAPI 'romaji' 作为 LRC 音译源");
      effectiveLrcRomaSource = laapiRomajiLyricLines;
    }
    result.hasLrcRoma = effectiveLrcRomaSource.length > 0;

    let effectiveYrcTranSource: LyricLine[] = [];
    if (lrcData.ytlrc && lrcData.ytlrc.trim() !== '') {
      console.log("[parseLyric] 使用 'ytlrc' 作为 YRC 翻译源");
      effectiveYrcTranSource = parseCoreLrc(lrcData.ytlrc.replace(/\n/g, '\n'));
    } else if (lrcData.tlyric && lrcData.tlyric.trim() !== '') {
      console.log("[parseLyric] 使用 'tlyric' 作为 YRC 翻译源 (ytlrc缺失)");
      effectiveYrcTranSource = parseCoreLrc(lrcData.tlyric.replace(/\n/g, '\n'));
    } else if (laapiTranslationLyricLines && laapiTranslationLyricLines.length > 0) {
      console.log("[parseLyric] 使用 LAAPI 'translation' 作为 YRC 翻译源 (ytlrc 和 tlyric 缺失)");
      effectiveYrcTranSource = laapiTranslationLyricLines;
    }
    result.hasYrcTran = effectiveYrcTranSource.length > 0;

    let effectiveYrcRomaSource: LyricLine[] = [];
    if (lrcData.yromalrc && lrcData.yromalrc.trim() !== '') {
      console.log("[parseLyric] 使用 'yromalrc' 作为 YRC 音译源");
      effectiveYrcRomaSource = parseCoreLrc(lrcData.yromalrc.replace(/\n/g, '\n'));
    } else if (lrcData.romalrc && lrcData.romalrc.trim() !== '') {
      console.log("[parseLyric] 使用 'romalrc' 作为 YRC 音译源 (yromalrc缺失)");
      effectiveYrcRomaSource = parseCoreLrc(lrcData.romalrc.replace(/\n/g, '\n'));
    } else if (laapiRomajiLyricLines && laapiRomajiLyricLines.length > 0) {
      console.log("[parseLyric] 使用 LAAPI 'romaji' 作为 YRC 音译源 (yromalrc 和 romalrc 缺失)");
      effectiveYrcRomaSource = laapiRomajiLyricLines;
    }
    result.hasYrcRoma = effectiveYrcRomaSource.length > 0;
    
    console.log(`[parseLyric] 最终标志设置:
      - hasLrcTran: ${result.hasLrcTran} (源: ${effectiveLrcTranSource.length > 0 ? '可用' : '无'})
      - hasLrcRoma: ${result.hasLrcRoma} (源: ${effectiveLrcRomaSource.length > 0 ? '可用' : '无'})
      - hasYrc: ${result.hasYrc}
      - hasYrcTran: ${result.hasYrcTran} (源: ${effectiveYrcTranSource.length > 0 ? '可用' : '无'})
      - hasYrcRoma: ${result.hasYrcRoma} (源: ${effectiveYrcRomaSource.length > 0 ? '可用' : '无'})`
    );

    // Parse normal lyrics (LRC)
    if (lrcData.lrc) {
      try {
        console.log("[parseLyric] 开始解析LRC歌词");
        let lrcText = lrcData.lrc;
        lrcText = lrcText.replace(/\n/g, '\n'); // Ensure newlines are correct for parser
        const lrcParsedRaw = parseCoreLrc(lrcText);
        console.log(`[parseLyric] 解析LRC完成，共 ${lrcParsedRaw.length} 行`);
        result.lrc = parseLrcData(lrcParsedRaw);

        if (effectiveLrcTranSource.length > 0) {
          console.log("[parseLyric] 对齐LRC翻译");
          result.lrc = alignLyrics(result.lrc, parseLrcData(effectiveLrcTranSource), "tran");
        }
        if (effectiveLrcRomaSource.length > 0) {
          console.log("[parseLyric] 对齐LRC音译");
          result.lrc = alignLyrics(result.lrc, parseLrcData(effectiveLrcRomaSource), "roma");
        }

        result.lrcAMData = parseAMData(lrcParsedRaw, effectiveLrcTranSource, effectiveLrcRomaSource);
        console.log(`[parseLyric] LRC AM格式数据生成完成，共 ${result.lrcAMData.length} 行`);
      } catch (error) {
        console.error("[parseLyric] LRC解析或AM数据生成出错:", error);
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
          console.log('[parseLyric] 成功解析预处理的TTML数据 (作为YRC源)', yrcParsedRawLines.length);
          result.hasTTML = true;
          result.ttml = yrcParsedRawLines; // Store raw LyricLine[] for ttml
        } catch (error) {
          console.error('[parseLyric] 解析预处理的TTML数据失败:', error);
          yrcParsedRawLines = [];
        }
      } else {
        console.log("[parseLyric] 开始解析YRC/QRC歌词");
        yrcParsedRawLines = parseCoreYrc(lrcData.yrc);
        console.log(`[parseLyric] YRC/QRC解析完成，共 ${yrcParsedRawLines.length} 行`);
      }
      
      result.yrc = parseYrcData(yrcParsedRawLines); // Converts to YrcLine[] for display (time in seconds)

      if (effectiveYrcTranSource.length > 0) {
        console.log("[parseLyric] 对齐YRC翻译");
        try {
          // parseLrcData converts LyricLine[] (ms times) to ParsedLyricLine[] (ms times)
          // alignLyrics expects main lyric time (YrcLine seconds, ParsedLyricLine ms) and other lyric time (ParsedLyricLine ms)
          // The previous fix in alignLyrics handles YrcLine seconds vs ParsedLyricLine ms.
          result.yrc = alignLyrics(result.yrc, parseLrcData(effectiveYrcTranSource), "tran");
        } catch (error) {
          console.warn('[parseLyric] 对齐YRC翻译失败，尝试备用方法:', error);
          if (result.yrc && result.yrc.length > 0 && effectiveYrcTranSource.length > 0) {
            const parsedTran = parseLrcData(effectiveYrcTranSource);
            const minLength = Math.min(result.yrc.length, parsedTran.length);
            for (let i = 0; i < minLength; i++) {
              result.yrc[i].tran = parsedTran[i].content;
            }
          }
        }
      } 
      
      if (effectiveYrcRomaSource.length > 0) {
        console.log("[parseLyric] 对齐YRC音译");
        try {
          result.yrc = alignLyrics(result.yrc, parseLrcData(effectiveYrcRomaSource), "roma");
        } catch (error) {
          console.warn('[parseLyric] 对齐YRC音译失败，尝试备用方法:', error);
           if (result.yrc && result.yrc.length > 0 && effectiveYrcRomaSource.length > 0) {
            const parsedRoma = parseLrcData(effectiveYrcRomaSource);
            const minLength = Math.min(result.yrc.length, parsedRoma.length);
            for (let i = 0; i < minLength; i++) {
              result.yrc[i].roma = parsedRoma[i].content;
            }
          }
        }
      }
      
      console.log("[parseLyric] 开始生成 YRC AM格式数据");
      result.yrcAMData = parseAMData(yrcParsedRawLines, effectiveYrcTranSource, effectiveYrcRomaSource);
      console.log(`[parseLyric] YRC AM格式数据生成完成，共 ${result.yrcAMData.length} 行`);
      if (result.yrcAMData.length > 0 && effectiveYrcTranSource.length > 0) {
        const translatedLines = result.yrcAMData.filter(line => line.translatedLyric && line.translatedLyric.trim() !== "").length;
        console.log(`[parseLyric] YRC AM格式数据中，包含翻译的行数: ${translatedLines}/${result.yrcAMData.length}`);
        if (translatedLines === 0 && result.yrcAMData.length > 0) {
            console.warn("[parseLyric] YRC AM数据已生成，但没有行获得翻译，请检查 parseAMData 逻辑和时间匹配。");
        }
      }
    }

  } catch (error) {
      console.error("[parseLyric] 歌词解析过程中发生严重错误:", error);
      return getDefaultLyricResult(); // Return default object on major parsing error
  }
  
  // 最终检查：如果没有lrc数据但有yrc数据，为了安全起见再次确认lrc数据存在
  if ((!result.lrc || result.lrc.length === 0) && result.yrc && result.yrc.length > 0) {
    console.log('[parseLyric] 最终检查：创建基本的lrc数据');
    
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
    console.log('[parseLyric] 没有可用的歌词数据，创建占位歌词');
    result.lrc = [
      { time: 0, content: "暂无歌词" },
      { time: 999, content: "No Lyrics Available" }
    ];
  }

  return result;
};

/**
 * Process parsed Lyric data into easier to use format
 */
const parseLrcData = (lrcData: LyricLine[]): ParsedLyricLine[] => {
  if (!lrcData || !lrcData.length) {
    console.warn('[parseLrcData] 输入的歌词数据为空');
    return [];
  }

  console.log(`[parseLrcData] 开始处理${lrcData.length}行歌词数据`);
  
  const result = lrcData.map((line, index) => {
    // 确保line和line.words存在
    if (!line || !line.words || !line.words.length) {
      console.warn(`[parseLrcData] 第${index}行数据不完整`);
      return null;
    }
    
    // 获取行开始时间
    let time = 0;
    if (line.words && line.words.length > 0) {
      time = line.words[0].startTime;
    }

    // 将歌词单词连接为完整内容
    let content = '';
    if (line.words && line.words.length > 0) {
      content = line.words.map((word) => word.word || '').join('');
    }
    
    // 只有有内容的行才返回
    if (!content || !content.trim()) {
      return null;
    }

    if (index < 5 || index % 10 === 0) {
      console.log(`[parseLrcData] 处理第${index}行: 时间=${time}ms, 内容="${content.substring(0, 15)}..."`);
    }
    
    return {
      time,
      content,
    };
  }).filter((line): line is ParsedLyricLine => line !== null);

  console.log(`[parseLrcData] 处理完成，输出${result.length}行有效歌词`);
  return result;
};

/**
 * Parse YRC (word-by-word) lyrics
 * @param {LyricLine[]} yrcData Array of LyricLine objects
 * @returns {YrcLine[]} Parsed YRC data
 */
const parseYrcData = (yrcData: LyricLine[]): YrcLine[] => {
  if (!yrcData) return [];

  return yrcData
    .map(line => {
      const words = line.words;
      const time = msToS(words[0].startTime);
      const endTime = msToS(words[words.length - 1].endTime);

      const content = words.map(word => ({
        time: msToS(word.startTime),
        endTime: msToS(word.endTime),
        duration: msToS(word.endTime - word.startTime),
        content: word.word.endsWith(" ") ? word.word : word.word.trim(),
        endsWithSpace: word.word.endsWith(" ")
      }));

      const contentStr = content
        .map(word => word.content)
        .join("");

      if (!contentStr) return null;

      return {
        time,
        endTime,
        content,
        TextContent: contentStr
      };
    })
    .filter((line): line is YrcLine => line !== null);
};

/**
 * Align lyrics with translations
 * @param {(ParsedLyricLine[] | YrcLine[])} lyrics Main lyrics array
 * @param {ParsedLyricLine[]} otherLyrics Translation lyrics array
 * @param {string} key Property key for translation ('tran' or 'roma')
 * @returns {(ParsedLyricLine[] | YrcLine[])} Aligned lyrics array
 */
const alignLyrics = <T extends ParsedLyricLine | YrcLine>(
  lyrics: T[],
  otherLyrics: ParsedLyricLine[],
  key: 'tran' | 'roma'
): T[] => {
  if (!lyrics.length || !otherLyrics.length) {
    return lyrics;
  }

  console.log(`[alignLyrics] 开始对齐${key}歌词，主歌词${lyrics.length}行，辅助歌词${otherLyrics.length}行`);

  if (lyrics.length === otherLyrics.length) {
    console.log(`[alignLyrics] 使用策略1：行数相同，直接对应`);
    for (let i = 0; i < lyrics.length; i++) {
      (lyrics[i] as any)[key] = otherLyrics[i].content;
    }
    return lyrics;
  }

  console.log(`[alignLyrics] 使用策略2：时间最接近匹配 (优化版)`);
  const sortedOtherLyrics = [...otherLyrics].sort((a, b) => a.time - b.time); // otherLyrics.time is in ms

  lyrics.forEach(mainLine => {
    if (sortedOtherLyrics.length === 0) return;

    // Determine mainLine's time in milliseconds for consistent comparison
    // 'TextContent' in mainLine helps distinguish YrcLine from ParsedLyricLine.
    // ParsedLyricLine has: time: number (ms); content: string;
    // YrcLine has: time: number (s); endTime: number (s); content: object[]; TextContent: string;
    const mainLineTimeInMs = ('TextContent' in mainLine && typeof mainLine.time === 'number')
      ? mainLine.time * 1000 // YrcLine time is in seconds, convert to ms
      : mainLine.time;        // ParsedLyricLine time is already in ms, or fallback if type check fails

    let low = 0;
    let high = sortedOtherLyrics.length - 1;
    let closestMatch = sortedOtherLyrics[0];
    // Compare otherLyrics time (ms) with mainLineTimeInMs (ms)
    let minTimeDiff = Math.abs(mainLineTimeInMs - closestMatch.time);

    while (low <= high) {
      const mid = Math.floor((low + high) / 2);
      const currentOtherTimeInMs = sortedOtherLyrics[mid].time;
      const currentDiff = mainLineTimeInMs - currentOtherTimeInMs; // ms - ms
      
      if (Math.abs(currentDiff) < minTimeDiff) {
        minTimeDiff = Math.abs(currentDiff);
        closestMatch = sortedOtherLyrics[mid];
      } else if (Math.abs(currentDiff) === minTimeDiff) {
        // 时间差相同时，优先选择索引更小的（更早的歌词行）
        if (currentOtherTimeInMs < closestMatch.time) { // if times are same, prefer earlier otherLyric
            closestMatch = sortedOtherLyrics[mid];
        }
      }

      if (currentDiff < 0) {
        high = mid - 1;
      } else if (currentDiff > 0) {
        low = mid + 1;
      } else {
        // 完全匹配
        closestMatch = sortedOtherLyrics[mid];
        minTimeDiff = 0;
        break;
      }
    }
    
    const checkNeighbor = (index: number) => {
      if (index >= 0 && index < sortedOtherLyrics.length) {
        const otherTimeInMs = sortedOtherLyrics[index].time;
        const diff = Math.abs(mainLineTimeInMs - otherTimeInMs); // ms - ms
        if (diff < minTimeDiff) {
          minTimeDiff = diff;
          closestMatch = sortedOtherLyrics[index];
        } else if (diff === minTimeDiff && otherTimeInMs < closestMatch.time) {
          closestMatch = sortedOtherLyrics[index];
        }
      }
    };

    // `low` 最终会指向大于target的第一个元素，或者等于target的元素，或者length
    // `high` 最终会指向小于target的最后一个元素，或者等于target的元素，或者-1
    // 需要检查 `low` 和 `high` 指向的元素（及其邻近）
    checkNeighbor(low);
    checkNeighbor(low - 1);
    checkNeighbor(high);
    checkNeighbor(high + 1);


    if (closestMatch) { // Ensure closestMatch is defined before using it
      (mainLine as any)[key] = closestMatch.content;
      // minTimeDiff is now in milliseconds. So 1.0 second threshold is 1000 ms.
      if (minTimeDiff > 1000) { 
        console.log(`[alignLyrics] 警告：歌词对齐时间差较大 (${(minTimeDiff / 1000).toFixed(2)}秒)，主歌词"${
          'TextContent' in mainLine ? mainLine.TextContent.substring(0, 10) : (mainLine as ParsedLyricLine).content.substring(0,10)
        }..."，${key}歌词"${closestMatch.content.substring(0, 10)}..."`);
      }
    }
  });

  console.log(`[alignLyrics] 歌词对齐完成`);
  return lyrics;
};

/**
 * Parse lyrics for Apple Music like format
 * @param {LyricLine[]} lrcData Main lyrics array
 * @param {LyricLine[]} tranData Translation lyrics array
 * @param {LyricLine[]} romaData Romanization lyrics array
 * @returns {any[]} Formatted lyrics array
 */
const parseAMData = (
  lrcData: LyricLine[],
  tranData: LyricLine[] = [],
  romaData: LyricLine[] = []
): any[] => {
  console.log(`[parseAMData] 开始处理AM格式歌词，主歌词${lrcData.length}行，翻译${tranData.length}行，音译${romaData.length}行`);

  const TIME_TOLERANCE = 15000; // 15秒容忍度

  const getBestMatchFromArray = (mainTimeMs: number, timedArray: {timeMs: number, content: string}[], type: string): string => {
    if (!timedArray || !timedArray.length) {
        // console.log(`[getBestMatchFromArray] (${type}) 匹配数组为空，返回空字符串 for time ${mainTimeMs}ms`);
        return "";
    }

    let bestMatchContent = "";
    let minDiff = TIME_TOLERANCE + 1;
    let closestTime = -1;

    // console.log(`[getBestMatchFromArray] (${type}) 尝试为 ${mainTimeMs}ms 匹配，数组长度 ${timedArray.length}, 容忍度 ${TIME_TOLERANCE}ms`);

    for (const item of timedArray) {
        const diff = Math.abs(item.timeMs - mainTimeMs);
        if (diff <= TIME_TOLERANCE) { // Only consider if within tolerance
            if (diff < minDiff) {
                minDiff = diff;
                bestMatchContent = item.content;
                closestTime = item.timeMs;
            } else if (diff === minDiff) {
                // Prefer earlier line if time diff is identical
                if (item.timeMs < closestTime) {
                    bestMatchContent = item.content;
                    closestTime = item.timeMs;
                }
            }
        }
    }
    
    if (bestMatchContent) {
    //   console.log(`[getBestMatchFromArray] (${type}) 匹配成功 for ${mainTimeMs}ms: (原文时间 ${closestTime}ms, 内容 "${bestMatchContent.substring(0,15)}...") diff: ${minDiff}ms`);
    } else {
    //   console.log(`[getBestMatchFromArray] (${type}) 未找到匹配 for ${mainTimeMs}ms 在容忍度 ${TIME_TOLERANCE}ms 内`);
    }
    return bestMatchContent;
  };
  
  // Prepare sorted arrays from tranData and romaData IF they are not already aligned by line count
  // parseAMData expects LyricLine[], which have words[0].startTime etc.
  // The getBestMatchFromArray helper expects {timeMs, content}[]
  
  let sortedTranArrayForAM: {timeMs: number, content: string}[] = [];
  if (tranData.length > 0) {
    // Assuming tranData (LyricLine[]) is not necessarily sorted or matched one-to-one with lrcData
    sortedTranArrayForAM = tranData.map(line => {
        const firstWord = line.words && line.words.length > 0 ? line.words[0] : null;
        return {
            timeMs: firstWord ? firstWord.startTime : 0, // startTime is in ms
            content: firstWord ? line.words.map(w => w.word).join('') : "" // Join all words in the line
        };
    }).filter(item => item.content.trim() !== '').sort((a,b) => a.timeMs - b.timeMs);
    console.log(`[parseAMData] 已构建排序的翻译数组 (sortedTranArrayForAM), ${sortedTranArrayForAM.length} 有效行`);
    if (sortedTranArrayForAM.length > 0) {
        // console.log("[parseAMData] sortedTranArrayForAM 示例:", JSON.stringify(sortedTranArrayForAM.slice(0,2)));
    }
  }

  let sortedRomaArrayForAM: {timeMs: number, content: string}[] = [];
  if (romaData.length > 0) {
    sortedRomaArrayForAM = romaData.map(line => {
        const firstWord = line.words && line.words.length > 0 ? line.words[0] : null;
        return {
            timeMs: firstWord ? firstWord.startTime : 0,
            content: firstWord ? line.words.map(w => w.word).join('') : ""
        };
    }).filter(item => item.content.trim() !== '').sort((a,b) => a.timeMs - b.timeMs);
    console.log(`[parseAMData] 已构建排序的音译数组 (sortedRomaArrayForAM), ${sortedRomaArrayForAM.length} 有效行`);
  }


  const resultAM = lrcData.map((line, index, lines) => {
    // Ensure line.words exists and is not empty before accessing
    const mainLineFirstWord = line.words && line.words.length > 0 ? line.words[0] : null;
    const mainLineLastWord = line.words && line.words.length > 0 ? line.words[line.words.length - 1] : null;

    const startTimeMs = mainLineFirstWord ? mainLineFirstWord.startTime : 0;
    
    // Calculate endTimeMs carefully
    let endTimeMs;
    const nextLineFirstWord = lines[index + 1]?.words && lines[index + 1].words.length > 0 ? lines[index + 1].words[0] : null;
    if (nextLineFirstWord) {
      endTimeMs = nextLineFirstWord.startTime;
    } else if (mainLineLastWord) {
      endTimeMs = mainLineLastWord.endTime;
    } else {
      endTimeMs = startTimeMs + 5000; // Fallback: 5 seconds duration
    }

    // Ensure endTimeMs is greater than startTimeMs, add a minimum duration if not.
    if (endTimeMs <= startTimeMs) {
        endTimeMs = startTimeMs + 100; // Default to 100ms if end time is not sensible
    }

    if (index < 3 || index % 20 === 0) {
        // console.log(`[parseAMData] 处理主歌词行 #${index}: startTimeMs=${startTimeMs}`);
    }

    let translatedLyric = "";
    // Strategy: if line counts match and it's not a huge list, could do direct index. But general case is matching.
    if (sortedTranArrayForAM.length > 0) {
      translatedLyric = getBestMatchFromArray(startTimeMs, sortedTranArrayForAM, "翻译");
    }


    let romanLyric = "";
    if (sortedRomaArrayForAM.length > 0) {
      romanLyric = getBestMatchFromArray(startTimeMs, sortedRomaArrayForAM, "音译");
    }
    
    // Fallback direct mapping (less accurate, use with caution or remove if getBestMatch is robust)
    // This was complex and potentially buggy. Rely on getBestMatchFromArray with sorted arrays.

    translatedLyric = translatedLyric || ""; // Ensure it's not null/undefined
    romanLyric = romanLyric || "";

    // 确保 words 数组包含所有必需的字段（romanWord 和 obscene 将在 convertToAMLL 中添加）
    const words = (line.words || []).map(w => ({
      word: w.word,
      startTime: w.startTime,
      endTime: w.endTime,
      // 保留原始数据中可能存在的额外字段
      ...(w as any),
    }));
    
    return {
      words, // words from main lyric line，将在 convertToAMLL 中转换为 AMLL 格式
      startTime: startTimeMs,
      endTime: endTimeMs,
      translatedLyric,
      romanLyric,
      isBG: line.isBG ?? false,
      isDuet: line.isDuet ?? false,
    };
  });
  
  console.log(`[parseAMData] AM格式处理完成，共生成${resultAM.length}行`);
  if (resultAM.length > 0 && tranData.length > 0) {
      const amTranslatedCount = resultAM.filter(r => r.translatedLyric && r.translatedLyric !== "").length;
      console.log(`[parseAMData] 在生成的AM数据中，${amTranslatedCount}/${resultAM.length} 行包含有效翻译。`);
  }
  return resultAM;
};

/**
 * 将解析后的歌词数据转换为标准LRC格式文本
 * @param {ParsedLyricResult} parsedLyric 解析后的歌词结果对象
 * @returns {string} 标准LRC格式文本
 */
const formatToLrc = (parsedLyric: ParsedLyricResult): string => {
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

export {
  parseLyric,
  parseLrcData,
  parseYrcData,
  alignLyrics,
  parseAMData,
  resetSongLyric,
  formatToLrc
};

export default parseLyric;