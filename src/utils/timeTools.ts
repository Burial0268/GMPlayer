import getLanguageData from "./getLanguageData";
import { format } from 'date-fns';
import dayjs from 'dayjs';
import duration from 'dayjs/plugin/duration';

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
 */
export const getSongTime = (mss: number): string => {
  const date = new Date(0);
  date.setMilliseconds(mss);

  // Format the date as mm:ss
  return format(date, 'mm:ss');
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
export const formatNumber = (num: number | string): string | number => {
  const n = Number(num);

  // If the number is less than 10000 or zero, return as-is
  if (n === 0 || n < 10000) return n;

  // Define formatter based on the current locale
  const formatter = new Intl.NumberFormat(undefined, {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1
  });

  // Function to format the number into million or billion
  const formatToMillionOrBillion = (number: number): string => {
    const million = getLanguageData("million"); // Replace with your i18n function
    const billion = getLanguageData("billion"); // Replace with your i18n function

    if (number < 100000000) {
      return formatter.format(number / 10000) + million;
    } else {
      return formatter.format(number / 100000000) + billion;
    }
  };

  // Format based on the current locale
  return formatToMillionOrBillion(n);
};

const memo: Record<number, string> = {};
/**
 * 歌曲播放时间转换
 * @param num 歌曲播放时间，单位为秒
 * @returns 格式为 "mm:ss" 的字符串
 */
export const getSongPlayingTime = (num: number): string => {
  // Check if result is memoized
  if (memo[num]) return memo[num];

  // Calculate minutes and seconds
  const minutes = String(Math.floor(num / 60)).padStart(2, "0");
  const seconds = String(Math.floor(num % 60)).padStart(2, "0");

  // Combine minutes and seconds
  const formattedTime = `${minutes}:${seconds}`;

  // Memoize the result
  memo[num] = formattedTime;

  return formattedTime;
};
