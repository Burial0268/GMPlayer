/**
 * LyricsProcessor Service
 * 歌词服务 - 歌词获取与处理
 */

import request from "@/utils/request";
// @ts-ignore
import { parseLrc, parseQrc, parseYrc, LyricLine } from "@applemusic-like-lyrics/lyric";
import { ensureTTMLLoaded, parseTTML } from "./parseTTML";
import { preprocessLyrics } from "./processor";
import { detectYrcType } from "./timeUtils";

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
  // 添加AMLL格式数据字段（由preprocessLyrics填充）
  lrcAMData?: any[];
  yrcAMData?: any[];
  settingsHash?: string;
}

// Interface for the raw response from Netease /lyric/new endpoint (assumed structure)
interface NeteaseRawLyricResponse extends LyricData {
  // Potentially other fields like klyric, etc.
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
  getLyric(id: number, fast?: boolean): Promise<LyricData | null>;
  // 新增: 获取歌词元数据信息的方法
  checkLyricMeta?(id: number, fast?: boolean): Promise<LyricMeta | null>;
}

// Implementation for the Netease API - Return raw data matching LyricData format
class NeteaseLyricProvider implements LyricProvider {
  async getLyric(id: number, _fast?: boolean): Promise<LyricData | null> {
    try {
      const response: NeteaseRawLyricResponse = await request({
        method: "GET",
        hiddenBar: true,
        url: "/lyric/new",
        params: { id },
      });

      // Ensure the response has a code, default to 200 if missing but data exists
      if (response && (response.lrc || response.tlyric || response.yrc)) {
        if (typeof response.code === "undefined") {
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

// TTML mirror URLs for fetching high-quality word-by-word lyrics from the AMLL TTML DB
const TTML_MIRROR_URLS = [
  "https://amll-ttml-db.gbclstudio.cn/ncm-lyrics/{id}.ttml",
  "https://amlldb.bikonoo.com/ncm-lyrics/{id}.ttml",
  "https://raw.githubusercontent.com/amll-dev/amll-ttml-db/refs/heads/main/ncm-lyrics/{id}.ttml",
  "https://amll.mirror.dimeta.top/api/db/ncm-lyrics/{id}.ttml",
];

// Implementation for TTML Repository - fetches .ttml files directly from GitHub repo mirrors
class TTMLRepoProvider implements LyricProvider {
  /**
   * Try each mirror URL sequentially until one returns a valid TTML file.
   * @param id Song ID
   * @returns TTML text content or null if all mirrors fail
   */
  private async fetchTTMLFromMirrors(id: number): Promise<string | null> {
    for (const urlTemplate of TTML_MIRROR_URLS) {
      const url = urlTemplate.replace("{id}", String(id));
      try {
        const controller = new AbortController();
        const timeoutId = setTimeout(() => controller.abort(), 8000);

        const response = await fetch(url, { signal: controller.signal });
        clearTimeout(timeoutId);

        if (response.ok) {
          const text = await response.text();
          if (text && text.includes("<tt")) {
            console.log(`[TTMLRepoProvider] Successfully fetched TTML for id ${id} from ${url}`);
            return text;
          }
        }
        console.log(`[TTMLRepoProvider] Mirror returned ${response.status} for id ${id}: ${url}`);
      } catch (error: any) {
        if (error.name === "AbortError") {
          console.log(`[TTMLRepoProvider] Timeout fetching id ${id} from ${url}`);
        } else {
          console.log(`[TTMLRepoProvider] Error fetching id ${id} from ${url}:`, error.message);
        }
      }
    }
    return null;
  }

  async getLyric(id: number, _fast?: boolean): Promise<LyricData | null> {
    try {
      const ttmlContent = await this.fetchTTMLFromMirrors(id);

      if (!ttmlContent) {
        console.log(`[TTMLRepoProvider] No TTML found for id ${id} in any mirror`);
        return null;
      }

      // Parse TTML using existing WASM parser
      await ensureTTMLLoaded();
      const ttmlLyric = parseTTML(ttmlContent) as TTMLLyric;

      if (!ttmlLyric || !ttmlLyric.lines || ttmlLyric.lines.length === 0) {
        console.warn(`[TTMLRepoProvider] TTML parsing resulted in empty lines for id: ${id}`);
        return null;
      }

      console.log(`[TTMLRepoProvider] Parsed TTML for id ${id}: ${ttmlLyric.lines.length} lines`);

      // Build serialized YRC from parsed TTML lines
      const serializedYrc = `___PARSED_LYRIC_LINES___${JSON.stringify(ttmlLyric.lines)}`;

      // Build fallback LRC text from TTML lines
      let lrcText = "";
      ttmlLyric.lines.forEach((line) => {
        if (line.words && line.words.length > 0) {
          const timeMs = line.words[0].startTime;
          const minutes = Math.floor(timeMs / 60000);
          const seconds = ((timeMs % 60000) / 1000).toFixed(2);
          const timeStr = `${minutes.toString().padStart(2, "0")}:${seconds.padStart(5, "0")}`;
          const content = line.words.map((w: any) => w.word).join("");
          lrcText += `[${timeStr}]${content}\n`;
        }
      });

      if (!lrcText.trim()) {
        lrcText = "[00:00.00]无法生成歌词\n[99:99.99]";
      }

      const result: LyricData = {
        code: 200,
        lrc: { lyric: lrcText },
        tlyric: null,
        romalrc: null,
        yrc: { lyric: serializedYrc },
        ytlrc: null,
        yromalrc: null,
        hasTTML: true,
        ttml: ttmlLyric.lines,
        romaji: "",
        translation: "",
        meta: {
          found: true,
          id: String(id),
          source: "repository",
        },
      };

      return result;
    } catch (error) {
      console.error(`[TTMLRepoProvider] Error processing TTML for id ${id}:`, error);
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
    showTransl: false,
  };
  // 添加NCM提供者实例，用于回退
  private ncmProvider: NeteaseLyricProvider;
  // TTML仓库提供者实例
  private ttmlProvider: TTMLRepoProvider | null = null;

  constructor(useTTMLRepo: boolean = false) {
    // 始终初始化网易云提供者，用于回退
    this.ncmProvider = new NeteaseLyricProvider();

    if (useTTMLRepo) {
      console.log("Using TTML Repository provider.");
      this.ttmlProvider = new TTMLRepoProvider();
      this.provider = this.ttmlProvider;
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

      if (this.ttmlProvider) {
        // Try TTML repository first, fall back to Netease
        result = await this.ttmlProvider.getLyric(id);
        if (!result) {
          console.log(`[LyricService] TTML仓库无歌词数据，回退到网易云API，ID: ${id}`);
          result = await this.ncmProvider.getLyric(id);
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
          const mainTimeMap = new Map<number, { time: string; content: string; rawLine: string }>();
          const mainLrcLines = result.lrc.lyric.split("\n").filter((line) => line.trim());
          const timeRegex = /\[(\d{2}):(\d{2})\.(\d{2})\]/;

          for (const line of mainLrcLines) {
            const match = line.match(timeRegex);
            if (match) {
              const min = parseInt(match[1]);
              const sec = parseInt(match[2]);
              const ms = parseInt(match[3]);
              const timeMs = min * 60000 + sec * 1000 + ms * 10;
              const timeStr = `${match[1]}:${match[2]}.${match[3]}`;
              const content = line.replace(timeRegex, "").trim();
              if (content) {
                mainTimeMap.set(timeMs, { time: timeStr, content, rawLine: line });
              }
            }
          }

          // 条件性跳过 syncLyricTimestamps
          const skipTimestampSyncLrc = result.meta?.source === "repository"; // Main lyric source
          // For roma, we also check if the specific romaji lyric source (if distinguishable) is repository grade
          // Assuming for now that if meta.source is repository, associated roma is also repository grade.
          const skipTimestampSyncRoma = result.meta?.source === "repository";

          if (result.tlyric?.lyric) {
            // Translation sync logic doesn't change based on roma source being repository
            // unless we have a specific meta flag for tlyric source.
            console.log(`[LyricService] 对翻译歌词进行时间戳同步，id: ${id}`);
            result.tlyric.lyric = this.syncLyricTimestamps(
              result.tlyric.lyric,
              mainTimeMap,
              "翻译歌词",
              id,
            );
          } else {
            console.log(`[LyricService] 没有发现翻译歌词，id: ${id}`);
          }

          if (result.romalrc?.lyric) {
            if (skipTimestampSyncRoma) {
              console.log(
                `[LyricService] 检测到音译来源 (romalrc) 为 repository，跳过时间戳同步，id: ${id}`,
              );
            } else {
              console.log(`[LyricService] 对音译歌词进行时间戳同步，id: ${id}`);
              result.romalrc.lyric = this.syncLyricTimestamps(
                result.romalrc.lyric,
                mainTimeMap,
                "音译歌词",
                id,
              );
            }
          } else {
            console.log(`[LyricService] 没有发现音译歌词，id: ${id}`);
          }

          // 如果没有lrc但有yrc，确保我们能从yrc中创建一个基本的lrc
          if ((!result.lrc || !result.lrc.lyric) && result.yrc && result.yrc.lyric) {
            console.log(
              `[LyricService] No LRC found for id ${id}, attempting to generate from YRC`,
            );

            try {
              // 判断内容是否是yrc或qrc格式，并选择对应的解析器
              let parsedLyric;

              // 使用内容检测歌词类型
              const content = result.yrc.lyric;
              const contentType = detectYrcType(content);

              if (contentType === "yrc") {
                // 使用YRC解析器
                parsedLyric = parseYrc(content);
                console.log(`[LyricService] Using YRC parser for id: ${id}`);
              } else {
                // 使用QRC解析器
                parsedLyric = parseQrc(content);
                console.log(`[LyricService] Using QRC parser for id: ${id}`);
              }

              if (parsedLyric && parsedLyric.length > 0) {
                let lrcText = "";
                parsedLyric.forEach((line) => {
                  if (line.words && line.words.length > 0) {
                    const timeMs = line.words[0].startTime;
                    const minutes = Math.floor(timeMs / 60000);
                    const seconds = ((timeMs % 60000) / 1000).toFixed(2);
                    const timeStr = `${minutes.toString().padStart(2, "0")}:${seconds.padStart(5, "0")}`;
                    const content = line.words.map((w) => w.word).join("");
                    lrcText += `[${timeStr}]${content}\n`;
                  }
                });

                // 如果成功创建了lrc文本，使用它
                if (lrcText.trim()) {
                  result.lrc = { lyric: lrcText };
                  console.log(
                    `[LyricService] Successfully generated LRC from ${contentType} for id ${id}`,
                  );
                } else {
                  // 如果无法创建有效内容，使用占位符
                  result.lrc = { lyric: "[00:00.00]无法从歌词生成LRC\n[99:99.99]" };
                  console.warn(
                    `[LyricService] Failed to generate meaningful LRC from ${contentType} for id ${id}`,
                  );
                }
              } else {
                // 解析YRC/QRC失败，使用占位符
                result.lrc = { lyric: "[00:00.00]无法解析歌词\n[99:99.99]" };
                console.warn(`[LyricService] Failed to parse ${contentType} for id ${id}`);
              }
            } catch (error) {
              // 出现异常，使用占位符
              result.lrc = { lyric: "[00:00.00]处理歌词时出错\n[99:99.99]" };
              console.error(
                `[LyricService] Error generating LRC from YRC/QRC for id ${id}:`,
                error,
              );
            }
          }

          // 如果没有lrc也没有yrc，使用占位符
          if ((!result.lrc || !result.lrc.lyric) && (!result.yrc || !result.yrc.lyric)) {
            console.warn(
              `[LyricService] No lyric data (neither LRC nor YRC) found for id ${id}, using placeholder`,
            );
            result.lrc = { lyric: "[00:00.00]暂无歌词\n[99:99.99]" };
          }
        }

        // 设置歌词处理选项，优先使用传入的选项，否则使用默认选项
        const options = processOptions || this.defaultProcessOptions;

        // 预处理歌词数据，提前生成缓存以提高性能
        console.time("[LyricService] 预处理歌词");
        try {
          // 初始化AMLL数据字段
          result.lrcAMData = result.lrcAMData || [];
          result.yrcAMData = result.yrcAMData || [];
          // 这里我们调用改进后的预处理函数，将处理结果缓存到歌词对象中
          preprocessLyrics(result as any, options);
          console.log(`[LyricService] 歌曲ID ${id} 歌词预处理成功`);
        } catch (err) {
          console.warn(`[LyricService] 歌曲ID ${id} 歌词预处理失败:`, err);
        }
        console.timeEnd("[LyricService] 预处理歌词");
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
    mainTimeMap: Map<number, { time: string; content: string; rawLine: string }>,
    lyricType: string,
    songId: number,
  ): string {
    if (!lyricText || !mainTimeMap.size) return lyricText;

    console.log(`[LyricService] 开始同步${lyricType}，歌曲ID: ${songId}`);

    const timeRegex = /\[(\d{2}):(\d{2})\.(\d{2})\]/;
    const lines = lyricText.split("\n").filter((line) => line.trim());
    const mainTimestamps = Array.from(mainTimeMap.keys()).sort((a, b) => a - b);

    // 构建辅助歌词的时间和内容数组
    const auxLyrics: { timeMs: number; timeStr: string; content: string }[] = [];

    for (const line of lines) {
      const match = line.match(timeRegex);
      if (match) {
        const min = parseInt(match[1]);
        const sec = parseInt(match[2]);
        const ms = parseInt(match[3]);
        const timeMs = min * 60000 + sec * 1000 + ms * 10;
        const timeStr = `${match[1]}:${match[2]}.${match[3]}`;

        const content = line.replace(timeRegex, "").trim();
        if (content) {
          auxLyrics.push({ timeMs, timeStr, content });
        }
      }
    }

    // 按时间排序
    auxLyrics.sort((a, b) => a.timeMs - b.timeMs);

    // 如果辅助歌词数量和主歌词不同，使用智能匹配
    let newLyricText = "";

    if (auxLyrics.length === mainTimestamps.length) {
      // 数量相同，直接一一对应同步
      console.log(
        `[LyricService] ${lyricType}行数与主歌词匹配(${auxLyrics.length}行)，执行直接同步`,
      );
      for (let i = 0; i < auxLyrics.length; i++) {
        const mainTime = mainTimeMap.get(mainTimestamps[i])?.time || "00:00.00";
        newLyricText += `[${mainTime}]${auxLyrics[i].content}\n`;
      }
    } else {
      // 数量不同，使用时间最接近原则匹配
      console.log(
        `[LyricService] ${lyricType}行数与主歌词不匹配(主: ${mainTimestamps.length}行, 辅: ${auxLyrics.length}行)，执行智能匹配`,
      );

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

    console.log(
      `[LyricService] ${lyricType}同步完成，原行数: ${auxLyrics.length}，同步后行数: ${newLyricText.split("\n").filter((l) => l.trim()).length}`,
    );

    return newLyricText;
  }

  /**
   * 检查歌词元数据
   * @param id 歌曲ID
   * @param fast 是否仅获取仓库结果
   * @returns 歌词元数据信息，若不支持或出错则返回null
   */
  async checkLyricMeta(id: number, fast?: boolean): Promise<LyricMeta | null> {
    // 检查provider是否支持元数据检查
    if (this.provider.checkLyricMeta) {
      try {
        return await this.provider.checkLyricMeta(id, fast);
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
