/**
 * LyricsProcessor Alignment
 * 歌词对齐工具函数
 */

import type { AMLLLine, ParsedLrcLine, ParsedYrcLine } from './types';

/**
 * 判断歌词行是否是间奏/空白行
 * 间奏行只包含符号（如 ♪♩🎵）或空白，没有实际歌词文字
 */
export function isInterludeLine(line: AMLLLine): boolean {
  if (!line.words || line.words.length === 0) return true;

  const fullText = line.words.map(w => w.word).join('').trim();
  if (!fullText) return true;

  // 仅包含音乐符号、标点、空白、分隔线的行视为间奏
  const strippedText = fullText.replace(
    /[\s♪♩♫♬🎵🎶🎼·…\-_—─●◆◇○■□▲△▼▽★☆♥♡❤💕、。，,.!！?？~～\u200B\u00A0]/g,
    ''
  );
  return strippedText.length === 0;
}

/**
 * Align lyrics with translations using index-based or time-based matching
 * When the count of valid other lines matches valid main lines, use index matching.
 * When counts differ (e.g., romaji source skips English lines), fall back to time-based matching.
 * @param lyrics Main lyrics array
 * @param otherLyrics Translation lyrics array
 * @param key Property key for translation ('tran' or 'roma')
 * @returns Aligned lyrics array
 */
export const alignByIndex = <T extends ParsedLrcLine | ParsedYrcLine>(
  lyrics: T[],
  otherLyrics: ParsedLrcLine[],
  key: 'tran' | 'roma'
): T[] => {
  if (!lyrics.length || !otherLyrics.length) {
    return lyrics;
  }

  console.log(`[alignByIndex] 开始对齐${key}歌词，主歌词${lyrics.length}行，辅助歌词${otherLyrics.length}行`);

  // 收集有效主歌词行的索引（非空行）
  const validMainIndices: number[] = [];
  for (let i = 0; i < lyrics.length; i++) {
    const line = lyrics[i];
    const isYrcLine = 'TextContent' in line;
    const content = isYrcLine ? (line as ParsedYrcLine).TextContent : (line as ParsedLrcLine).content;
    // 过滤间奏行（只有符号的行）
    const stripped = (content || '').replace(/[\s♪♩♫♬🎵🎶🎼·…\-_—─]/g, '');
    if (stripped.length > 0) {
      validMainIndices.push(i);
    }
  }

  // 收集有效翻译行（非空行）
  const validOtherLines: ParsedLrcLine[] = otherLyrics.filter(line => {
    const stripped = (line.content || '').replace(/[\s♪♩♫♬🎵🎶🎼·…\-_—─]/g, '');
    return stripped.length > 0;
  });

  console.log(`[alignByIndex] 有效主歌词行: ${validMainIndices.length}, 有效${key}行: ${validOtherLines.length}`);

  if (validMainIndices.length === validOtherLines.length) {
    // 数量相同，按索引一一对应
    console.log(`[alignByIndex] 行数匹配，使用索引对齐`);
    for (let i = 0; i < validMainIndices.length; i++) {
      const mainIdx = validMainIndices[i];
      (lyrics[mainIdx] as any)[key] = validOtherLines[i].content;
    }
    console.log(`[alignByIndex] 歌词对齐完成，匹配 ${validMainIndices.length} 行`);
  } else {
    // 数量不同（如音译不含英文行），使用时间最近匹配
    console.log(`[alignByIndex] 行数不匹配(主:${validMainIndices.length}, 辅:${validOtherLines.length})，使用时间匹配`);
    let matched = 0;
    for (const otherLine of validOtherLines) {
      let bestIdx = -1;
      let bestDiff = Infinity;
      for (const mainIdx of validMainIndices) {
        const mainTime = lyrics[mainIdx].time;
        const diff = Math.abs(mainTime - otherLine.time);
        if (diff < bestDiff) {
          bestDiff = diff;
          bestIdx = mainIdx;
        }
      }
      // 10秒容差（time字段单位为秒）
      if (bestIdx >= 0 && bestDiff < 10) {
        (lyrics[bestIdx] as any)[key] = otherLine.content;
        matched++;
      }
    }
    console.log(`[alignByIndex] 时间匹配完成，匹配 ${matched}/${validOtherLines.length} 行`);
  }

  return lyrics;
};

/**
 * 构建行索引匹配映射
 * 当条目数量与有效行数量相同时，按索引一一对应。
 * 当数量不同时（如音译不含英文行），按时间最近匹配。
 *
 * @param validLines 有效歌词行（已过滤空行）
 * @param entries 翻译/音译条目数组（按时间排序）
 * @returns Map<lineIndex, text> 行索引到文本的映射
 */
export function buildIndexMatching(
  validLines: AMLLLine[],
  entries: { timeMs: number; text: string }[]
): Map<number, string> {
  const result = new Map<number, string>();
  if (entries.length === 0) return result;

  // 收集非间奏行的索引
  const contentLineIndices: number[] = [];
  for (let i = 0; i < validLines.length; i++) {
    if (!isInterludeLine(validLines[i])) {
      contentLineIndices.push(i);
    }
  }

  if (entries.length === contentLineIndices.length) {
    // 数量相同，按索引一一对应
    for (let i = 0; i < entries.length; i++) {
      result.set(contentLineIndices[i], entries[i].text);
    }
    console.log(`[LyricsProcessor] 行索引匹配(索引): ${entries.length} 条 → ${contentLineIndices.length} 个有效行，匹配 ${result.size} 行`);
  } else {
    // 数量不同，按时间最近匹配
    for (const entry of entries) {
      let bestIdx = -1;
      let bestDiff = Infinity;
      for (const lineIdx of contentLineIndices) {
        const lineStartTime = validLines[lineIdx].startTime ?? 0;
        const diff = Math.abs(lineStartTime - entry.timeMs);
        if (diff < bestDiff) {
          bestDiff = diff;
          bestIdx = lineIdx;
        }
      }
      // 10秒容差（单位ms）
      if (bestIdx >= 0 && bestDiff < 10000) {
        result.set(bestIdx, entry.text);
      }
    }
    console.log(`[LyricsProcessor] 行索引匹配(时间): ${entries.length} 条翻译/音译 → ${contentLineIndices.length} 个有效行，匹配 ${result.size} 行`);
  }

  return result;
}

// Backward compatibility export
export const alignLyrics = alignByIndex;
