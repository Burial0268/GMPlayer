import { toRaw } from 'vue';
// 使用 @applemusic-like-lyrics/core 的类型定义，与 AMLL-Editor 保持一致
import type { LyricLine as AMLLLine, LyricWord as AMLLWord } from '@applemusic-like-lyrics/core';

// 重新导出类型，方便其他模块使用
export type { LyricLine as AMLLLine, LyricWord as AMLLWord };
export type LyricLine = AMLLLine;
export type LyricWord = AMLLWord;

/**
 * 歌词数据接口
 */
export interface LyricData {
  lyric: string;
}

/**
 * 歌词元数据接口
 */
export interface LyricMeta {
  found: boolean;
  id: string;
  availableFormats?: string[]; // 如 ["yrc", "eslrc", "lrc", "ttml"]
  hasTranslation?: boolean;
  hasRomaji?: boolean;
}

/**
 * 歌曲歌词接口
 */
export interface SongLyric {
  lrcAMData: LyricLine[];
  yrcAMData: LyricLine[];
  hasTTML?: boolean;    // 是否拥有TTML格式歌词
  ttml?: LyricLine[];   // TTML解析后的数据
  tlyric?: { lyric: string }; // LAAPI 提供的翻译歌词对象
  romalrc?: { lyric: string }; // LAAPI 提供的音译歌词对象
  // 兼容新版LAAPI格式
  translation?: string | { lyric: string }; // 新版LAAPI 提供的翻译歌词 (LRC格式)
  romaji?: string | { lyric: string };      // 新版LAAPI 提供的音译歌词 (LRC格式)
  // 添加预处理后的缓存
  processedLyrics?: LyricLine[];
  // 处理设置的哈希值，用于检测设置是否变更
  settingsHash?: string;
  // 添加元数据信息
  meta?: LyricMeta;
}

/**
 * 设置状态接口
 */
export interface SettingState {
  showYrc: boolean;
  showRoma: boolean;
  showTransl: boolean;
}

/**
 * 生成设置状态的哈希值，用于判断设置是否变更
 * @param settings 设置状态
 * @returns 设置状态的哈希值
 */
function generateSettingsHash(settings: SettingState): string {
  return `${settings.showYrc}-${settings.showRoma}-${settings.showTransl}`;
}

/**
 * 转换LRC格式文本为时间映射
 * @param lrcText LRC格式文本
 * @returns 时间到文本的映射
 */
function parseLrcToTimeMap(lrcText: string): Map<number, string> {
  const timeMap = new Map<number, string>();
  if (!lrcText) return timeMap;
  
  const lines = lrcText.split('\n');
  lines.forEach(line => {
    const match = line.match(/^\[(\d+):(\d+)\.(\d+)\](.*)/);
    if (match) {
      const min = parseInt(match[1]);
      const sec = parseInt(match[2]);
      const ms = parseInt(match[3]);
      const timeMs = min * 60000 + sec * 1000 + ms * 10; // LRC时间格式转毫秒
      const text = match[4].trim();
      if (text) {
        timeMap.set(timeMs, text);
      }
    }
  });
  
  return timeMap;
}

/**
 * 查找最接近的时间戳匹配
 * 优先选择时间戳 <= targetTime 且最接近的匹配，确保翻译/音译与原文正确对应
 * @param targetTime 目标时间
 * @param timeMap 时间映射
 * @param tolerance 容差范围
 * @returns 匹配的文本
 */
function findBestTimeMatch(targetTime: number, timeMap: Map<number, string>, tolerance: number = 5000): string {
  // 首先尝试精确匹配
  if (timeMap.has(targetTime)) {
    return timeMap.get(targetTime) || "";
  }
  
  // 优先查找时间戳 <= targetTime 且最接近的匹配
  let bestMatch = "";
  let bestTime = -1;
  let minDiff = tolerance + 1;
  
  // 首先查找 <= targetTime 的最佳匹配
  for (const [time, text] of timeMap.entries()) {
    if (time <= targetTime) {
      const diff = targetTime - time;
      if (diff < minDiff) {
        minDiff = diff;
        bestTime = time;
        bestMatch = text;
      }
    }
  }
  
  // 如果没有找到 <= targetTime 的匹配，或者找到的匹配超出容差，尝试查找 > targetTime 的匹配（在容差范围内）
  if (bestTime < 0 || minDiff > tolerance) {
    let futureMinDiff = tolerance + 1;
    let futureMatch = "";
    
    for (const [time, text] of timeMap.entries()) {
      if (time > targetTime) {
        const diff = time - targetTime;
        if (diff < futureMinDiff && diff <= tolerance) {
          futureMinDiff = diff;
          futureMatch = text;
        }
      }
    }
    
    // 如果找到未来的匹配，且比过去的匹配更接近，使用未来的匹配
    if (futureMatch && (bestTime < 0 || futureMinDiff < minDiff)) {
      return futureMatch;
    }
  }
  
  return bestMatch;
}

/**
 * 预处理和缓存歌词数据，在获取歌词时提前处理以提高性能
 * @param songLyric 歌曲歌词
 * @param settings 设置状态
 */
export function preprocessLyrics(songLyric: SongLyric, settings: SettingState): void {
  // 生成当前设置的哈希值
  const currentHash = generateSettingsHash(settings);
  
  // 如果已经预处理过且设置没变，直接返回
  if (songLyric.processedLyrics && 
      songLyric.processedLyrics.length > 0 &&
      songLyric.settingsHash === currentHash) {
    console.log('[preprocessLyrics] 使用缓存的预处理歌词数据');
    return;
  }
  
  console.log('[preprocessLyrics] 开始预处理歌词数据');
  const startTime = performance.now();
  
  // 缓存处理后的歌词结果
  songLyric.processedLyrics = createLyricsProcessor(songLyric, settings);
  // 保存设置哈希值，用于检测设置是否变更
  songLyric.settingsHash = currentHash;
  
  const endTime = performance.now();
  console.log(`[preprocessLyrics] 预处理完成，耗时: ${(endTime - startTime).toFixed(2)}ms，行数: ${songLyric.processedLyrics.length}`);
}

/**
 * 处理歌词数据
 * @param songLyric 歌曲歌词
 * @param settings 设置状态
 * @returns 处理后的歌词行数组
 */
export function createLyricsProcessor(songLyric: SongLyric, settings: SettingState): LyricLine[] {
  // 优先级顺序：TTML > YRC > LRC
  let rawLyricsSource: InputLyricLine[] = []; // Use InputLyricLine for the source type

  // 选择合适的歌词源
  if (songLyric.hasTTML && songLyric.ttml && songLyric.ttml.length > 0) {
    console.log('[createLyricsProcessor] 使用TTML格式歌词');
    rawLyricsSource = toRaw(songLyric.ttml) as InputLyricLine[];
  }
  else if (settings.showYrc && songLyric.yrcAMData?.length) {
    console.log('[createLyricsProcessor] 使用YRC格式歌词');
    rawLyricsSource = toRaw(songLyric.yrcAMData) as InputLyricLine[];
  }
  else if (songLyric.lrcAMData?.length) { // Ensure lrcAMData exists before using it
    console.log('[createLyricsProcessor] 使用LRC格式歌词');
    rawLyricsSource = toRaw(songLyric.lrcAMData) as InputLyricLine[];
  } else {
    console.log('[createLyricsProcessor] 没有有效的歌词源数据');
    return []; // No data to process
  }

  // 预解析LRC格式的翻译和音译到时间映射表
  let translationMap: Map<number, string> = new Map();
  if (settings.showTransl) {
    let translationSourceText: string | undefined;
    if (songLyric.tlyric?.lyric) translationSourceText = songLyric.tlyric.lyric;
    else if (typeof songLyric.translation === 'string') translationSourceText = songLyric.translation;
    else if (typeof songLyric.translation === 'object' && songLyric.translation?.lyric) translationSourceText = songLyric.translation.lyric;
    if (translationSourceText) {
      translationMap = parseLrcToTimeMap(translationSourceText);
      // console.log(`[createLyricsProcessor] 解析翻译歌词完成，共${translationMap.size}行`);
    }
  }

  let romajiMap: Map<number, string> = new Map();
  if (settings.showRoma) {
    let romajiSourceText: string | undefined;
    if (songLyric.romalrc?.lyric) romajiSourceText = songLyric.romalrc.lyric;
    else if (typeof songLyric.romaji === 'string') romajiSourceText = songLyric.romaji;
    else if (typeof songLyric.romaji === 'object' && songLyric.romaji?.lyric) romajiSourceText = songLyric.romaji.lyric;
    if (romajiSourceText) {
      romajiMap = parseLrcToTimeMap(romajiSourceText);
      // console.log(`[createLyricsProcessor] 解析音译歌词完成，共${romajiMap.size}行`);
    }
  }

  // 首先转换为 AMLL 格式
  const amllLines = convertToAMLL(rawLyricsSource);
  
  // 过滤空行：没有 words 或 words 为空的歌词行
  const validLines = amllLines.filter((line) => {
    // 检查是否有有效的 words
    if (!line.words || line.words.length === 0) {
      return false;
    }
    // 检查 words 中是否有非空内容
    const hasContent = line.words.some(w => w.word && w.word.trim().length > 0);
    return hasContent;
  });
  
  // 然后根据设置应用翻译和音译
  return validLines.map((line) => {
    // 根据设置动态决定是否包含翻译和音译
    let translatedLyric = line.translatedLyric;
    if (settings.showTransl) {
      // 如果行内没有翻译，从LRC时间映射中查找
      if (!translatedLyric && translationMap.size > 0) {
        translatedLyric = findBestTimeMatch(line.startTime, translationMap);
      }
    } else {
      translatedLyric = ""; // 如果设置关闭，清空翻译
    }

    let romanLyric = line.romanLyric;
    if (settings.showRoma) {
      // 如果行内没有音译，从LRC时间映射中查找
      if (!romanLyric && romajiMap.size > 0) {
        romanLyric = findBestTimeMatch(line.startTime, romajiMap);
      }
    } else {
      romanLyric = ""; // 如果设置关闭，清空音译
    }

    return {
      ...line,
      translatedLyric,
      romanLyric,
    };
  });
}

/**
 * 转换歌词行数据为 AMLL 格式
 * 直接复制 AMLL-Editor 的 convertToAMLL 实现模式
 * @see https://github.com/Linho1219/AMLL-Editor/blob/main/src/core/convert/amll.ts
 * 
 * @param lines 内部歌词行数组（包含 words 数组，来自 parseAMData）
 * @returns AMLL 格式的歌词行数组（符合 @applemusic-like-lyrics/core 的 LyricLine 类型）
 */
function convertToAMLL(lines: InputLyricLine[]): AMLLLine[] {
  return lines.map((l) => {
    const words = (l.words || []).map((w) => ({
      startTime: w.startTime,
      endTime: w.endTime,
      word: w.word,
      romanWord: (w as any).romanWord ?? (w as any).romanization ?? "",
      obscene: (w as any).obscene ?? false,
    }));

    const firstWord = words[0];
    const lastWord = words[words.length - 1];
    const startTime = l.startTime ?? firstWord?.startTime ?? 0;
    const endTime = l.endTime ?? lastWord?.endTime ?? startTime;

    return {
      words,
      translatedLyric: l.translatedLyric ?? "",
      romanLyric: l.romanLyric ?? "",
      isBG: l.isBG ?? false,
      isDuet: l.isDuet ?? false,
      startTime,
      endTime,
    };
  });
}

/**
 * 获取处理后的歌词行，优先使用缓存
 * @param songLyric 歌曲歌词
 * @param settings 设置状态
 * @returns 处理后的歌词行数组
 */
export function getProcessedLyrics(songLyric: SongLyric, settings: SettingState): LyricLine[] {
  // 生成当前设置的哈希值
  const currentHash = generateSettingsHash(settings);
  
  // 如果有缓存且设置未变，直接返回缓存
  if (songLyric.processedLyrics && 
      songLyric.processedLyrics.length > 0 && 
      songLyric.settingsHash === currentHash) {
    console.log('[getProcessedLyrics] 使用缓存的歌词数据');
    return songLyric.processedLyrics;
  }
  
  // 否则，重新处理并更新缓存
  console.log('[getProcessedLyrics] 缓存未命中，重新处理歌词');
  songLyric.processedLyrics = createLyricsProcessor(songLyric, settings);
  songLyric.settingsHash = currentHash;
  
  return songLyric.processedLyrics;
}

//确保 InputLyricLine 和 InputLyricWord 类型定义与 @applemusic-like-lyrics/lyric 库的导出一致
//假设它们是这样的：
interface InputLyricWord {
  word: string;
  startTime: number;
  endTime: number;
  [key: string]: any; // 其他可能的属性
}

interface InputLyricLine {
  words: InputLyricWord[];
  startTime?: number; // 行的开始时间 (毫秒)
  endTime?: number;   // 行的结束时间 (毫秒)
  translatedLyric?: string; // 可能已预填充
  romanLyric?: string;    // 可能已预填充
  isBG?: boolean;
  isDuet?: boolean;
  [key: string]: any; // 其他可能的属性
}