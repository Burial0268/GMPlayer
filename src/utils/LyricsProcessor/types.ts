/**
 * LyricsProcessor - Type Definitions
 * 歌词处理模块类型定义
 */

import type { LyricLine as AMLLLine } from "@applemusic-like-lyrics/core";

// Re-export AMLL types
export type { LyricLine as AMLLLine, LyricWord as AMLLWord } from "@applemusic-like-lyrics/core";

// ============== Basic Types ==============

export interface LyricWord {
  word: string;
  startTime: number;
  endTime: number;
  romanWord?: string;
  /** 注音片段（TTML <ruby>）；沿用 AMLL 的 LyricWordBase 形状 */
  ruby?: { word: string; startTime: number; endTime: number }[];
}

// Backward compatibility alias
export type InputLyricWord = LyricWord;

export interface LyricLine {
  words: LyricWord[];
  isBG?: boolean;
  isDuet?: boolean;
  translatedLyric?: string;
  romanLyric?: string;
}

/** AMLL-compatible lyric line stored in Pinia (matches store structure) */
export interface StoredLyricLine {
  startTime: number;
  endTime: number;
  words: LyricWord[];
  translatedLyric?: string;
  romanLyric?: string;
  isBG?: boolean;
  isDuet?: boolean;
}

// ============== Parsed Output Types ==============

/** Parsed LRC line (time in seconds) */
export interface ParsedLrcLine {
  time: number;
  content: string;
  tran?: string;
  roma?: string;
}

/**
 * Parsed YRC line (time in seconds). Word timing lives in `yrcAMData`; this
 * only carries what index sync and the plain-text surfaces read.
 */
export interface ParsedYrcLine {
  time: number;
  endTime: number;
  TextContent: string;
  tran?: string;
  roma?: string;
}

// ============== API Response Types ==============

/** Raw lyric data from API */
export interface RawLyricData {
  lrc?: { lyric: string } | null;
  tlyric?: { lyric: string } | null;
  romalrc?: { lyric: string } | null;
  yrc?: { lyric: string } | null;
  ytlrc?: { lyric: string } | null;
  yromalrc?: { lyric: string } | null;
  code?: number;
  // LAAPI fields
  translation?: string;
  romaji?: string;
  // TTML fields
  hasTTML?: boolean;
  ttml?: any;
  meta?: LyricMeta;
}

// Backward compatibility alias
export type LyricData = RawLyricData;

/** Lyric metadata */
export interface LyricMeta {
  found: boolean;
  id: string;
  availableFormats?: string[];
  hasTranslation?: boolean;
  translationLanguages?: string[];
  translationLanguage?: string;
  hasRomaji?: boolean;
  foundNCM?: boolean;
  source?: string;
}

// ============== Parsed Result Types ==============

/** Complete parsed lyric result */
export interface ParsedLyricResult {
  hasLrcTran: boolean;
  hasLrcRoma: boolean;
  hasYrc: boolean;
  hasYrcTran: boolean;
  hasYrcRoma: boolean;
  hasTTML: boolean;
  /** TTML 里出现过的翻译语言（供切换），以及本次选中的那一种 */
  translationLanguages: string[];
  translationLanguage: string;
  lrc: ParsedLrcLine[];
  yrc: ParsedYrcLine[];
  ttml: any[];
  lrcAMData: AMLLLine[];
  yrcAMData: AMLLLine[];
}

// ============== Store Types ==============

/** Song lyric data stored in Pinia */
export interface SongLyric {
  // Feature flags
  hasLrcTran: boolean;
  hasLrcRoma: boolean;
  hasYrc: boolean;
  hasYrcTran: boolean;
  hasYrcRoma: boolean;
  hasTTML: boolean;
  translationLanguages: string[];
  translationLanguage: string;
  // Parsed lyric data
  lrc: ParsedLrcLine[];
  yrc: ParsedYrcLine[];
  ttml: StoredLyricLine[];
  // AMLL format data (stored with our LyricWord type)
  lrcAMData: StoredLyricLine[];
  yrcAMData: StoredLyricLine[];
  // Other fields
  processedLyrics?: StoredLyricLine[];
  settingsHash?: string;
  // Optional translation/romaji sources
  tlyric?: { lyric: string };
  romalrc?: { lyric: string };
  translation?: string | { lyric: string };
  romaji?: string | { lyric: string };
  meta?: LyricMeta;
}

/** Processing settings */
export interface ProcessingSettings {
  showYrc: boolean;
  showRoma: boolean;
  showTransl: boolean;
  showRuby: boolean;
}

// Backward compatibility alias
export type SettingState = ProcessingSettings;

// ============== Internal Types ==============

/** Time-text entry for matching */
export interface TimeTextEntry {
  timeMs: number;
  text: string;
}

/** Input lyric line (internal use) */
export interface InputLyricLine {
  words: LyricWord[];
  startTime?: number;
  endTime?: number;
  translatedLyric?: string;
  romanLyric?: string;
  isBG?: boolean;
  isDuet?: boolean;
  [key: string]: any;
}
