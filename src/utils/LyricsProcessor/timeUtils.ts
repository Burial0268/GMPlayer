/**
 * LyricsProcessor - Time Utilities
 * 时间相关工具函数 (优化版)
 */

// Pre-compiled regex patterns (avoid recompilation on each call)
const LRC_TIME_REGEX = /^(\d+):(\d+)(?:\.(\d+))?$/;

/**
 * 解析 LRC 时间戳，支持多种格式
 * @param timeStr 时间字符串 (如 "01:23.45" 或 "01:23.456" 或 "01:23")
 * @returns 毫秒数，解析失败返回 -1
 */
export function parseLrcTime(timeStr: string): number {
  const match = LRC_TIME_REGEX.exec(timeStr);
  if (!match) return -1;

  const min = parseInt(match[1], 10);
  const sec = parseInt(match[2], 10);
  const msStr = match[3];

  if (!msStr) {
    return min * 60000 + sec * 1000;
  }

  const msLen = msStr.length;
  const msVal = parseInt(msStr, 10);

  // Use lookup instead of conditionals
  const multipliers = [0, 100, 10, 1];
  const multiplier = multipliers[msLen] ?? 1;

  return min * 60000 + sec * 1000 + msVal * multiplier;
}

/**
 * 格式化毫秒为 LRC 时间戳
 * @param timeMs 毫秒数
 * @returns 格式化的时间字符串 "mm:ss.xx"
 */
export function formatLrcTime(timeMs: number): string {
  const totalSec = timeMs / 1000;
  const min = (totalSec / 60) | 0;
  const sec = (totalSec % 60) | 0;
  const cs = ((timeMs % 1000) / 10) | 0;

  return `${min < 10 ? "0" : ""}${min}:${sec < 10 ? "0" : ""}${sec}.${cs < 10 ? "0" : ""}${cs}`;
}

/**
 * 检测 YRC/QRC 格式类型
 * @param content 歌词内容
 * @returns 'yrc' 或 'qrc'
 */
export function detectYrcType(content: string): "yrc" | "qrc" {
  // Check for YRC markers first (more specific)
  if (content.includes("[x-trans") || content.includes("[merge]")) {
    return "yrc";
  }
  // QRC uses < > delimiters with comma-separated values
  if (content.indexOf("<") !== -1 && content.indexOf(",") !== -1 && content.indexOf(">") !== -1) {
    return "qrc";
  }
  return "qrc";
}
