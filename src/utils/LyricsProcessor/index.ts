/**
 * LyricsProcessor Module
 * 歌词处理模块 - 统一导出
 */

// Types
export type {
  LyricLine,
  LyricWord,
  AMLLLine,
  AMLLWord,
  StoredLyricLine,
  LyricData,
  LyricMeta,
  SongLyric,
  SettingState,
  ProcessingSettings,
  InputLyricLine,
  InputLyricWord,
  TimeTextEntry,
  ParsedLrcLine,
  ParsedYrcLine,
  RawLyricData,
  ParsedLyricResult,
} from './types';

// Time utilities
export { parseLrcTime, formatLrcTime, detectYrcType } from './timeUtils';

// Entry parsing
export { parseLrcToEntries, buildTimeMap, strictTimeMatch, strictTimeMatchBinary } from './entryParser';

// Format parsing
export { parseLrcLines, parseYrcLines, buildAMLLData, convertToAMLL } from './formatParser';

// Alignment
export { alignByIndex, isInterludeLine, buildIndexMatching } from './alignment';

// Main parser
export { parseLyricData, formatAsLrc, createEmptyLyricResult, resetSongLyric } from './lyricParser';

// Processing
export { processLyrics, preprocessLyrics, getProcessedLyrics } from './processor';

// Service
export { LyricService } from './service';

// === Backward compatibility aliases ===
// These allow old import names to keep working

export { parseLyricData as parseLyric } from './lyricParser';
export { formatAsLrc as formatToLrc } from './lyricParser';
export { parseLrcLines as parseLrcData } from './formatParser';
export { parseYrcLines as parseYrcData } from './formatParser';
export { buildAMLLData as parseAMData } from './formatParser';
export { alignByIndex as alignLyrics } from './alignment';
export { createEmptyLyricResult as getDefaultLyricResult } from './lyricParser';

// Default export for backward compatibility
export { parseLyricData as default } from './lyricParser';
