import getLanguageData from "./getLanguageData";
import dayjs from "dayjs";
import duration from "dayjs/plugin/duration";

// 初始化dayjs的duration插件
dayjs.extend(duration);

export const msToS = (milliseconds: number, decimalPlaces = 2): number => {
  return Number((milliseconds / 1000).toFixed(decimalPlaces));
};

export const msToTime = (milliseconds: number): string => {
  const dur = dayjs.duration(milliseconds, "milliseconds");
  return milliseconds < 3600000 ? dur.format("mm:ss") : dur.format("H:mm:ss");
};

/**
 * 歌曲时长时间戳转换
 * @param mss 毫秒数
 * @returns 格式为 "mm:ss" 的字符串
 *
 * 手算，不走 `date-fns`。两个原因，都是实测出来的：
 *
 * 1. **它算错过。** 旧写法是 `new Date(0)` + `setMilliseconds(mss)` 再 `format`，
 *    而 `format` 输出的是**本地时间**。UTC 偏移不是整小时的时区（印度 +5:30、
 *    尼泊尔 +5:45、南澳 +9:30、纽芬兰 -3:30）里，纪元零点的本地分钟数不是 0，于是
 *    3:24 的歌在印度显示成 `33:24`。整小时偏移的时区（含 +8）恰好看不出来，所以
 *    这个 bug 一直没被发现。
 * 2. **它在长列表的热路径上。** 歌单页一块 hydrate 1000 行，每行调一次；实测
 *    `date-fns` 版 10000 次 29 ms，手算 1.1 ms。一块的构造成本因此少掉近四成。
 *
 * 超过一小时不再回绕（旧写法 65 分钟会显示 `05:00`），而是照 `mm:ss` 的字面意思
 * 让分钟进到两位数以上——和同文件的 `getSongPlayingTime` 一致。
 */
export const getSongTime = (mss: number): string => {
  const totalSeconds = Number.isFinite(mss) ? Math.max(0, Math.floor(mss / 1000)) : 0;
  const minutes = String(Math.floor(totalSeconds / 60)).padStart(2, "0");
  const seconds = String(totalSeconds % 60).padStart(2, "0");
  return `${minutes}:${seconds}`;
};

/**
 * 获取时间戳对应的日期
 * @param mss - 时间戳
 * @returns 日期字符串
 */
export const getLongTime = (mss: number | string): string => {
  const date = new Date(parseInt(String(mss)));
  const y = date.getFullYear();
  const m = `0${date.getMonth() + 1}`.slice(-2);
  const d = `0${date.getDate()}`.slice(-2);
  return `${y}-${m}-${d}`;
};

/**
 * 网易云日推每天 6:00 更新，6:00 前仍对应前一天的推荐日期。
 */
export const getDailySongsDate = (date = new Date()): string => {
  const dailyDate = new Date(date);
  if (dailyDate.getHours() < 6) dailyDate.setDate(dailyDate.getDate() - 1);
  const y = dailyDate.getFullYear();
  const m = `0${dailyDate.getMonth() + 1}`.slice(-2);
  const d = `0${dailyDate.getDate()}`.slice(-2);
  return `${y}-${m}-${d}`;
};

/**
 * 将时间戳转化为对应的时间格式
 * @param t - 时间戳，单位为毫秒
 * @returns 转换后的时间字符串
 */
export const getCommentTime = (t: number): string => {
  const nowDate = new Date(); // Current date object
  const nowTime = nowDate.getTime(); // Current timestamp

  // Calculate today's 23:59:59.999 timestamp
  const todayLast = new Date(nowDate.setHours(23, 59, 59, 999)).getTime();

  // Create Date object from the provided timestamp
  const userDate = new Date(t);

  // Extract hours and minutes with zero-padding
  const UH = userDate.getHours().toString().padStart(2, "0");
  const Um = userDate.getMinutes().toString().padStart(2, "0");

  // Calculate time difference in milliseconds
  const timeDiff = nowTime - t;
  const minutes = Math.floor(timeDiff / 60000);

  // Constants for language data
  const just = getLanguageData("just");
  const minutesAgo = getLanguageData("minutesAgo");
  const yesterday = getLanguageData("yesterday");
  const month = getLanguageData("month");
  const day = getLanguageData("day");
  const year = getLanguageData("year");

  // Logic to determine and return formatted time string
  switch (true) {
    case timeDiff <= 60000:
      return just;
    case timeDiff <= 3600000:
      return `${minutes} ${minutesAgo}`;
    case t >= todayLast - 86400000 && t < todayLast:
      return `${UH}:${Um}`;
    case t >= todayLast - 172800000 && t < todayLast:
      return `${yesterday} ${UH}:${Um}`;
    case t >= todayLast - 31557600000 && t < todayLast:
      return `${userDate.getMonth() + 1}${month}${userDate.getDate()}${day}`;
    default:
      return `${userDate.getFullYear()}${year}${userDate.getMonth() + 1}${month}${userDate.getDate()}${day}`;
  }
};

/**
 * 过万/亿数字转化
 * @param num 需要格式化的数字
 * @returns 格式化后的字符串或原样返回的数字
 */
// Intl.NumberFormat 构造远比 format() 昂贵，而 locale 固定为运行环境默认值，
// 因此惰性构造一次后复用（列表渲染时每项都会调用本函数）
let compactFormatter: Intl.NumberFormat | null = null;
const getCompactFormatter = (): Intl.NumberFormat => {
  if (!compactFormatter) {
    compactFormatter = new Intl.NumberFormat(undefined, {
      minimumFractionDigits: 1,
      maximumFractionDigits: 1,
    });
  }
  return compactFormatter;
};

export const formatNumber = (num: number | string): string | number => {
  const n = Number(num);

  // If the number is less than 10000 or zero, return as-is
  if (n === 0 || n < 10000) return n;

  const formatter = getCompactFormatter();

  // 语言可在运行时切换，故单位文本仍每次读取
  if (n < 100000000) {
    return formatter.format(n / 10000) + getLanguageData("million");
  }
  return formatter.format(n / 100000000) + getLanguageData("billion");
};

const memo: Record<number, string> = {};
/**
 * 歌曲播放时间转换
 * @param num 歌曲播放时间，单位为秒
 * @returns 格式为 "mm:ss" 的字符串
 */
export const getSongPlayingTime = (num: number): string => {
  const wholeSeconds = Number.isFinite(num) ? Math.max(0, Math.floor(num)) : 0;
  // Check if result is memoized
  if (memo[wholeSeconds]) return memo[wholeSeconds];

  // Calculate minutes and seconds
  const minutes = String(Math.floor(wholeSeconds / 60)).padStart(2, "0");
  const seconds = String(wholeSeconds % 60).padStart(2, "0");

  // Combine minutes and seconds
  const formattedTime = `${minutes}:${seconds}`;

  // Memoize the result
  memo[wholeSeconds] = formattedTime;

  return formattedTime;
};
