/**
 * LyricsProcessor - 歌词处理核心模块
 *
 * 主要功能：
 * 1. 将歌词源数据转换为 AMLL 格式
 * 2. 严格的行索引匹配（第N行翻译对应第N行歌词）
 * 3. 缓存处理结果
 */

import { toRaw } from 'vue';
import type { LyricLine as AMLLLine } from '@applemusic-like-lyrics/core';
import type { SongLyric, ProcessingSettings, InputLyricLine, TimeTextEntry } from './types';
import { parseLrcToEntries } from './entryParser';
import { isInterludeLine, buildIndexMatching } from './alignment';
import { convertToAMLL } from './formatParser';

/**
 * 生成设置状态的哈希值
 */
function generateSettingsHash(settings: ProcessingSettings): string {
  return `${settings.showYrc}-${settings.showRoma}-${settings.showTransl}`;
}

/**
 * 从 SongLyric 中提取翻译文本
 */
function extractTranslationText(songLyric: SongLyric): string | undefined {
  if (songLyric.tlyric?.lyric) return songLyric.tlyric.lyric;
  if (typeof songLyric.translation === 'string') return songLyric.translation;
  if (typeof songLyric.translation === 'object' && songLyric.translation?.lyric) {
    return songLyric.translation.lyric;
  }
  return undefined;
}

/**
 * 从 SongLyric 中提取音译文本
 */
function extractRomajiText(songLyric: SongLyric): string | undefined {
  if (songLyric.romalrc?.lyric) return songLyric.romalrc.lyric;
  if (typeof songLyric.romaji === 'string') return songLyric.romaji;
  if (typeof songLyric.romaji === 'object' && songLyric.romaji?.lyric) {
    return songLyric.romaji.lyric;
  }
  return undefined;
}

/**
 * 处理歌词数据 - 使用行索引匹配
 *
 * @param songLyric 歌曲歌词
 * @param settings 设置状态
 * @returns 处理后的歌词行数组
 */
export function processLyrics(songLyric: SongLyric, settings: ProcessingSettings): AMLLLine[] {
  // 选择歌词源：TTML > YRC > LRC
  let rawLyricsSource: InputLyricLine[] = [];

  if (songLyric.hasTTML && songLyric.ttml && songLyric.ttml.length > 0) {
    console.log('[LyricsProcessor] 使用 TTML 格式歌词');
    rawLyricsSource = toRaw(songLyric.ttml) as InputLyricLine[];
  } else if (settings.showYrc && songLyric.yrcAMData?.length) {
    console.log('[LyricsProcessor] 使用 YRC 格式歌词');
    rawLyricsSource = toRaw(songLyric.yrcAMData) as InputLyricLine[];
  } else if (songLyric.lrcAMData?.length) {
    console.log('[LyricsProcessor] 使用 LRC 格式歌词');
    rawLyricsSource = toRaw(songLyric.lrcAMData) as InputLyricLine[];
  } else {
    console.log('[LyricsProcessor] 没有有效的歌词源数据');
    return [];
  }

  // 转换为 AMLL 格式
  const amllLines = convertToAMLL(rawLyricsSource);

  // 过滤完全空白的行（保留间奏行，但间奏行不会获得翻译/音译）
  const validLines = amllLines.filter((line) => {
    if (!line.words || line.words.length === 0) return false;
    return line.words.some(w => w.word && w.word.trim().length > 0);
  });

  // 解析翻译和音译
  let translationEntries: TimeTextEntry[] = [];
  let romajiEntries: TimeTextEntry[] = [];

  if (settings.showTransl) {
    const transText = extractTranslationText(songLyric);
    if (transText) {
      translationEntries = parseLrcToEntries(transText);
      console.log(`[LyricsProcessor] 解析翻译歌词: ${translationEntries.length} 行`);
    }
  }

  if (settings.showRoma) {
    const romaText = extractRomajiText(songLyric);
    if (romaText) {
      romajiEntries = parseLrcToEntries(romaText);
      console.log(`[LyricsProcessor] 解析音译歌词: ${romajiEntries.length} 行`);
    }
  }

  // 构建行索引匹配（按行号一一对应，不基于时间）
  const translationMatchMap = settings.showTransl && translationEntries.length > 0
    ? buildIndexMatching(validLines, translationEntries)
    : new Map<number, string>();

  const romajiMatchMap = settings.showRoma && romajiEntries.length > 0
    ? buildIndexMatching(validLines, romajiEntries)
    : new Map<number, string>();

  // 应用翻译和音译
  return validLines.map((line, index) => {
    let translatedLyric = "";
    let romanLyric = "";

    // 处理翻译 - 行索引匹配
    if (settings.showTransl) {
      // 优先使用行内已有的翻译（来自 TTML 等格式）
      if (line.translatedLyric) {
        translatedLyric = line.translatedLyric;
      } else {
        translatedLyric = translationMatchMap.get(index) || "";
      }
    }

    // 处理音译 - 行索引匹配
    if (settings.showRoma) {
      // 优先使用行内已有的音译
      if (line.romanLyric) {
        romanLyric = line.romanLyric;
      } else {
        romanLyric = romajiMatchMap.get(index) || "";
      }
    }

    return {
      ...line,
      translatedLyric,
      romanLyric,
    };
  });
}

/**
 * 预处理并缓存歌词数据
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
    console.log('[LyricsProcessor] 使用缓存的预处理歌词数据');
    return;
  }

  console.log('[LyricsProcessor] 开始预处理歌词数据');
  const startTime = performance.now();

  songLyric.processedLyrics = processLyrics(songLyric, settings);
  songLyric.settingsHash = currentHash;

  const endTime = performance.now();
  console.log(
    `[LyricsProcessor] 预处理完成，耗时: ${(endTime - startTime).toFixed(2)}ms，行数: ${songLyric.processedLyrics.length}`
  );
}

/**
 * 获取处理后的歌词行，优先使用缓存
 *
 * @param songLyric 歌曲歌词
 * @param settings 设置状态
 * @returns 处理后的歌词行数组
 */
export function getProcessedLyrics(songLyric: SongLyric, settings: ProcessingSettings): AMLLLine[] {
  const currentHash = generateSettingsHash(settings);

  // 检查缓存
  if (
    songLyric.processedLyrics &&
    songLyric.processedLyrics.length > 0 &&
    songLyric.settingsHash === currentHash
  ) {
    return songLyric.processedLyrics;
  }

  // 重新处理并缓存
  console.log('[LyricsProcessor] 缓存未命中，重新处理歌词');
  songLyric.processedLyrics = processLyrics(songLyric, settings);
  songLyric.settingsHash = currentHash;

  return songLyric.processedLyrics;
}
