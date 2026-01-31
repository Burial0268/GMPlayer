/**
 * LyricsProcessor Service
 * 歌词服务 - 歌词获取与处理
 */

// @ts-ignore
import axios from "@/utils/request.js";
// @ts-ignore
import { parseLrc, parseQrc, parseYrc, parseTTML, LyricLine } from "@applemusic-like-lyrics/lyric";
import { preprocessLyrics } from './processor';
import { parseLrcToEntries } from './entryParser';
import { detectYrcType } from './timeUtils';

// Re-define LyricData interface based on parseLyric.ts
interface LyricData {
  romaji: string;
  translation: string;
  lrc?: { lyric: string } | null;
  tlyric?: { lyric: string } | null;
  romalrc?: { lyric: string } | null;
  yrc?: { lyric: string } | null;
  ytlrc?: { lyric: string } | null;
  yromalrc?: { lyric: string } | null;
  code?: number;
  // 添加TTML相关字段，用于传递TTML数据
  hasTTML?: boolean;
  ttml?: any;
  // 添加处理后的缓存字段
  processedLyrics?: any;
  // 添加歌词元数据字段
  meta?: LyricMeta;
}

// Interface for the raw response from Netease /lyric/new endpoint (assumed structure)
interface NeteaseRawLyricResponse extends LyricData {
  // Potentially other fields like klyric, etc.
}

// Updated interface for the *actual* Lyric Atlas API response structure based on logs
interface LyricAtlasDirectResponse {
  found: boolean;
  id: string; // API returns string ID
  format?: 'lrc' | 'qrc' | 'ttml' | string;
  source?: string;
  content?: string; // Raw lyric string
  translation?: string; // 翻译歌词内容 (新版LAAPI)
  romaji?: string; // 音译歌词内容 (新版LAAPI)
  // API might return other fields, add if necessary
}

// 新增: 定义歌词元数据接口
interface LyricMeta {
  found: boolean;
  id: string;
  availableFormats?: string[]; // 如 ["yrc", "eslrc", "lrc", "ttml"]
  hasTranslation?: boolean;
  hasRomaji?: boolean;
  foundNCM?: boolean;
  source?: string; // 添加歌词来源字段
}

// 设置选项接口
interface LyricProcessOptions {
  showYrc: boolean;
  showRoma: boolean;
  showTransl: boolean;
}

// TTML格式歌词的接口声明
interface TTMLLyric {
  lines: LyricLine[];
  metadata: [string, string[]][];
}

// Define the Lyric Provider interface - now returns LyricData
interface LyricProvider {
  getLyric(id: number): Promise<LyricData | null>;
  // 新增: 获取歌词元数据信息的方法
  checkLyricMeta?(id: number): Promise<LyricMeta | null>;
}

// Implementation for the Netease API - Return raw data matching LyricData format
class NeteaseLyricProvider implements LyricProvider {
  async getLyric(id: number): Promise<LyricData | null> {
    try {
      const response: NeteaseRawLyricResponse = await axios({
        method: "GET",
        hiddenBar: true,
        url: "/lyric/new",
        params: { id },
      });

      // Ensure the response has a code, default to 200 if missing but data exists
      if (response && (response.lrc || response.tlyric || response.yrc)) {
          if (typeof response.code === 'undefined') {
              response.code = 200;
          }
      } else if (!response || response.code !== 200) {
          console.warn("Netease lyric response indicates failure or no data:", response);
          return null; // Return null if code is not 200 or data is missing
      }

      return response;
    } catch (error) {
      console.error("Failed to fetch lyrics from Netease:", error);
      return null;
    }
  }
}

// Implementation for the Lyric-Atlas API - ADJUSTED FOR ACTUAL RESPONSE
class LyricAtlasProvider implements LyricProvider {
  async getLyric(id: number): Promise<LyricData | null> {
    try {
      // 首先尝试获取元数据，检查是否有歌词和可用的格式
      const meta = await this.checkLyricMeta(id);

      // 如果未找到歌词，直接返回null
      if (!meta || !meta.found) {
        console.warn(`[LyricAtlasProvider] No lyrics found for id: ${id} based on meta check`);
        return null;
      }

      // Expecting the direct response structure now
      const response: LyricAtlasDirectResponse = await axios({
        method: 'GET',
        hiddenBar: true,
        url: `/api/la/api/search`,
        params: { id }, // Keep numeric ID for request
      });

      // Log the raw response from Lyric Atlas API
      console.log(`[LyricAtlasProvider] Raw API response for id ${id}:`, JSON.stringify(response));

      // Check the actual response structure
      if (!response || !response.found || !response.content || !response.format) {
        console.warn(`No valid lyric content found in Lyric Atlas direct response for id: ${id}`, response);
        return null;
      }

      console.log(`[LyricAtlasProvider] Received direct response for ${id}: format=${response.format}, source=${response.source}`);

      const result: LyricData = {
        code: 200, // Assume success if found=true and content exists
        lrc: { lyric: "[00:00.00]加载歌词中...\n[99:99.99]" }, // 默认提供一个占位lrc，确保UI不会出错
        tlyric: null,
        romalrc: null,
        yrc: null,
        ytlrc: null,
        yromalrc: null,
        hasTTML: false, // 默认不是TTML格式
        ttml: null, // 默认无TTML数据
        romaji: "",
        translation: "",
        // 添加元数据信息
        meta: meta
      };

      // 处理翻译歌词 (新版LAAPI)
      if (response.translation) {
        console.log(`[LyricAtlasProvider] Found translation lyrics for id: ${id}`);
        // 将字符串版本的translation转换为对象格式，以匹配现有接口
        result.tlyric = { lyric: response.translation };
        // 同时保留原始字符串格式，以便processLyrics可以处理
        result.translation = response.translation;
      }

      // 处理音译歌词 (新版LAAPI)
      if (response.romaji) {
        console.log(`[LyricAtlasProvider] Found romaji lyrics for id: ${id}`);
        // 将字符串版本的romaji转换为对象格式，以匹配现有接口
        result.romalrc = { lyric: response.romaji };
        // 同时保留原始字符串格式，以便processLyrics可以处理
        result.romaji = response.romaji;
      }

      // Map content based on format
      if (response.format === 'lrc') {
        // 对于LRC格式，直接使用内容
        result.lrc = { lyric: response.content };
      } else if (response.format === 'qrc' || response.format === 'yrc') {
        // 将qrc或yrc格式映射到yrc字段
        result.yrc = { lyric: response.content }; // Map qrc/yrc to yrc

        // 从qrc/yrc解析并创建lrc格式
        try {
          // 根据格式选择正确的解析器
          let parsedLyric: any[];

          // 如果接口已明确返回格式，优先使用返回的格式
          if (response.format === 'qrc') {
            // 使用QRC解析器
            parsedLyric = parseQrc(response.content);
            console.log(`[LyricAtlasProvider] Using QRC parser for id: ${id}`);
          } else if (response.format === 'yrc') {
            // 使用YRC解析器
            parsedLyric = parseYrc(response.content);
            console.log(`[LyricAtlasProvider] Using YRC parser for id: ${id}`);
          } else {
            // 尝试通过内容检测格式
            const contentType = detectYrcType(response.content);
            if (contentType === 'yrc') {
              parsedLyric = parseYrc(response.content);
              console.log(`[LyricAtlasProvider] Detected YRC format for id: ${id}`);
            } else {
              parsedLyric = parseQrc(response.content);
              console.log(`[LyricAtlasProvider] Detected QRC format for id: ${id}`);
            }
          }

          if (parsedLyric && parsedLyric.length > 0) {
            // 创建LRC文本
            let lrcText = '';
            parsedLyric.forEach(line => {
              if (line.words && line.words.length > 0) {
                const timeMs = line.words[0].startTime;
                const minutes = Math.floor(timeMs / 60000);
                const seconds = ((timeMs % 60000) / 1000).toFixed(2);
                const timeStr = `${minutes.toString().padStart(2, '0')}:${seconds.padStart(5, '0')}`;
                const content = line.words.map(w => w.word).join('');
                lrcText += `[${timeStr}]${content}\n`;
              }
            });

            // 如果生成的lrc文本为空，回退到默认值
            if (!lrcText.trim()) {
              lrcText = "[00:00.00]无法生成歌词\n[99:99.99]";
            }

            result.lrc = { lyric: lrcText };
            console.log(`[LyricAtlasProvider] Successfully created LRC from ${response.format} for id: ${id}`);

            // 对于YRC/QRC格式，直接处理翻译和音译数据
            if (response.translation || response.romaji) {
              console.log(`[LyricAtlasProvider] 为YRC/QRC格式预处理翻译和音译数据`);

              // 收集有效歌词行（有内容的行）
              const validLineIndices: number[] = [];
              parsedLyric.forEach((line, idx) => {
                if (line.words && line.words.length > 0) {
                  const text = line.words.map(w => w.word).join('').trim();
                  // 过滤间奏行（只有符号的行）
                  const stripped = text.replace(/[\s♪♩♫♬🎵🎶🎼·…\-_—─]/g, '');
                  if (stripped.length > 0) {
                    validLineIndices.push(idx);
                  }
                }
              });

              // 预处理翻译 - 使用行索引匹配
              if (response.translation && validLineIndices.length > 0) {
                try {
                  const transEntries = parseLrcToEntries(response.translation);
                  // 按索引一一对应
                  const matchCount = Math.min(transEntries.length, validLineIndices.length);
                  for (let i = 0; i < matchCount; i++) {
                    const lineIdx = validLineIndices[i];
                    parsedLyric[lineIdx].translatedLyric = transEntries[i].text;
                  }
                  console.log(`[LyricAtlasProvider] 成功为YRC/QRC预处理翻译数据: ${matchCount} 行`);
                } catch (error) {
                  console.error(`[LyricAtlasProvider] 预处理YRC/QRC翻译数据出错:`, error);
                }
              }

              // 预处理音译 - 使用行索引匹配
              if (response.romaji && validLineIndices.length > 0) {
                try {
                  const romaEntries = parseLrcToEntries(response.romaji);
                  // 按索引一一对应
                  const matchCount = Math.min(romaEntries.length, validLineIndices.length);
                  for (let i = 0; i < matchCount; i++) {
                    const lineIdx = validLineIndices[i];
                    parsedLyric[lineIdx].romanLyric = romaEntries[i].text;
                  }
                  console.log(`[LyricAtlasProvider] 成功为YRC/QRC预处理音译数据: ${matchCount} 行`);
                } catch (error) {
                  console.error(`[LyricAtlasProvider] 预处理YRC/QRC音译数据出错:`, error);
                }
              }

              console.log(`[LyricAtlasProvider] YRC/QRC预处理完成，已包含翻译和音译数据`);
            }
          } else {
            // 解析结果为空，使用默认lrc
            result.lrc = { lyric: "[00:00.00]无法解析歌词内容\n[99:99.99]" };
            console.warn(`[LyricAtlasProvider] ${response.format} parsing resulted in empty lines for id: ${id}`);
          }
        } catch (error) {
          console.warn(`[LyricAtlasProvider] Could not extract LRC from ${response.format} for id: ${id}:`, error);
          // 如果无法提取，创建一个占位LRC，确保UI不会出错
          result.lrc = { lyric: "[00:00.00]解析歌词时出错\n[99:99.99]" };
        }
      } else if (response.format === 'ttml') {
        // 处理 TTML 格式
        try {
          const ttmlLyric = parseTTML(response.content) as TTMLLyric;

          // 标记拥有TTML格式歌词
          result.hasTTML = true;
          // 存储解析后的TTML数据
          result.ttml = ttmlLyric.lines;

          // 为TTML准备数据
          if (ttmlLyric && ttmlLyric.lines && ttmlLyric.lines.length > 0) {
            // 对于TTML格式，直接处理翻译和音译数据
            if (response.translation || response.romaji) {
              console.log(`[LyricAtlasProvider] 为TTML格式预处理翻译和音译数据`);

              // 收集有效歌词行（有内容的行）
              const validLineIndices: number[] = [];
              ttmlLyric.lines.forEach((line, idx) => {
                if (line.words && line.words.length > 0) {
                  const text = line.words.map(w => w.word).join('').trim();
                  // 过滤间奏行（只有符号的行）
                  const stripped = text.replace(/[\s♪♩♫♬🎵🎶🎼·…\-_—─]/g, '');
                  if (stripped.length > 0) {
                    validLineIndices.push(idx);
                  }
                }
              });

              // 预处理翻译 - 使用行索引匹配
              if (response.translation && validLineIndices.length > 0) {
                try {
                  const transEntries = parseLrcToEntries(response.translation);
                  // 按索引一一对应
                  const matchCount = Math.min(transEntries.length, validLineIndices.length);
                  for (let i = 0; i < matchCount; i++) {
                    const lineIdx = validLineIndices[i];
                    ttmlLyric.lines[lineIdx].translatedLyric = transEntries[i].text;
                  }
                  console.log(`[LyricAtlasProvider] 成功为TTML预处理翻译数据: ${matchCount} 行`);
                } catch (error) {
                  console.error(`[LyricAtlasProvider] 预处理TTML翻译数据出错:`, error);
                }
              }

              // 预处理音译 - 使用行索引匹配
              if (response.romaji && validLineIndices.length > 0) {
                try {
                  const romaEntries = parseLrcToEntries(response.romaji);
                  // 按索引一一对应
                  const matchCount = Math.min(romaEntries.length, validLineIndices.length);
                  for (let i = 0; i < matchCount; i++) {
                    const lineIdx = validLineIndices[i];
                    ttmlLyric.lines[lineIdx].romanLyric = romaEntries[i].text;
                  }
                  console.log(`[LyricAtlasProvider] 成功为TTML预处理音译数据: ${matchCount} 行`);
                } catch (error) {
                  console.error(`[LyricAtlasProvider] 预处理TTML音译数据出错:`, error);
                }
              }
            }

            // 创建一个包含特殊标记的字符串，表示这是已解析的LyricLine[]
            const serializedYrc = `___PARSED_LYRIC_LINES___${JSON.stringify(ttmlLyric.lines)}`;
            result.yrc = { lyric: serializedYrc };
            console.log(`[LyricAtlasProvider] Successfully parsed TTML for id: ${id}, lines: ${ttmlLyric.lines.length}`);

            // 同时创建LRC格式的歌词，确保lrc数组有内容
            let lrcText = '';
            ttmlLyric.lines.forEach(line => {
              if (line.words && line.words.length > 0) {
                const timeMs = line.words[0].startTime;
                const minutes = Math.floor(timeMs / 60000);
                const seconds = ((timeMs % 60000) / 1000).toFixed(2);
                const timeStr = `${minutes.toString().padStart(2, '0')}:${seconds.padStart(5, '0')}`;
                const content = line.words.map(w => w.word).join('');
                lrcText += `[${timeStr}]${content}\n`;
              }
            });

            // 如果生成的lrc文本为空，回退到默认值
            if (!lrcText.trim()) {
              lrcText = "[00:00.00]无法生成歌词\n[99:99.99]";
            }

            result.lrc = { lyric: lrcText };
            console.log(`[LyricAtlasProvider] Created compatible LRC format from TTML for id: ${id}`);
          } else {
            console.warn(`[LyricAtlasProvider] TTML parsing resulted in empty lines for id: ${id}`);
            result.lrc = { lyric: "[00:00.00]TTML解析结果为空\n[99:99.99]" };
            result.hasTTML = false; // 解析结果为空，置回false
            result.ttml = null;
          }
        } catch (error) {
          console.error(`[LyricAtlasProvider] Error parsing TTML for id: ${id}:`, error);
          result.lrc = { lyric: "[00:00.00]TTML解析出错\n[99:99.99]" };
          result.hasTTML = false; // 解析出错，置回false
          result.ttml = null;
        }
      } else {
        // 处理未知格式
        console.warn(`[LyricAtlasProvider] Trying to handle unknown format '${response.format}' for id: ${id}`);

        // 检查内容是否看起来像LRC
        if (typeof response.content === 'string' && response.content.includes('[') && response.content.includes(']')) {
          // 尝试作为LRC格式处理
          try {
            result.lrc = { lyric: response.content };
            console.log(`[LyricAtlasProvider] Content looks like LRC, using as-is for id: ${id}`);
          } catch (e) {
            console.error(`[LyricAtlasProvider] Error treating content as LRC:`, e);
            result.lrc = { lyric: `[00:00.00]解析${response.format}格式失败\n[99:99.99]` };
          }
        } else {
          // 尝试从纯文本提取内容生成简单LRC
          try {
            let lines = response.content.split(/\r?\n/);
            let lrcText = '';

            // 为每行添加时间标记，简单地按顺序分配时间
            lines.forEach((line, index) => {
              if (line.trim()) {
                const minutes = Math.floor(index / 6); // 每行大约10秒
                const seconds = (index % 6) * 10;
                const timeStr = `${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}.00`;
                lrcText += `[${timeStr}]${line.trim()}\n`;
              }
            });

            if (lrcText.trim()) {
              result.lrc = { lyric: lrcText };
              console.log(`[LyricAtlasProvider] Created simple LRC from text content for id: ${id}`);
            } else {
              result.lrc = { lyric: `[00:00.00]未能从${response.format}提取文本\n[99:99.99]` };
            }
          } catch (e) {
            console.error(`[LyricAtlasProvider] Error extracting text from content:`, e);
            result.lrc = { lyric: `[00:00.00]不支持的歌词格式: ${response.format}\n[99:99.99]` };
          }
        }
      }

      return result;

    } catch (error) {
      console.error("Failed to fetch or process lyrics from Lyric Atlas:", error);
      return null;
    }
  }

  /**
   * 检查歌词元数据，例如支持的格式和翻译/音译的可用性
   * @param id 歌曲ID
   * @returns 包含元数据信息的LyricMeta对象，如果请求失败则返回null
   */
  async checkLyricMeta(id: number): Promise<LyricMeta | null> {
    try {
      // 使用新的元数据API端点
      const response = await axios({
        method: 'GET',
        hiddenBar: true,
        url: `/api/la/api/lyrics/meta`,
        params: { id }, // 使用歌曲ID作为参数
      });

      // 检查响应是否有效
      if (!response || response.found === undefined) {
        console.warn(`[LyricAtlasProvider] Invalid meta response for id: ${id}`, response);
        return null;
      }

      // 提取并返回元数据
      const meta: LyricMeta = {
        found: response.found,
        id: response.id,
        availableFormats: response.availableFormats || [],
        hasTranslation: response.hasTranslation || false,
        hasRomaji: response.hasRomaji || false,
        source: response.source
      };

      console.log(`[LyricAtlasProvider] Lyric meta for id ${id}:`, meta);
      return meta;
    } catch (error) {
      console.error(`[LyricAtlasProvider] Failed to fetch lyric meta for id ${id}:`, error);
      return null;
    }
  }
}

// Lyric Service Factory - fetchLyric now returns Promise<LyricData | null>
export class LyricService {
  private provider: LyricProvider;
  private defaultProcessOptions: LyricProcessOptions = {
    showYrc: true,
    showRoma: false,
    showTransl: false
  };
  // 添加NCM提供者实例，用于回退
  private ncmProvider: NeteaseLyricProvider;
  // 添加Lyric Atlas提供者实例，用于元数据检查
  private laProvider: LyricAtlasProvider | null = null;

  constructor(useLyricAtlas: boolean = false) {
    // 始终初始化网易云提供者，用于回退
    this.ncmProvider = new NeteaseLyricProvider();

    // The presence of the provider is now controlled by the setting alone.
    if (useLyricAtlas) {
      console.log("Using Lyric Atlas provider.");
      this.laProvider = new LyricAtlasProvider();
      this.provider = this.laProvider;
    } else {
      console.log("Using Netease lyric provider.");
      this.provider = this.ncmProvider;
    }
  }

  /**
   * 设置默认的歌词处理选项
   * @param options 歌词处理选项
   */
  setDefaultProcessOptions(options: LyricProcessOptions): void {
    this.defaultProcessOptions = { ...this.defaultProcessOptions, ...options };
  }

  /**
   * 获取歌词并进行处理
   * @param id 歌曲ID
   * @param processOptions 歌词处理选项，可选，不提供则使用默认选项
   */
  async fetchLyric(id: number, processOptions?: LyricProcessOptions): Promise<LyricData | null> {
    try {
      const startTime = performance.now();
      console.time(`[LyricService] 获取并处理歌词 ${id}`);

      let result: LyricData | null = null;

      if (this.laProvider) {
        const meta = await this.laProvider.checkLyricMeta(id);

        if (meta && meta.found) {
          console.log(`[LyricService] 元数据检查成功，使用Lyric Atlas获取歌词，ID: ${id}`);
          result = await this.laProvider.getLyric(id); // This should already have meta
        } else {
          console.log(`[LyricService] Lyric Atlas没有歌词数据，回退到网易云API，ID: ${id}`);
          result = await this.ncmProvider.getLyric(id);
          if (result && meta) { // If NCM gave lyrics, and we had LA meta initially (though found=false)
            result.meta = { ...meta, foundNCM: true }; // Augment meta
          } else if (result && !meta && this.laProvider) {
            // If NCM gave lyrics and we never had LA meta, try to get LA meta just for source info etc.
            const freshMeta = await this.laProvider.checkLyricMeta(id);
            if (freshMeta) result.meta = freshMeta;
          }
        }
      } else {
        console.log(`[LyricService] 使用默认提供者获取歌词，ID: ${id}`);
        result = await this.provider.getLyric(id);
      }

      if (result) {
        if (result.code === undefined) {
          result.code = 200;
        }

        if (result.lrc?.lyric) {
          console.log(`[LyricService] 处理歌词同步，id: ${id}`);
          const mainTimeMap = new Map<number, {time: string, content: string, rawLine: string}>();
          const mainLrcLines = result.lrc.lyric.split('\n').filter(line => line.trim());
          const timeRegex = /\[(\d{2}):(\d{2})\.(\d{2})\]/;

          for (const line of mainLrcLines) {
            const match = line.match(timeRegex);
            if (match) {
              const min = parseInt(match[1]);
              const sec = parseInt(match[2]);
              const ms = parseInt(match[3]);
              const timeMs = min * 60000 + sec * 1000 + ms * 10;
              const timeStr = `${match[1]}:${match[2]}.${match[3]}`;
              const content = line.replace(timeRegex, '').trim();
              if (content) {
                mainTimeMap.set(timeMs, {time: timeStr, content, rawLine: line});
              }
            }
          }

          // 条件性跳过 syncLyricTimestamps
          const skipTimestampSyncLrc = result.meta?.source === 'repository'; // Main lyric source
          // For roma, we also check if the specific romaji lyric source (if distinguishable) is repository grade
          // Assuming for now that if meta.source is repository, associated roma is also repository grade.
          const skipTimestampSyncRoma = result.meta?.source === 'repository';

          if (result.tlyric?.lyric) {
            // Translation sync logic doesn't change based on roma source being repository
            // unless we have a specific meta flag for tlyric source.
            console.log(`[LyricService] 对翻译歌词进行时间戳同步，id: ${id}`);
            result.tlyric.lyric = this.syncLyricTimestamps(
              result.tlyric.lyric,
              mainTimeMap,
              "翻译歌词",
              id
            );
          } else {
            console.log(`[LyricService] 没有发现翻译歌词，id: ${id}`);
          }

          if (result.romalrc?.lyric) {
            if (skipTimestampSyncRoma) {
              console.log(`[LyricService] 检测到音译来源 (romalrc) 为 repository，跳过时间戳同步，id: ${id}`);
            } else {
              console.log(`[LyricService] 对音译歌词进行时间戳同步，id: ${id}`);
              result.romalrc.lyric = this.syncLyricTimestamps(
                result.romalrc.lyric,
                mainTimeMap,
                "音译歌词",
                id
              );
            }
          } else {
            console.log(`[LyricService] 没有发现音译歌词，id: ${id}`);
          }

          // 如果没有lrc但有yrc，确保我们能从yrc中创建一个基本的lrc
          if ((!result.lrc || !result.lrc.lyric) && result.yrc && result.yrc.lyric) {
            console.log(`[LyricService] No LRC found for id ${id}, attempting to generate from YRC`);

            try {
              // 判断内容是否是yrc或qrc格式，并选择对应的解析器
              let parsedLyric;

              // 使用内容检测歌词类型
              const content = result.yrc.lyric;
              const contentType = detectYrcType(content);

              if (contentType === 'yrc') {
                // 使用YRC解析器
                parsedLyric = parseYrc(content);
                console.log(`[LyricService] Using YRC parser for id: ${id}`);
              } else {
                // 使用QRC解析器
                parsedLyric = parseQrc(content);
                console.log(`[LyricService] Using QRC parser for id: ${id}`);
              }

              if (parsedLyric && parsedLyric.length > 0) {
                let lrcText = '';
                parsedLyric.forEach(line => {
                  if (line.words && line.words.length > 0) {
                    const timeMs = line.words[0].startTime;
                    const minutes = Math.floor(timeMs / 60000);
                    const seconds = ((timeMs % 60000) / 1000).toFixed(2);
                    const timeStr = `${minutes.toString().padStart(2, '0')}:${seconds.padStart(5, '0')}`;
                    const content = line.words.map(w => w.word).join('');
                    lrcText += `[${timeStr}]${content}\n`;
                  }
                });

                // 如果成功创建了lrc文本，使用它
                if (lrcText.trim()) {
                  result.lrc = { lyric: lrcText };
                  console.log(`[LyricService] Successfully generated LRC from ${contentType} for id ${id}`);
                } else {
                  // 如果无法创建有效内容，使用占位符
                  result.lrc = { lyric: "[00:00.00]无法从歌词生成LRC\n[99:99.99]" };
                  console.warn(`[LyricService] Failed to generate meaningful LRC from ${contentType} for id ${id}`);
                }
              } else {
                // 解析YRC/QRC失败，使用占位符
                result.lrc = { lyric: "[00:00.00]无法解析歌词\n[99:99.99]" };
                console.warn(`[LyricService] Failed to parse ${contentType} for id ${id}`);
              }
            } catch (error) {
              // 出现异常，使用占位符
              result.lrc = { lyric: "[00:00.00]处理歌词时出错\n[99:99.99]" };
              console.error(`[LyricService] Error generating LRC from YRC/QRC for id ${id}:`, error);
            }
          }

          // 如果没有lrc也没有yrc，使用占位符
          if ((!result.lrc || !result.lrc.lyric) && (!result.yrc || !result.yrc.lyric)) {
            console.warn(`[LyricService] No lyric data (neither LRC nor YRC) found for id ${id}, using placeholder`);
            result.lrc = { lyric: "[00:00.00]暂无歌词\n[99:99.99]" };
          }
        }

        // 设置歌词处理选项，优先使用传入的选项，否则使用默认选项
        const options = processOptions || this.defaultProcessOptions;

        // 预处理歌词数据，提前生成缓存以提高性能
        console.time('[LyricService] 预处理歌词');
        try {
          // 这里我们调用改进后的预处理函数，将处理结果缓存到歌词对象中
          preprocessLyrics(result, options);
          console.log(`[LyricService] 歌曲ID ${id} 歌词预处理成功`);
        } catch (err) {
          console.warn(`[LyricService] 歌曲ID ${id} 歌词预处理失败:`, err);
        }
        console.timeEnd('[LyricService] 预处理歌词');
      }

      const endTime = performance.now();
      console.timeEnd(`[LyricService] 获取并处理歌词 ${id}`);
      console.log(`[LyricService] 歌词处理总耗时: ${(endTime - startTime).toFixed(2)}ms`);

      return result;
    } catch (error) {
      console.error(`[LyricService] Failed to fetch lyric for id ${id}:`, error);
      return null;
    }
  }

  /**
   * 同步歌词时间戳，使辅助歌词(翻译、音译等)的时间戳与主歌词一致
   * @param lyricText 要同步的歌词文本
   * @param mainTimeMap 主歌词时间映射
   * @param lyricType 歌词类型描述(用于日志)
   * @param songId 歌曲ID(用于日志)
   * @returns 同步后的歌词文本
   */
  private syncLyricTimestamps(
    lyricText: string,
    mainTimeMap: Map<number, {time: string, content: string, rawLine: string}>,
    lyricType: string,
    songId: number
  ): string {
    if (!lyricText || !mainTimeMap.size) return lyricText;

    console.log(`[LyricService] 开始同步${lyricType}，歌曲ID: ${songId}`);

    const timeRegex = /\[(\d{2}):(\d{2})\.(\d{2})\]/;
    const lines = lyricText.split('\n').filter(line => line.trim());
    const mainTimestamps = Array.from(mainTimeMap.keys()).sort((a, b) => a - b);

    // 构建辅助歌词的时间和内容数组
    const auxLyrics: {timeMs: number, timeStr: string, content: string}[] = [];

    for (const line of lines) {
      const match = line.match(timeRegex);
      if (match) {
        const min = parseInt(match[1]);
        const sec = parseInt(match[2]);
        const ms = parseInt(match[3]);
        const timeMs = min * 60000 + sec * 1000 + ms * 10;
        const timeStr = `${match[1]}:${match[2]}.${match[3]}`;

        const content = line.replace(timeRegex, '').trim();
        if (content) {
          auxLyrics.push({timeMs, timeStr, content});
        }
      }
    }

    // 按时间排序
    auxLyrics.sort((a, b) => a.timeMs - b.timeMs);

    // 如果辅助歌词数量和主歌词不同，使用智能匹配
    let newLyricText = '';

    if (auxLyrics.length === mainTimestamps.length) {
      // 数量相同，直接一一对应同步
      console.log(`[LyricService] ${lyricType}行数与主歌词匹配(${auxLyrics.length}行)，执行直接同步`);
      for (let i = 0; i < auxLyrics.length; i++) {
        const mainTime = mainTimeMap.get(mainTimestamps[i])?.time || "00:00.00";
        newLyricText += `[${mainTime}]${auxLyrics[i].content}\n`;
      }
    } else {
      // 数量不同，使用时间最接近原则匹配
      console.log(`[LyricService] ${lyricType}行数与主歌词不匹配(主: ${mainTimestamps.length}行, 辅: ${auxLyrics.length}行)，执行智能匹配`);

      // 为每行辅助歌词找到时间上最接近的主歌词行
      for (const auxLyric of auxLyrics) {
        // 找出时间上最接近的主歌词时间戳
        let closestMainTime = mainTimestamps[0];
        let minTimeDiff = Math.abs(auxLyric.timeMs - closestMainTime);

        for (const mainTime of mainTimestamps) {
          const timeDiff = Math.abs(auxLyric.timeMs - mainTime);
          if (timeDiff < minTimeDiff) {
            minTimeDiff = timeDiff;
            closestMainTime = mainTime;
          }
        }

        // 使用找到的主歌词时间戳
        const mainTime = mainTimeMap.get(closestMainTime)?.time || "00:00.00";
        newLyricText += `[${mainTime}]${auxLyric.content}\n`;
      }

      // 确保所有辅助歌词都有对应的主歌词时间
      if (auxLyrics.length < mainTimestamps.length) {
        console.log(`[LyricService] ${lyricType}行数少于主歌词，已进行最佳匹配`);
      } else {
        console.log(`[LyricService] ${lyricType}行数多于主歌词，已尝试去重和合并`);
        // 可能有多行辅助歌词对应同一个时间戳，这里已经通过最接近原则处理了
      }
    }

    console.log(`[LyricService] ${lyricType}同步完成，原行数: ${auxLyrics.length}，同步后行数: ${newLyricText.split('\n').filter(l => l.trim()).length}`);

    return newLyricText;
  }

  /**
   * 检查歌词元数据
   * @param id 歌曲ID
   * @returns 歌词元数据信息，若不支持或出错则返回null
   */
  async checkLyricMeta(id: number): Promise<LyricMeta | null> {
    // 检查provider是否支持元数据检查
    if (this.provider.checkLyricMeta) {
      try {
        return await this.provider.checkLyricMeta(id);
      } catch (error) {
        console.error(`[LyricService] Error checking lyric meta for id ${id}:`, error);
        return null;
      }
    } else {
      console.warn(`[LyricService] Current provider doesn't support lyric meta check`);
      return null;
    }
  }
}

export default LyricService;
