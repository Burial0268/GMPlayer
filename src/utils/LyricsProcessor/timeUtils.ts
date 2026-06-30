/**
 * LyricsProcessor - Time Utilities
 * 时间相关工具函数 (优化版)
 */

// Pre-compiled regex patterns (avoid recompilation on each call)
const LRC_COLON_TIME_REGEX = /^(?:\d+:)+\d+(?:\.\d+)?$/;
const LRC_BARE_TIME_REGEX = /^\d+(?:\.\d+)?$/;
const LRC_TIMESTAMP_TAG_REGEX = /\[((?:\d+:)*\d+(?:\.\d+)?)\]/g;

type LrcTimestampMode = "default" | "seconds-fraction";

export interface LrcTimestampMatch {
  raw: string;
  timeText: string;
  timeMs: number;
  index: number;
}

function parseCompactCentisecondTime(timeStr: string): number {
  const integerPart = timeStr.split(".")[0];
  const value = parseInt(integerPart, 10);
  return Number.isFinite(value) ? value * 10 : -1;
}

function parseColonLrcTime(timeStr: string): number {
  const parts = timeStr.split(":");
  if (parts.length < 2) return -1;

  // Some lyric sources emit seconds plus a fractional field as [11:003.00].
  // Standard LRC seconds do not use three digits, so this is normalized as
  // 11.003s instead of letting AMLL treat it as 11m03s.
  const lastPart = parts[parts.length - 1];
  if (parts.length === 2 && /^\d{3,}(?:\.0+)?$/.test(lastPart)) {
    return parseSecondsFractionTime(timeStr);
  }

  let totalSeconds = 0;
  for (let i = 0; i < parts.length; i++) {
    const value = Number(parts[i]);
    if (!Number.isFinite(value)) return -1;
    totalSeconds = totalSeconds * 60 + value;
  }

  return Math.round(totalSeconds * 1000);
}

function parseSecondsFractionTime(timeStr: string): number {
  const parts = timeStr.split(":");
  if (parts.length !== 2) return parseLrcTime(timeStr);

  const seconds = parseInt(parts[0], 10);
  const fractionPart = parts[1].split(".")[0];
  const fraction = parseInt(fractionPart, 10);
  if (!Number.isFinite(seconds) || !Number.isFinite(fraction)) return -1;

  const multiplier = fractionPart.length <= 1 ? 100 : fractionPart.length === 2 ? 10 : 1;
  return seconds * 1000 + fraction * multiplier;
}

function parseBareLrcTime(timeStr: string): number {
  if (!LRC_BARE_TIME_REGEX.test(timeStr)) return -1;

  const dotIndex = timeStr.indexOf(".");
  if (dotIndex === -1) {
    return parseCompactCentisecondTime(timeStr);
  }

  const integerPart = timeStr.slice(0, dotIndex);
  const fractionPart = timeStr.slice(dotIndex + 1);
  if (integerPart.length >= 3 && /^0+$/.test(fractionPart)) {
    return parseCompactCentisecondTime(integerPart);
  }

  const seconds = Number(timeStr);
  return Number.isFinite(seconds) ? Math.round(seconds * 1000) : -1;
}

/**
 * 解析 LRC 时间戳，支持多种格式
 * @param timeStr 时间字符串 (如 "01:23.45"、"01:23.456"、"01:23"、"663")
 * @returns 毫秒数，解析失败返回 -1
 */
export function parseLrcTime(timeStr: string): number {
  if (!timeStr) return -1;
  const trimmed = timeStr.trim();
  if (!trimmed) return -1;

  if (LRC_COLON_TIME_REGEX.test(trimmed)) {
    return parseColonLrcTime(trimmed);
  }

  return parseBareLrcTime(trimmed);
}

function parseLrcTimeByMode(timeStr: string, mode: LrcTimestampMode): number {
  return mode === "seconds-fraction" ? parseSecondsFractionTime(timeStr) : parseLrcTime(timeStr);
}

function medianPositiveGap(times: number[]): number {
  if (times.length < 4) return 0;

  const sorted = times.filter((time) => time > 0).sort((a, b) => a - b);
  if (sorted.length < 4) return 0;

  const gaps: number[] = [];
  gaps.length = sorted.length - 1;

  let gapCount = 0;
  for (let i = 1; i < sorted.length; i++) {
    const gap = sorted[i] - sorted[i - 1];
    if (gap > 0) gaps[gapCount++] = gap;
  }
  gaps.length = gapCount;

  if (gapCount < 3) return 0;

  gaps.sort((a, b) => a - b);
  return gaps[gapCount >> 1];
}

export function detectLrcTimestampMode(lrcText: string): LrcTimestampMode {
  if (!lrcText) return "default";

  const defaultTimes: number[] = [];
  const alternateTimes: number[] = [];

  LRC_TIMESTAMP_TAG_REGEX.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = LRC_TIMESTAMP_TAG_REGEX.exec(lrcText))) {
    const timeText = match[1];
    if (!/^\d+:\d+(?:\.\d+)?$/.test(timeText)) continue;

    const defaultTime = parseLrcTime(timeText);
    const alternateTime = parseSecondsFractionTime(timeText);
    if (defaultTime >= 0 && alternateTime >= 0) {
      defaultTimes.push(defaultTime);
      alternateTimes.push(alternateTime);
    }
  }

  const defaultMedianGap = medianPositiveGap(defaultTimes);
  const alternateMedianGap = medianPositiveGap(alternateTimes);

  return defaultMedianGap >= 30000 && alternateMedianGap >= 300 && alternateMedianGap <= 15000
    ? "seconds-fraction"
    : "default";
}

function formatNormalizedLrcTime(timeMs: number): string {
  const normalized = Math.max(0, Math.round(timeMs));
  const min = Math.floor(normalized / 60000);
  const sec = Math.floor((normalized % 60000) / 1000);
  const ms = normalized % 1000;

  return `${min.toString().padStart(2, "0")}:${sec.toString().padStart(2, "0")}.${ms
    .toString()
    .padStart(3, "0")}`;
}

/**
 * 统一 LRC 文本中的时间戳格式，避免 AMLL 将 [663] 当作 663 秒。
 * AMLL 输出时间单位为毫秒，归一化后再交给 AMLL 解析可保持单位一致。
 */
export function normalizeLrcTimestampText(lrcText: string): string {
  if (!lrcText) return lrcText;

  const mode = detectLrcTimestampMode(lrcText);

  LRC_TIMESTAMP_TAG_REGEX.lastIndex = 0;
  return lrcText.replace(LRC_TIMESTAMP_TAG_REGEX, (raw: string, timeText: string) => {
    const timeMs = parseLrcTimeByMode(timeText, mode);
    return timeMs >= 0 ? `[${formatNormalizedLrcTime(timeMs)}]` : raw;
  });
}

/**
 * 读取一行 LRC 中的第一个可解析时间戳。
 */
export function parseFirstLrcTimestamp(
  line: string,
  mode: LrcTimestampMode = "default",
): LrcTimestampMatch | null {
  if (!line) return null;

  LRC_TIMESTAMP_TAG_REGEX.lastIndex = 0;
  const match = LRC_TIMESTAMP_TAG_REGEX.exec(line);
  if (!match) return null;

  const timeMs = parseLrcTimeByMode(match[1], mode);
  if (timeMs < 0) return null;

  return {
    raw: match[0],
    timeText: match[1],
    timeMs,
    index: match.index,
  };
}

/**
 * 移除一行 LRC 中的所有可解析时间戳标签。
 */
export function stripLrcTimestampTags(line: string): string {
  if (!line) return "";

  LRC_TIMESTAMP_TAG_REGEX.lastIndex = 0;
  return line.replace(LRC_TIMESTAMP_TAG_REGEX, "");
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
