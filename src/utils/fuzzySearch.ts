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
 * 给 `query` 在 `text` 中的匹配打分，不匹配返回 `null`。
 *
 * 分数只用于**排序**，绝对值没有意义。约定：越大越好。
 */
export const fuzzyScore = (text: string, query: string): number | null => {
  if (!query) return 0;
  if (!text) return null;

  const t = text.toLowerCase();
  const q = query.toLowerCase();

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
 * 取歌曲各字段里的最佳得分，不匹配返回 `null`。
 *
 * 曲名权重最高：搜「周杰伦」时匹配到艺人当然算，但同样分数下应该让曲名命中排前面。
 */
export const fuzzyScoreSong = (row: any, query: string): number | null => {
  const fields = songFields(row);
  let best: number | null = null;
  for (let i = 0; i < fields.length; i++) {
    const s = fuzzyScore(fields[i], query);
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
 */
export const fuzzyFilterSongs = <T>(rows: T[], query: string): T[] => {
  const q = query.trim();
  if (!q) return rows;
  const scored: { row: T; score: number }[] = [];
  for (const row of rows) {
    const score = fuzzyScoreSong(row, q);
    if (score !== null) scored.push({ row, score });
  }
  scored.sort((a, b) => b.score - a.score);
  return scored.map((s) => s.row);
};

/*
 * 关于拼音：`bysj` → 《不由自主》这类首字母匹配这里**没有**实现。它需要一张
 * 汉字→拼音表（最小的也有上百 KB），属于要单独权衡包体的事，不该顺手塞进来。
 */
