/**
 * LyricsProcessor Entry Parser
 * LRC 格式解析器 - 严格时间匹配 (优化版)
 */

import type { TimeTextEntry } from '../types';
import { parseLrcTime } from '../timeUtils';

// Pre-compiled regex (avoid recompilation on each call)
const LRC_LINE_REGEX = /^\[(\d+:\d+(?:\.\d+)?)\](.*)/;

/**
 * 解析 LRC 文本为时间-文本条目数组
 * @param lrcText LRC 格式文本
 * @returns 时间-文本条目数组，按时间排序
 */
export function parseLrcToEntries(lrcText: string): TimeTextEntry[] {
  if (!lrcText) return [];

  const lines = lrcText.split('\n');
  const entries: TimeTextEntry[] = [];
  entries.length = lines.length; // Pre-allocate approximate size

  let count = 0;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const match = LRC_LINE_REGEX.exec(line);
    if (match) {
      const timeMs = parseLrcTime(match[1]);
      const text = match[2].trim();
      if (timeMs >= 0 && text) {
        entries[count++] = { timeMs, text };
      }
    }
  }

  // Trim to actual size
  entries.length = count;

  // Sort by time (in-place)
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
  for (let i = 0; i < entries.length; i++) {
    map.set(entries[i].timeMs, entries[i].text);
  }
  return map;
}

/**
 * 严格时间匹配：在指定容差范围内查找匹配 (优化版 - 使用二分查找)
 * @param targetTime 目标时间 (毫秒)
 * @param sortedEntries 按时间排序的条目数组
 * @param tolerance 容差范围 (毫秒)，默认 500ms
 * @returns 匹配的文本，无匹配返回 undefined
 */
export function strictTimeMatchBinary(
  targetTime: number,
  sortedEntries: TimeTextEntry[],
  tolerance: number = 500
): string | undefined {
  if (sortedEntries.length === 0) return undefined;

  // Binary search to find closest entry
  let left = 0;
  let right = sortedEntries.length - 1;

  while (left < right) {
    const mid = (left + right) >> 1;
    if (sortedEntries[mid].timeMs < targetTime) {
      left = mid + 1;
    } else {
      right = mid;
    }
  }

  // Check the found position and its neighbors
  let bestMatch: string | undefined;
  let bestDiff = tolerance + 1;

  // Check positions: left-1, left, left+1
  for (let i = Math.max(0, left - 1); i <= Math.min(sortedEntries.length - 1, left + 1); i++) {
    const diff = Math.abs(sortedEntries[i].timeMs - targetTime);
    if (diff <= tolerance && diff < bestDiff) {
      bestDiff = diff;
      bestMatch = sortedEntries[i].text;
    }
  }

  return bestMatch;
}

/**
 * 严格时间匹配：在指定容差范围内查找匹配 (Map版本，保留兼容性)
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
  // Fast path: exact match
  const exact = timeMap.get(targetTime);
  if (exact !== undefined) return exact;

  // Convert to sorted array for binary search (cached if called multiple times)
  let bestMatch: string | undefined;
  let bestDiff = tolerance + 1;

  // For small maps, linear search is acceptable
  if (timeMap.size < 50) {
    for (const [time, text] of timeMap) {
      const diff = Math.abs(time - targetTime);
      if (diff <= tolerance && diff < bestDiff) {
        bestDiff = diff;
        bestMatch = text;
      }
    }
    return bestMatch;
  }

  // For larger maps, use sorted keys
  const times = Array.from(timeMap.keys()).sort((a, b) => a - b);
  let left = 0;
  let right = times.length - 1;

  while (left < right) {
    const mid = (left + right) >> 1;
    if (times[mid] < targetTime) {
      left = mid + 1;
    } else {
      right = mid;
    }
  }

  for (let i = Math.max(0, left - 1); i <= Math.min(times.length - 1, left + 1); i++) {
    const diff = Math.abs(times[i] - targetTime);
    if (diff <= tolerance && diff < bestDiff) {
      bestDiff = diff;
      bestMatch = timeMap.get(times[i]);
    }
  }

  return bestMatch;
}

