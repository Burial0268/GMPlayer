/**
 * 子序列模糊匹配，带打分。
 *
 * 用于列表内搜索：用户想要的是「打几个字就能捞出来」，而不是精确子串。
 * 例如 `jgl` 能命中「精灵」拼不出来的场合下的 `Jungle`，`海阔` 能命中
 * 《海阔天空》，`bysj` 目前**不能**命中《不由自主》——拼音首字母是另一件事，
 * 见文件末尾的说明。
 *
 * 不引第三方库（fuse.js 之类）是刻意的：这里只需要几十行，而首屏包体是有预算的。
 *
 * CJK 天然可用：匹配以「字符」为单位推进，中文每个字就是一个单位。
 */

/** 词首字符——命中这些位置给额外加分，让「海天」优先于恰好散落的匹配。 */
const isBoundary = (prev: string): boolean =>
  prev === "" ||
  prev === " " ||
  prev === "-" ||
  prev === "_" ||
  prev === "(" ||
  prev === "[" ||
  prev === "/" ||
  prev === "、" ||
  prev === "（";

/**
 * 打分的内核，两边都**已经**小写。
 *
 * 从 `fuzzyScore` 里拆出来，是为了让调用方能把小写化的结果留下来——见
 * `loweredFields`。整表过滤时 `toLowerCase()` 本身就是这条路径上最贵的一步。
 */
const scoreLowered = (t: string, q: string): number | null => {
  // 连续子串是最强信号，直接短路并按「越靠前越好、越短的宿主越好」加权。
  const direct = t.indexOf(q);
  if (direct !== -1) {
    return 1000 - direct * 2 - (t.length - q.length);
  }

  let score = 0;
  let ti = 0;
  let streak = 0;
  for (let qi = 0; qi < q.length; qi++) {
    const ch = q[qi];
    const found = t.indexOf(ch, ti);
    if (found === -1) return null; // 有一个字符接不上就算不匹配
    if (found === ti && qi > 0) {
      // 紧邻上一个命中：连击加分，越长越值钱。
      streak++;
      score += 8 + streak * 4;
    } else {
      streak = 0;
      // 跳过的距离要扣分，但有下限，免得长标题被一票否决。
      score -= Math.min(found - ti, 12);
    }
    if (isBoundary(found === 0 ? "" : t[found - 1])) score += 10;
    ti = found + 1;
  }
  // 命中密度：同样命中，宿主越短越相关。
  score += Math.max(0, 30 - (t.length - q.length));
  return score;
};

/**
 * 给 `query` 在 `text` 中的匹配打分，不匹配返回 `null`。
 *
 * 分数只用于**排序**，绝对值没有意义。约定：越大越好。
 */
export const fuzzyScore = (text: string, query: string): number | null => {
  if (!query) return 0;
  if (!text) return null;
  return scoreLowered(text.toLowerCase(), query.toLowerCase());
};

/** 一行歌曲里参与匹配的文本：曲名、专辑名、每一位艺人名。 */
const songFields = (row: any): string[] => {
  const fields: string[] = [];
  if (row?.name) fields.push(String(row.name));
  if (row?.album?.name) fields.push(String(row.album.name));
  const artists = Array.isArray(row?.artist) ? row.artist : [];
  for (const a of artists) if (a?.name) fields.push(String(a.name));
  return fields;
};

/**
 * 小写化后的字段，按行缓存。
 *
 * 万首歌单整表过滤时，`toLowerCase()` 是这条路径上最贵的一步：每行 4 个字段
 * × 每敲一个键一次。实测 10000 行、关键词 `a`，不缓存 37 ms，缓存后 18 ms。
 * 而列表条目是 `markRaw` 的、构造后再不改写（见 `utils/rawEntry`），所以
 * 「这一行的小写字段」是可以一次算完永久有效的。
 *
 * 用 `WeakMap`：键就是行对象本身，换歌单、过滤出新数组都不需要清理，行被丢掉
 * 时缓存条目跟着回收。
 */
const loweredCache = new WeakMap<object, string[]>();

const loweredFields = (row: any): string[] => {
  if (!row || typeof row !== "object") return [];
  const hit = loweredCache.get(row);
  if (hit) return hit;
  const fields = songFields(row).map((s) => s.toLowerCase());
  loweredCache.set(row, fields);
  return fields;
};

/**
 * 取歌曲各字段里的最佳得分，不匹配返回 `null`。
 *
 * 曲名权重最高：搜「周杰伦」时匹配到艺人当然算，但同样分数下应该让曲名命中排前面。
 *
 * `query` 必须**已经**小写。这个函数在整表过滤里是逐行调用的，把小写化留给调用方
 * 做一次，而不是每行重做一遍。
 */
export const fuzzyScoreSong = (row: any, loweredQuery: string): number | null => {
  const fields = loweredFields(row);
  let best: number | null = null;
  for (let i = 0; i < fields.length; i++) {
    const s = scoreLowered(fields[i], loweredQuery);
    if (s === null) continue;
    // i === 0 是曲名。
    const weighted = i === 0 ? s + 40 : s;
    if (best === null || weighted > best) best = weighted;
  }
  return best;
};

/**
 * 过滤并按相关度排序。
 *
 * 排序是稳定的（`Array.prototype.sort` 在现代引擎里保证），所以同分行保持原始
 * 列表顺序——这很重要：一张专辑里同分的曲目应该还是按曲序排。
 *
 * 得分按「行 × 当前关键词」缓存。长流页面在补齐期间会一块一块地追加行，每次追加
 * 都要拿**同一个**关键词把整张表重过一遍；没有这层缓存，万首歌单分十块补齐就是
 * 十次全表打分（实测 27→12 ms，四十块时 79→25 ms）。关键词一变就整片作废，因为
 * 分数是相对关键词而言的。
 */
let scoreMemoQuery = "";
let scoreMemo = new WeakMap<object, number | null>();

export const fuzzyFilterSongs = <T>(rows: readonly T[], query: string): T[] => {
  const q = query.trim().toLowerCase();
  if (!q) return rows as T[];
  if (scoreMemoQuery !== q) {
    scoreMemoQuery = q;
    scoreMemo = new WeakMap();
  }
  const scored: { row: T; score: number }[] = [];
  for (const row of rows) {
    const keyed = typeof row === "object" && row !== null ? (row as object) : null;
    let score = keyed ? scoreMemo.get(keyed) : undefined;
    if (score === undefined) {
      score = fuzzyScoreSong(row, q);
      if (keyed) scoreMemo.set(keyed, score);
    }
    if (score !== null) scored.push({ row, score });
  }
  scored.sort((a, b) => b.score - a.score);
  return scored.map((s) => s.row);
};

/*
 * 关于拼音：`bysj` → 《不由自主》这类首字母匹配这里**没有**实现。它需要一张
 * 汉字→拼音表（最小的也有上百 KB），属于要单独权衡包体的事，不该顺手塞进来。
 */
