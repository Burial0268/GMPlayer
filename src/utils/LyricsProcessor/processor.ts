/**
 * LyricsProcessor - 歌词处理核心模块 (优化版)
 *
 * 主要功能：
 * 1. 将歌词源数据转换为 AMLL 格式
 * 2. 严格的行索引匹配（第N行翻译对应第N行歌词）
 * 3. 缓存处理结果
 */

import { toRaw } from 'vue';
import type { LyricLine as AMLLLine } from '@applemusic-like-lyrics/core';
import type { SongLyric, ProcessingSettings, InputLyricLine, TimeTextEntry, StoredLyricLine } from './types';
import { parseLrcToEntries } from './entryParser';
import { isInterludeLine, buildIndexMatching } from './alignment';
import { convertToAMLL } from './formatParser';

// Debug flag - disable in production for better performance
const DEBUG = false;

/**
 * 生成设置状态的哈希值
 */
function generateSettingsHash(settings: ProcessingSettings): string {
  // Use bit flags for faster comparison (3 booleans = 3 bits)
  const flags = (settings.showYrc ? 4 : 0) | (settings.showRoma ? 2 : 0) | (settings.showTransl ? 1 : 0);
  return String(flags);
}

/**
 * 从 SongLyric 中提取翻译文本 (优化版 - 减少对象访问)
 */
function extractTranslationText(songLyric: SongLyric): string | undefined {
  const tlyric = songLyric.tlyric;
  if (tlyric?.lyric) return tlyric.lyric;

  const translation = songLyric.translation;
  if (typeof translation === 'string') return translation;
  if (translation && typeof translation === 'object' && 'lyric' in translation) {
    return translation.lyric;
  }
  return undefined;
}

/**
 * 从 SongLyric 中提取音译文本 (优化版 - 减少对象访问)
 */
function extractRomajiText(songLyric: SongLyric): string | undefined {
  const romalrc = songLyric.romalrc;
  if (romalrc?.lyric) return romalrc.lyric;

  const romaji = songLyric.romaji;
  if (typeof romaji === 'string') return romaji;
  if (romaji && typeof romaji === 'object' && 'lyric' in romaji) {
    return romaji.lyric;
  }
  return undefined;
}

/**
 * 处理歌词数据 - 使用行索引匹配 (优化版)
 *
 * @param songLyric 歌曲歌词
 * @param settings 设置状态
 * @returns 处理后的歌词行数组
 */
export function processLyrics(songLyric: SongLyric, settings: ProcessingSettings): AMLLLine[] {
  // 选择歌词源：TTML > YRC > LRC
  let rawLyricsSource: InputLyricLine[];

  const hasTTML = songLyric.hasTTML && songLyric.ttml && songLyric.ttml.length > 0;
  const hasYrc = settings.showYrc && songLyric.yrcAMData && songLyric.yrcAMData.length > 0;
  const hasLrc = songLyric.lrcAMData && songLyric.lrcAMData.length > 0;

  if (hasTTML) {
    rawLyricsSource = toRaw(songLyric.ttml) as InputLyricLine[];
  } else if (hasYrc) {
    rawLyricsSource = toRaw(songLyric.yrcAMData) as InputLyricLine[];
  } else if (hasLrc) {
    rawLyricsSource = toRaw(songLyric.lrcAMData) as InputLyricLine[];
  } else {
    return [];
  }

  // 转换为 AMLL 格式
  const amllLines = convertToAMLL(rawLyricsSource);

  // 过滤完全空白的行 - 使用更高效的过滤
  const validLines: AMLLLine[] = [];
  validLines.length = amllLines.length;
  let validCount = 0;

  for (let i = 0; i < amllLines.length; i++) {
    const line = amllLines[i];
    const words = line.words;
    if (!words || words.length === 0) continue;

    // Check if any word has content
    let hasContent = false;
    for (let j = 0; j < words.length; j++) {
      const word = words[j].word;
      if (word && word.trim().length > 0) {
        hasContent = true;
        break;
      }
    }

    if (hasContent) {
      validLines[validCount++] = line;
    }
  }
  validLines.length = validCount;

  // 早期返回：如果不需要翻译和音译
  if (!settings.showTransl && !settings.showRoma) {
    return validLines;
  }

  // 解析翻译和音译（只在需要时解析）
  let translationMatchMap: Map<number, string> | null = null;
  let romajiMatchMap: Map<number, string> | null = null;

  if (settings.showTransl) {
    const transText = extractTranslationText(songLyric);
    if (transText) {
      const translationEntries = parseLrcToEntries(transText);
      if (translationEntries.length > 0) {
        translationMatchMap = buildIndexMatching(validLines, translationEntries);
      }
    }
  }

  if (settings.showRoma) {
    const romaText = extractRomajiText(songLyric);
    if (romaText) {
      const romajiEntries = parseLrcToEntries(romaText);
      if (romajiEntries.length > 0) {
        romajiMatchMap = buildIndexMatching(validLines, romajiEntries);
      }
    }
  }

  // 如果没有任何匹配，直接返回
  if (!translationMatchMap && !romajiMatchMap) {
    return validLines;
  }

  // 应用翻译和音译 - 修改原数组以减少内存分配
  for (let i = 0; i < validCount; i++) {
    const line = validLines[i];

    // 处理翻译
    if (translationMatchMap) {
      // 优先使用行内已有的翻译（来自 TTML 等格式）
      if (!line.translatedLyric) {
        const matched = translationMatchMap.get(i);
        if (matched) {
          line.translatedLyric = matched;
        }
      }
    }

    // 处理音译
    if (romajiMatchMap) {
      if (!line.romanLyric) {
        const matched = romajiMatchMap.get(i);
        if (matched) {
          line.romanLyric = matched;
        }
      }
    }
  }

  return validLines;
}

/**
 * 预处理并缓存歌词数据 (优化版)
 *
 * @param songLyric 歌曲歌词
 * @param settings 设置状态
 */
export function preprocessLyrics(songLyric: SongLyric, settings: ProcessingSettings): void {
  const currentHash = generateSettingsHash(settings);

  // 检查缓存
  if (
    songLyric.processedLyrics &&
    songLyric.processedLyrics.length > 0 &&
    songLyric.settingsHash === currentHash
  ) {
    return;
  }

  songLyric.processedLyrics = processLyrics(songLyric, settings) as unknown as StoredLyricLine[];
  songLyric.settingsHash = currentHash;
}

/**
 * 获取处理后的歌词行，优先使用缓存 (优化版)
 *
 * @param songLyric 歌曲歌词
 * @param settings 设置状态
 * @returns 处理后的歌词行数组
 */
export function getProcessedLyrics(songLyric: SongLyric, settings: ProcessingSettings): AMLLLine[] {
  const currentHash = generateSettingsHash(settings);

  // 检查缓存 - StoredLyricLine 与 AMLLLine 结构兼容
  const cached = songLyric.processedLyrics;
  if (cached && cached.length > 0 && songLyric.settingsHash === currentHash) {
    return cached as unknown as AMLLLine[];
  }

  // 重新处理并缓存
  const processed = processLyrics(songLyric, settings);
  songLyric.processedLyrics = processed as unknown as StoredLyricLine[];
  songLyric.settingsHash = currentHash;

  return processed;
}
