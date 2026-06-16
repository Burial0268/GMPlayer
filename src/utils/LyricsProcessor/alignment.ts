/**
 * LyricsProcessor Alignment
 * 歌词对齐工具函数 (优化版)
 */

import type { AMLLLine, ParsedLrcLine, ParsedYrcLine } from "./types";

// Pre-compiled regex for interlude detection (avoid recompilation)
const INTERLUDE_CHARS_REGEX =
  /[\s♪♩♫♬🎵🎶🎼·…\-_—─●◆◇○■□▲△▼▽★☆♥♡❤💕、。，,.!！?？~～\u200B\u00A0]/gu;
const INTERLUDE_CHARS_SIMPLE = /[\s♪♩♫♬🎵🎶🎼·…\-_—─]/gu;

/**
 * 判断歌词行是否是间奏/空白行
 * 间奏行只包含符号（如 ♪♩🎵）或空白，没有实际歌词文字
 */
export function isInterludeLine(line: AMLLLine): boolean {
  const words = line.words;
  if (!words || words.length === 0) return true;

  // Build text and check in one pass
  let hasContent = false;
  for (let i = 0; i < words.length; i++) {
    const word = words[i].word;
    if (word) {
      // Check if word has non-interlude characters
      const stripped = word.replace(INTERLUDE_CHARS_REGEX, "");
      if (stripped.length > 0) {
        hasContent = true;
        break;
      }
    }
  }

  return !hasContent;
}

/**
 * 快速判断文本是否为间奏行（简化版，用于内部优化）
 */
function isInterludeText(content: string): boolean {
  if (!content) return true;
  const stripped = content.replace(INTERLUDE_CHARS_SIMPLE, "");
  return stripped.length === 0;
}

/**
 * Align lyrics with translations using index-based or time-based matching (优化版)
 * @param lyrics Main lyrics array
 * @param otherLyrics Translation lyrics array
 * @param key Property key for translation ('tran' or 'roma')
 * @returns Aligned lyrics array (modified in place for memory efficiency)
 */
export const alignByIndex = <T extends ParsedLrcLine | ParsedYrcLine>(
  lyrics: T[],
  otherLyrics: ParsedLrcLine[],
  key: "tran" | "roma",
): T[] => {
  const lyricsLen = lyrics.length;
  const otherLen = otherLyrics.length;

  if (lyricsLen === 0 || otherLen === 0) {
    return lyrics;
  }

  // Collect valid indices in single pass
  const validMainIndices: number[] = [];
  validMainIndices.length = lyricsLen; // Pre-allocate
  let mainCount = 0;

  for (let i = 0; i < lyricsLen; i++) {
    const line = lyrics[i];
    const content =
      "TextContent" in line ? (line as ParsedYrcLine).TextContent : (line as ParsedLrcLine).content;
    if (!isInterludeText(content || "")) {
      validMainIndices[mainCount++] = i;
    }
  }
  validMainIndices.length = mainCount;

  // Collect valid other lines
  const validOtherLines: ParsedLrcLine[] = [];
  validOtherLines.length = otherLen;
  let otherCount = 0;

  for (let i = 0; i < otherLen; i++) {
    const line = otherLyrics[i];
    if (!isInterludeText(line.content || "")) {
      validOtherLines[otherCount++] = line;
    }
  }
  validOtherLines.length = otherCount;

  if (mainCount === otherCount) {
    // Index-based matching (O(n))
    for (let i = 0; i < mainCount; i++) {
      (lyrics[validMainIndices[i]] as any)[key] = validOtherLines[i].content;
    }
  } else {
    // Time-based matching with binary search for better performance
    // Sort valid main indices by time for binary search
    const mainWithTime = validMainIndices.map((idx) => ({
      idx,
      time: lyrics[idx].time,
    }));
    mainWithTime.sort((a, b) => a.time - b.time);

    for (let i = 0; i < otherCount; i++) {
      const otherLine = validOtherLines[i];
      const targetTime = otherLine.time;

      // Binary search for closest time
      let left = 0;
      let right = mainWithTime.length - 1;
      let bestIdx = -1;
      let bestDiff = Infinity;

      while (left <= right) {
        const mid = (left + right) >> 1;
        const diff = Math.abs(mainWithTime[mid].time - targetTime);

        if (diff < bestDiff) {
          bestDiff = diff;
          bestIdx = mainWithTime[mid].idx;
        }

        if (mainWithTime[mid].time < targetTime) {
          left = mid + 1;
        } else {
          right = mid - 1;
        }
      }

      // 10秒容差（time字段单位为秒）
      if (bestIdx >= 0 && bestDiff < 10) {
        (lyrics[bestIdx] as any)[key] = otherLine.content;
      }
    }
  }

  return lyrics;
};

/**
 * 构建行索引匹配映射 (优化版)
 * @param validLines 有效歌词行（已过滤空行）
 * @param entries 翻译/音译条目数组（按时间排序）
 * @returns Map<lineIndex, text> 行索引到文本的映射
 */
export function buildIndexMatching(
  validLines: AMLLLine[],
  entries: { timeMs: number; text: string }[],
): Map<number, string> {
  const result = new Map<number, string>();
  const entriesLen = entries.length;

  if (entriesLen === 0) return result;

  // Collect non-interlude line indices in single pass
  const contentLineIndices: number[] = [];
  contentLineIndices.length = validLines.length;
  let count = 0;

  for (let i = 0; i < validLines.length; i++) {
    if (!isInterludeLine(validLines[i])) {
      contentLineIndices[count++] = i;
    }
  }
  contentLineIndices.length = count;

  if (entriesLen === count) {
    // Index-based matching (O(n))
    for (let i = 0; i < entriesLen; i++) {
      result.set(contentLineIndices[i], entries[i].text);
    }
  } else {
    // Time-based matching with binary search
    // Pre-compute line times for binary search
    const lineTimes: { idx: number; time: number }[] = contentLineIndices.map((idx) => ({
      idx,
      time: validLines[idx].startTime ?? 0,
    }));
    lineTimes.sort((a, b) => a.time - b.time);

    for (let i = 0; i < entriesLen; i++) {
      const entry = entries[i];
      const targetTime = entry.timeMs;

      // Binary search
      let left = 0;
      let right = lineTimes.length - 1;
      let bestIdx = -1;
      let bestDiff = Infinity;

      while (left <= right) {
        const mid = (left + right) >> 1;
        const diff = Math.abs(lineTimes[mid].time - targetTime);

        if (diff < bestDiff) {
          bestDiff = diff;
          bestIdx = lineTimes[mid].idx;
        }

        if (lineTimes[mid].time < targetTime) {
          left = mid + 1;
        } else {
          right = mid - 1;
        }
      }

      // 10秒容差（单位ms）
      if (bestIdx >= 0 && bestDiff < 10000) {
        result.set(bestIdx, entry.text);
      }
    }
  }

  return result;
}

// Backward compatibility export
export const alignLyrics = alignByIndex;
