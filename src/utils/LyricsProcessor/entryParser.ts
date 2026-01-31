/**
 * LyricsProcessor Entry Parser
 * LRC 格式解析器 - 严格时间匹配
 */

import type { TimeTextEntry } from './types';
import { parseLrcTime } from './timeUtils';

/**
 * 解析 LRC 文本为时间-文本条目数组
 * @param lrcText LRC 格式文本
 * @returns 时间-文本条目数组，按时间排序
 */
export function parseLrcToEntries(lrcText: string): TimeTextEntry[] {
  const entries: TimeTextEntry[] = [];
  if (!lrcText) return entries;

  const lines = lrcText.split('\n');
  for (const line of lines) {
    // 匹配 [mm:ss.xx] 或 [mm:ss.xxx] 或 [mm:ss] 格式
    const match = line.match(/^\[(\d+:\d+(?:\.\d+)?)\](.*)/);
    if (match) {
      const timeMs = parseLrcTime(match[1]);
      const text = match[2].trim();
      // 只添加有效的时间和非空文本
      if (timeMs >= 0 && text) {
        entries.push({ timeMs, text });
      }
    }
  }

  // 按时间排序
  entries.sort((a, b) => a.timeMs - b.timeMs);
  return entries;
}

/**
 * 构建时间到文本的 Map（用于严格匹配）
 * @param entries 时间-文本条目数组
 * @returns Map<timeMs, text>
 */
export function buildTimeMap(entries: TimeTextEntry[]): Map<number, string> {
  const map = new Map<number, string>();
  for (const entry of entries) {
    map.set(entry.timeMs, entry.text);
  }
  return map;
}

/**
 * 严格时间匹配：在指定容差范围内查找匹配
 * @param targetTime 目标时间 (毫秒)
 * @param timeMap 时间映射
 * @param tolerance 容差范围 (毫秒)，默认 500ms
 * @returns 匹配的文本，无匹配返回 undefined
 */
export function strictTimeMatch(
  targetTime: number,
  timeMap: Map<number, string>,
  tolerance: number = 500
): string | undefined {
  // 先尝试精确匹配
  if (timeMap.has(targetTime)) {
    return timeMap.get(targetTime);
  }

  // 在容差范围内查找最接近的
  let bestMatch: string | undefined;
  let bestDiff = tolerance + 1;

  const entries = Array.from(timeMap.entries());
  for (let i = 0; i < entries.length; i++) {
    const [time, text] = entries[i];
    const diff = Math.abs(time - targetTime);
    if (diff <= tolerance && diff < bestDiff) {
      bestDiff = diff;
      bestMatch = text;
    }
  }

  return bestMatch;
}
