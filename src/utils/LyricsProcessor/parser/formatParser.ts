/**
 * LyricsProcessor Format Parser
 * 歌词格式解析器 - LRC/YRC/AM格式转换 (优化版)
 */

import type {
  LyricLine,
  AMLLLine,
  ParsedLrcLine,
  ParsedYrcLine,
  InputLyricLine
} from '../types';
import { msToS } from '@/utils/timeTools';

// Pre-compiled regex for interlude detection
const INTERLUDE_CHARS_REGEX = /[\s♪♩♫♬🎵🎶🎼·…\-_—─●◆◇○■□▲△▼▽★☆♥♡❤💕、。，,.!！?？~～]/g;

// Debug flag - set to false in production for better performance
const DEBUG = false;

/**
 * 判断是否为间奏行
 */
function isInterludeContent(content: string): boolean {
  if (!content) return true;
  const stripped = content.replace(INTERLUDE_CHARS_REGEX, '');
  return stripped.length === 0;
}

/**
 * Process parsed Lyric data into easier to use format (优化版)
 * @param lrcData Array of LyricLine objects (times in ms)
 * @returns ParsedLrcLine[] with times in seconds
 */
export const parseLrcLines = (lrcData: LyricLine[]): ParsedLrcLine[] => {
  if (!lrcData || lrcData.length === 0) {
    return [];
  }

  const len = lrcData.length;
  const result: ParsedLrcLine[] = [];
  result.length = len; // Pre-allocate

  let count = 0;
  for (let i = 0; i < len; i++) {
    const line = lrcData[i];
    const words = line.words;

    if (!words || words.length === 0) continue;

    // Get start time and build content in one pass
    const startTime = words[0].startTime;
    let content = '';
    for (let j = 0; j < words.length; j++) {
      content += words[j].word || '';
    }

    const trimmed = content.trim();
    if (!trimmed) continue;

    result[count++] = {
      time: msToS(startTime),
      content: trimmed
    };
  }

  result.length = count;
  return result;
};

/**
 * Parse YRC (word-by-word) lyrics (优化版)
 * @param yrcData Array of LyricLine objects (times in ms)
 * @returns ParsedYrcLine[] with times in seconds
 */
export const parseYrcLines = (yrcData: LyricLine[]): ParsedYrcLine[] => {
  if (!yrcData || yrcData.length === 0) return [];

  const len = yrcData.length;
  const result: ParsedYrcLine[] = [];
  result.length = len;

  let count = 0;
  for (let i = 0; i < len; i++) {
    const line = yrcData[i];
    const words = line.words;

    if (!words || words.length === 0) continue;

    const wordsLen = words.length;
    const firstWord = words[0];
    const lastWord = words[wordsLen - 1];

    const time = msToS(firstWord.startTime);
    const endTime = msToS(lastWord.endTime);

    // Build content array and string in one pass
    const content: ParsedYrcLine['content'] = [];
    content.length = wordsLen;
    let textContent = '';

    for (let j = 0; j < wordsLen; j++) {
      const word = words[j];
      const wordText = word.word;
      const endsWithSpace = wordText.endsWith(' ');
      const processedWord = endsWithSpace ? wordText : wordText.trim();

      content[j] = {
        time: msToS(word.startTime),
        endTime: msToS(word.endTime),
        duration: msToS(word.endTime - word.startTime),
        content: processedWord,
        endsWithSpace
      };

      textContent += processedWord;
    }

    if (!textContent) continue;

    result[count++] = {
      time,
      endTime,
      content,
      TextContent: textContent
    };
  }

  result.length = count;
  return result;
};

/**
 * Parse lyrics for Apple Music like format using index-based matching (优化版)
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
  const lrcLen = lrcData.length;
  if (lrcLen === 0) return [];

  // Extract valid translation/romaji content in single pass
  const tranContents: string[] = [];
  const romaContents: string[] = [];

  if (tranData.length > 0) {
    for (let i = 0; i < tranData.length; i++) {
      const words = tranData[i].words;
      if (words && words.length > 0) {
        let content = '';
        for (let j = 0; j < words.length; j++) {
          content += words[j].word;
        }
        if (!isInterludeContent(content)) {
          tranContents.push(content);
        }
      }
    }
  }

  if (romaData.length > 0) {
    for (let i = 0; i < romaData.length; i++) {
      const words = romaData[i].words;
      if (words && words.length > 0) {
        let content = '';
        for (let j = 0; j < words.length; j++) {
          content += words[j].word;
        }
        if (!isInterludeContent(content)) {
          romaContents.push(content);
        }
      }
    }
  }

  // Collect valid main line indices
  const validMainIndices: number[] = [];
  validMainIndices.length = lrcLen;
  let validCount = 0;

  for (let i = 0; i < lrcLen; i++) {
    const words = lrcData[i].words;
    if (words && words.length > 0) {
      let content = '';
      for (let j = 0; j < words.length; j++) {
        content += words[j].word;
      }
      if (!isInterludeContent(content)) {
        validMainIndices[validCount++] = i;
      }
    }
  }
  validMainIndices.length = validCount;

  // Build index maps for translation/romaji
  const tranMap = new Map<number, string>();
  const romaMap = new Map<number, string>();

  // Index-based matching when counts are equal
  if (tranContents.length === validCount) {
    for (let i = 0; i < tranContents.length; i++) {
      tranMap.set(validMainIndices[i], tranContents[i]);
    }
  } else if (tranContents.length > 0 && tranData.length > 0) {
    // Time-based matching with binary search
    const mainTimes: { idx: number; time: number }[] = [];
    for (let i = 0; i < validCount; i++) {
      const idx = validMainIndices[i];
      mainTimes.push({ idx, time: lrcData[idx].words[0].startTime });
    }
    mainTimes.sort((a, b) => a.time - b.time);

    let tranIdx = 0;
    for (let i = 0; i < tranData.length && tranIdx < tranContents.length; i++) {
      const words = tranData[i].words;
      if (words && words.length > 0) {
        let content = '';
        for (let j = 0; j < words.length; j++) {
          content += words[j].word;
        }
        if (!isInterludeContent(content)) {
          const tranTime = words[0].startTime;

          // Binary search
          let left = 0, right = mainTimes.length - 1;
          let bestIdx = -1, bestDiff = Infinity;

          while (left <= right) {
            const mid = (left + right) >> 1;
            const diff = Math.abs(mainTimes[mid].time - tranTime);
            if (diff < bestDiff) {
              bestDiff = diff;
              bestIdx = mainTimes[mid].idx;
            }
            if (mainTimes[mid].time < tranTime) {
              left = mid + 1;
            } else {
              right = mid - 1;
            }
          }

          if (bestIdx >= 0 && bestDiff < 10000) {
            tranMap.set(bestIdx, tranContents[tranIdx]);
          }
          tranIdx++;
        }
      }
    }
  }

  if (romaContents.length === validCount) {
    for (let i = 0; i < romaContents.length; i++) {
      romaMap.set(validMainIndices[i], romaContents[i]);
    }
  } else if (romaContents.length > 0 && romaData.length > 0) {
    // Time-based matching with binary search
    const mainTimes: { idx: number; time: number }[] = [];
    for (let i = 0; i < validCount; i++) {
      const idx = validMainIndices[i];
      mainTimes.push({ idx, time: lrcData[idx].words[0].startTime });
    }
    mainTimes.sort((a, b) => a.time - b.time);

    let romaIdx = 0;
    for (let i = 0; i < romaData.length && romaIdx < romaContents.length; i++) {
      const words = romaData[i].words;
      if (words && words.length > 0) {
        let content = '';
        for (let j = 0; j < words.length; j++) {
          content += words[j].word;
        }
        if (!isInterludeContent(content)) {
          const romaTime = words[0].startTime;

          // Binary search
          let left = 0, right = mainTimes.length - 1;
          let bestIdx = -1, bestDiff = Infinity;

          while (left <= right) {
            const mid = (left + right) >> 1;
            const diff = Math.abs(mainTimes[mid].time - romaTime);
            if (diff < bestDiff) {
              bestDiff = diff;
              bestIdx = mainTimes[mid].idx;
            }
            if (mainTimes[mid].time < romaTime) {
              left = mid + 1;
            } else {
              right = mid - 1;
            }
          }

          if (bestIdx >= 0 && bestDiff < 10000) {
            romaMap.set(bestIdx, romaContents[romaIdx]);
          }
          romaIdx++;
        }
      }
    }
  }

  // Build result array
  const result: AMLLLine[] = [];
  result.length = lrcLen;

  for (let i = 0; i < lrcLen; i++) {
    const line = lrcData[i];
    const words = line.words || [];
    const wordsLen = words.length;

    const firstWord = wordsLen > 0 ? words[0] : null;
    const lastWord = wordsLen > 0 ? words[wordsLen - 1] : null;
    const startTime = firstWord ? firstWord.startTime : 0;

    // Calculate endTime
    let endTime: number;
    const nextLine = lrcData[i + 1];
    const nextFirstWord = nextLine?.words?.[0];

    if (nextFirstWord) {
      endTime = nextFirstWord.startTime;
    } else if (lastWord) {
      endTime = lastWord.endTime;
    } else {
      endTime = startTime + 5000;
    }

    if (endTime <= startTime) {
      endTime = startTime + 100;
    }

    // Build words array efficiently
    const resultWords: AMLLLine['words'] = [];
    resultWords.length = wordsLen;

    for (let j = 0; j < wordsLen; j++) {
      const w = words[j];
      resultWords[j] = {
        word: w.word,
        startTime: w.startTime,
        endTime: w.endTime,
        romanWord: ''
      };
    }

    result[i] = {
      words: resultWords,
      startTime,
      endTime,
      translatedLyric: tranMap.get(i) || '',
      romanLyric: romaMap.get(i) || '',
      isBG: line.isBG ?? false,
      isDuet: line.isDuet ?? false
    };
  }

  return result;
};

/**
 * 转换歌词行数据为 AMLL 格式 (优化版)
 * @param lines InputLyricLine[] 输入歌词行
 * @returns AMLLLine[] AMLL格式歌词行
 */
export function convertToAMLL(lines: InputLyricLine[]): AMLLLine[] {
  const len = lines.length;
  const result: AMLLLine[] = [];
  result.length = len;

  for (let i = 0; i < len; i++) {
    const l = lines[i];
    const sourceWords = l.words || [];
    const wordsLen = sourceWords.length;

    // Build words array
    const words: AMLLLine['words'] = [];
    words.length = wordsLen;

    for (let j = 0; j < wordsLen; j++) {
      const w = sourceWords[j];
      words[j] = {
        startTime: w.startTime,
        endTime: w.endTime,
        word: w.word,
        romanWord: ''
      };
    }

    const firstWord = words[0];
    const lastWord = words[wordsLen - 1];
    const startTime = l.startTime ?? firstWord?.startTime ?? 0;
    const endTime = l.endTime ?? lastWord?.endTime ?? startTime;

    result[i] = {
      words,
      translatedLyric: l.translatedLyric ?? '',
      romanLyric: l.romanLyric ?? '',
      isBG: l.isBG ?? false,
      isDuet: l.isDuet ?? false,
      startTime,
      endTime
    };
  }

  return result;
}

// Backward compatibility exports
export const parseLrcData = parseLrcLines;
export const parseYrcData = parseYrcLines;
export const parseAMData = buildAMLLData;

