/**
 * LyricsProcessor - Time Utilities
 * 时间相关工具函数
 */

import type { TimeTextEntry } from './types';

/**
 * 解析 LRC 时间戳，支持多种格式
 * @param timeStr 时间字符串 (如 "01:23.45" 或 "01:23.456" 或 "01:23")
 * @returns 毫秒数，解析失败返回 -1
 */
export function parseLrcTime(timeStr: string): number {
  // 格式: mm:ss.xx 或 mm:ss.xxx 或 mm:ss
  const match = timeStr.match(/^(\d+):(\d+)(?:\.(\d+))?$/);
  if (!match) return -1;

  const min = parseInt(match[1]);
  const sec = parseInt(match[2]);
  const msStr = match[3] || '0';

  let timeMs: number;
  if (msStr.length === 3) {
    // 3位数是毫秒 (如 .123 = 123ms)
    timeMs = min * 60000 + sec * 1000 + parseInt(msStr);
  } else if (msStr.length === 2) {
    // 2位数是厘秒 (如 .12 = 120ms)
    timeMs = min * 60000 + sec * 1000 + parseInt(msStr) * 10;
  } else if (msStr.length === 1) {
    // 1位数是十分之一秒 (如 .1 = 100ms)
    timeMs = min * 60000 + sec * 1000 + parseInt(msStr) * 100;
  } else {
    timeMs = min * 60000 + sec * 1000;
  }

  return timeMs;
}

/**
 * 格式化毫秒为 LRC 时间戳
 * @param timeMs 毫秒数
 * @returns 格式化的时间字符串 "mm:ss.xx"
 */
export function formatLrcTime(timeMs: number): string {
  const min = Math.floor(timeMs / 60000);
  const sec = Math.floor((timeMs % 60000) / 1000);
  const cs = Math.floor((timeMs % 1000) / 10); // 厘秒
  return `${min.toString().padStart(2, '0')}:${sec.toString().padStart(2, '0')}.${cs.toString().padStart(2, '0')}`;
}

/**
 * 检测 YRC/QRC 格式类型
 * @param content 歌词内容
 * @returns 'yrc' 或 'qrc'
 */
export function detectYrcType(content: string): 'yrc' | 'qrc' {
  // YRC 特征检测
  if (content.includes('[x-trans') || content.includes('[merge]')) {
    return 'yrc';
  }
  // QRC 特征检测
  if (content.includes('<') && content.includes(',') && content.includes('>')) {
    return 'qrc';
  }
  // 默认返回 qrc
  return 'qrc';
}
