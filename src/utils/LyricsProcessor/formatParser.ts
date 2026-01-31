/**
 * LyricsProcessor Format Parser
 * 歌词格式解析器 - LRC/YRC/AM格式转换
 */

import type {
  LyricLine,
  LyricWord,
  AMLLLine,
  ParsedLrcLine,
  ParsedYrcLine,
  InputLyricLine
} from './types';
import { msToS } from '@/utils/timeTools';

/**
 * Process parsed Lyric data into easier to use format
 * @param lrcData Array of LyricLine objects (times in ms)
 * @returns ParsedLrcLine[] with times in seconds
 */
export const parseLrcLines = (lrcData: LyricLine[]): ParsedLrcLine[] => {
  if (!lrcData || !lrcData.length) {
    console.warn('[parseLrcLines] 输入的歌词数据为空');
    return [];
  }

  console.log(`[parseLrcLines] 开始处理${lrcData.length}行歌词数据`);

  const result = lrcData.map((line, index) => {
    // 确保line和line.words存在
    if (!line || !line.words || !line.words.length) {
      console.warn(`[parseLrcLines] 第${index}行数据不完整`);
      return null;
    }

    // 获取行开始时间（转换为秒）
    let time = 0;
    if (line.words && line.words.length > 0) {
      time = msToS(line.words[0].startTime);
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
      console.log(`[parseLrcLines] 处理第${index}行: 时间=${time}s, 内容="${content.substring(0, 15)}..."`);
    }

    return {
      time,
      content,
    };
  }).filter((line): line is ParsedLrcLine => line !== null);

  console.log(`[parseLrcLines] 处理完成，输出${result.length}行有效歌词`);
  return result;
};

/**
 * Parse YRC (word-by-word) lyrics
 * @param yrcData Array of LyricLine objects (times in ms)
 * @returns ParsedYrcLine[] with times in seconds
 */
export const parseYrcLines = (yrcData: LyricLine[]): ParsedYrcLine[] => {
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
    .filter((line): line is ParsedYrcLine => line !== null);
};

/**
 * Parse lyrics for Apple Music like format using index-based matching
 * @param lrcData Main lyrics array (times in ms)
 * @param tranData Translation lyrics array (times in ms)
 * @param romaData Romanization lyrics array (times in ms)
 * @returns AMLLLine[] Formatted lyrics array
 */
export const buildAMLLData = (
  lrcData: LyricLine[],
  tranData: LyricLine[] = [],
  romaData: LyricLine[] = []
): AMLLLine[] => {
  console.log(`[buildAMLLData] 开始处理AM格式歌词，主歌词${lrcData.length}行，翻译${tranData.length}行，音译${romaData.length}行`);

  // 判断是否为间奏行
  const isInterludeLine = (content: string): boolean => {
    if (!content) return true;
    const stripped = content.replace(/[\s♪♩♫♬🎵🎶🎼·…\-_—─●◆◇○■□▲△▼▽★☆♥♡❤💕、。，,.!！?？~～]/g, '');
    return stripped.length === 0;
  };

  // 提取翻译内容数组（按顺序）
  const tranContents: string[] = [];
  if (tranData.length > 0) {
    for (const line of tranData) {
      if (line.words && line.words.length > 0) {
        const content = line.words.map(w => w.word).join('');
        if (!isInterludeLine(content)) {
          tranContents.push(content);
        }
      }
    }
    console.log(`[buildAMLLData] 提取有效翻译内容 ${tranContents.length} 行`);
  }

  // 提取音译内容数组（按顺序）
  const romaContents: string[] = [];
  if (romaData.length > 0) {
    for (const line of romaData) {
      if (line.words && line.words.length > 0) {
        const content = line.words.map(w => w.word).join('');
        if (!isInterludeLine(content)) {
          romaContents.push(content);
        }
      }
    }
    console.log(`[buildAMLLData] 提取有效音译内容 ${romaContents.length} 行`);
  }

  // 收集有效主歌词行的索引
  const validMainIndices: number[] = [];
  for (let i = 0; i < lrcData.length; i++) {
    const line = lrcData[i];
    if (line.words && line.words.length > 0) {
      const content = line.words.map(w => w.word).join('');
      if (!isInterludeLine(content)) {
        validMainIndices.push(i);
      }
    }
  }

  // 构建索引到翻译/音译的映射
  const tranMap = new Map<number, string>();
  const romaMap = new Map<number, string>();

  if (tranContents.length === validMainIndices.length) {
    // 数量相同，按索引一一对应
    for (let i = 0; i < tranContents.length; i++) {
      tranMap.set(validMainIndices[i], tranContents[i]);
    }
  } else if (tranContents.length > 0) {
    // 数量不同，按时间最近匹配
    console.log(`[buildAMLLData] 翻译行数(${tranContents.length})与主歌词有效行数(${validMainIndices.length})不同，使用时间匹配`);
    // 提取有效翻译行的时间戳
    let tranIdx = 0;
    for (const line of tranData) {
      if (line.words && line.words.length > 0) {
        const content = line.words.map(w => w.word).join('');
        if (!isInterludeLine(content)) {
          if (tranIdx < tranContents.length) {
            const tranStartTime = line.words[0].startTime;
            let bestMainIdx = -1;
            let bestDiff = Infinity;
            for (const mainIdx of validMainIndices) {
              const mainStartTime = lrcData[mainIdx].words[0].startTime;
              const diff = Math.abs(mainStartTime - tranStartTime);
              if (diff < bestDiff) {
                bestDiff = diff;
                bestMainIdx = mainIdx;
              }
            }
            if (bestMainIdx >= 0 && bestDiff < 10000) {
              tranMap.set(bestMainIdx, tranContents[tranIdx]);
            }
            tranIdx++;
          }
        }
      }
    }
  }

  if (romaContents.length === validMainIndices.length) {
    // 数量相同，按索引一一对应
    for (let i = 0; i < romaContents.length; i++) {
      romaMap.set(validMainIndices[i], romaContents[i]);
    }
  } else if (romaContents.length > 0) {
    // 数量不同，按时间最近匹配
    console.log(`[buildAMLLData] 音译行数(${romaContents.length})与主歌词有效行数(${validMainIndices.length})不同，使用时间匹配`);
    // 提取有效音译行的时间戳
    let romaIdx = 0;
    for (const line of romaData) {
      if (line.words && line.words.length > 0) {
        const content = line.words.map(w => w.word).join('');
        if (!isInterludeLine(content)) {
          if (romaIdx < romaContents.length) {
            const romaStartTime = line.words[0].startTime;
            let bestMainIdx = -1;
            let bestDiff = Infinity;
            for (const mainIdx of validMainIndices) {
              const mainStartTime = lrcData[mainIdx].words[0].startTime;
              const diff = Math.abs(mainStartTime - romaStartTime);
              if (diff < bestDiff) {
                bestDiff = diff;
                bestMainIdx = mainIdx;
              }
            }
            if (bestMainIdx >= 0 && bestDiff < 10000) {
              romaMap.set(bestMainIdx, romaContents[romaIdx]);
            }
            romaIdx++;
          }
        }
      }
    }
  }

  console.log(`[buildAMLLData] 翻译匹配 ${tranMap.size} 行，音译匹配 ${romaMap.size} 行`);

  const resultAM = lrcData.map((line, index, lines) => {
    const mainLineFirstWord = line.words && line.words.length > 0 ? line.words[0] : null;
    const mainLineLastWord = line.words && line.words.length > 0 ? line.words[line.words.length - 1] : null;

    const startTimeMs = mainLineFirstWord ? mainLineFirstWord.startTime : 0;

    // Calculate endTimeMs
    let endTimeMs;
    const nextLineFirstWord = lines[index + 1]?.words && lines[index + 1].words.length > 0 ? lines[index + 1].words[0] : null;
    if (nextLineFirstWord) {
      endTimeMs = nextLineFirstWord.startTime;
    } else if (mainLineLastWord) {
      endTimeMs = mainLineLastWord.endTime;
    } else {
      endTimeMs = startTimeMs + 5000;
    }

    if (endTimeMs <= startTimeMs) {
      endTimeMs = startTimeMs + 100;
    }

    // 使用索引匹配获取翻译和音译
    const translatedLyric = tranMap.get(index) || "";
    const romanLyric = romaMap.get(index) || "";

    const words = (line.words || []).map(w => ({
      word: w.word,
      startTime: w.startTime,
      endTime: w.endTime,
      ...(w as any),
    }));

    return {
      words,
      startTime: startTimeMs,
      endTime: endTimeMs,
      translatedLyric,
      romanLyric,
      isBG: line.isBG ?? false,
      isDuet: line.isDuet ?? false,
    };
  });

  console.log(`[buildAMLLData] AM格式处理完成，共生成${resultAM.length}行`);
  if (resultAM.length > 0 && tranData.length > 0) {
    const amTranslatedCount = resultAM.filter(r => r.translatedLyric && r.translatedLyric !== "").length;
    console.log(`[buildAMLLData] 在生成的AM数据中，${amTranslatedCount}/${resultAM.length} 行包含有效翻译。`);
  }
  return resultAM;
};

/**
 * 转换歌词行数据为 AMLL 格式
 * @param lines InputLyricLine[] 输入歌词行
 * @returns AMLLLine[] AMLL格式歌词行
 */
export function convertToAMLL(lines: InputLyricLine[]): AMLLLine[] {
  return lines.map((l) => {
    const words = (l.words || []).map((w) => ({
      startTime: w.startTime,
      endTime: w.endTime,
      word: w.word,
      romanWord: (w as any).romanWord ?? (w as any).romanization ?? "",
      obscene: (w as any).obscene ?? false,
    }));

    const firstWord = words[0];
    const lastWord = words[words.length - 1];
    const startTime = l.startTime ?? firstWord?.startTime ?? 0;
    const endTime = l.endTime ?? lastWord?.endTime ?? startTime;

    return {
      words,
      translatedLyric: l.translatedLyric ?? "",
      romanLyric: l.romanLyric ?? "",
      isBG: l.isBG ?? false,
      isDuet: l.isDuet ?? false,
      startTime,
      endTime,
    };
  });
}

// Backward compatibility exports
export const parseLrcData = parseLrcLines;
export const parseYrcData = parseYrcLines;
export const parseAMData = buildAMLLData;
